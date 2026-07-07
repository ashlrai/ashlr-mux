#!/usr/bin/env python3
"""Unit tests for desktop contract validation helpers."""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "scripts" / "desktop" / "verify_cmux_contracts.py"

spec = importlib.util.spec_from_file_location("verify_cmux_contracts", HELPER)
assert spec and spec.loader
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def load_fixture_inputs() -> tuple[dict[str, object], dict[str, object]]:
    schema = json.loads(module.SCHEMA_PATH.read_text(encoding="utf-8"))
    fixture = json.loads(module.FIXTURE_PATH.read_text(encoding="utf-8"))
    return schema, fixture


def test_validate_bootstrap_fixture_accepts_the_checked_in_fixture() -> None:
    schema, fixture = load_fixture_inputs()
    module.validate_bootstrap_fixture(schema, fixture)


def test_validate_bootstrap_fixture_rejects_unexpected_keys() -> None:
    schema, fixture = load_fixture_inputs()
    fixture["unexpected"] = True

    try:
        module.validate_bootstrap_fixture(schema, fixture)
    except ValueError as error:
        assert "unexpected keys" in str(error)
    else:
        raise AssertionError("unexpected bootstrap keys should fail validation")


def test_validate_bootstrap_fixture_requires_schema_version() -> None:
    schema, fixture = load_fixture_inputs()
    fixture["schemaVersion"] = 0

    try:
        module.validate_bootstrap_fixture(schema, fixture)
    except ValueError as error:
        assert "schemaVersion" in str(error)
    else:
        raise AssertionError("schemaVersion < 1 should fail validation")


def test_validate_socket_fixture_requires_newline_and_expected_payload() -> None:
    try:
        module.validate_socket_fixture(
            '{"jsonrpc":"2.0","method":"not-ping"}',
            needle='"method":"ping"',
            label="socket v2 request fixture",
        )
    except ValueError as error:
        assert "newline-delimited" in str(error)
        assert '"method":"ping"' in str(error)
    else:
        raise AssertionError("invalid socket fixtures should fail validation")


# ---------------------------------------------------------------------------
# Rust <-> web contract parity.
#
# These tests catch drift between the Rust Tauri commands and the TypeScript
# bridge that consumes them WITHOUT needing a native launch (which is blocked
# on Application-Control-locked machines with os error 4551). They read the
# source files as text and assert the shared contract values agree.
# ---------------------------------------------------------------------------

DESKTOP_TAURI_LIB = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "lib.rs"
BRIDGE_TS = ROOT / "apps" / "desktop" / "web" / "src" / "tauri-bridge.ts"
CORE_LIB = ROOT / "crates" / "cmux-core" / "src" / "lib.rs"
AGENT_LIB = ROOT / "crates" / "cmux-agent" / "src" / "lib.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_desktop_lib_registers_its_lib_local_tauri_commands() -> None:
    lib = _read(DESKTOP_TAURI_LIB)
    # ping + desktop_core_status are the two commands defined in lib.rs itself
    # (the terminal/session/agent/command-palette/diff/markdown commands live in
    # their own modules and are registered by module path).
    assert lib.count("#[tauri::command]") == 2
    assert "fn ping()" in lib
    assert "fn desktop_core_status()" in lib
    # Both are registered in the invoke handler. Check per-command containment
    # inside the handler block rather than an exact 2-command string, so adding a
    # module command does not bitrot this contract test.
    handler_start = lib.index("tauri::generate_handler![")
    handler = lib[handler_start : lib.index("]", handler_start)]
    for command in ("ping", "desktop_core_status"):
        assert command in handler, f"invoke handler missing {command!r}"


def test_desktop_core_status_struct_keys_match_the_documented_contract() -> None:
    lib = _read(DESKTOP_TAURI_LIB)
    # The serialized field names the web layer consumes. serde uses the Rust
    # field identifiers verbatim (no rename attribute present).
    assert '#[serde(rename' not in lib, "a rename would change the wire keys"
    for field in (
        "milestone:",
        "platform:",
        "agent_providers:",
        "ipc_fixture_request:",
    ):
        assert field in lib, f"DesktopCoreStatus missing field {field!r}"


def test_ping_command_returns_a_bare_string_matching_the_golden_fixture() -> None:
    lib = _read(DESKTOP_TAURI_LIB)
    # ping() returns the bare "pong" string the bridge passes straight through.
    assert 'fn ping_response() -> &\'static str {\n    "pong"' in lib
    # The golden socket response fixture agrees that a successful ping is pong.
    response = _read(module.RESPONSE_PATH)
    assert '"pong":true' in response


def test_milestone_and_platform_constants_match_the_rust_test_expectations() -> None:
    core = _read(CORE_LIB)
    # These are the exact values desktop_core_status surfaces to the web layer.
    assert 'CMUX_PLATFORM: &str = "windows-m1-core"' in core
    assert '"M1"' in core


def test_agent_provider_order_is_codex_claude_opencode() -> None:
    agent = _read(AGENT_LIB)
    # The web layer renders providers by index; order is part of the contract.
    assert "pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::OpenCode];" in agent
    assert '"codex"' in agent and '"claude"' in agent and '"opencode"' in agent


def test_ipc_fixture_request_matches_the_golden_ping_request_line() -> None:
    lib = _read(DESKTOP_TAURI_LIB)
    # The literal the Rust command frames into ipc_fixture_request.
    assert r'{"id":2,"method":"ping","params":{}}' in lib

    # That literal is exactly the first (non-blank) line of the golden request.
    request_lines = [
        line for line in _read(module.REQUEST_PATH).splitlines() if line.strip()
    ]
    assert request_lines, "golden ping request fixture is empty"
    assert request_lines[0] == '{"id":2,"method":"ping","params":{}}'


def test_bridge_consumes_status_as_a_bare_object_not_an_envelope() -> None:
    bridge = _read(BRIDGE_TS)
    # callNative only unwraps a reply that is a non-null object WITH an `ok`
    # field; otherwise it passes it through. desktop_core_status has no `ok`
    # key, so it must be passed through. Assert the membership-guarded branch
    # the contract relies on still exists.
    assert '"ok" in reply' in bridge
    assert "return reply as T" in bridge


def test_bridge_native_reply_envelope_shapes_match_rust_outputs() -> None:
    bridge = _read(BRIDGE_TS)
    # The three NativeReply<T> shapes the bridge handles. Rust currently emits
    # only bare values (ping -> string, desktop_core_status -> object), but the
    # bridge must still support the ok/err envelopes for future commands.
    assert "{ ok: true; value: T }" in bridge
    assert "{ ok: false; error?: { code?: string; userMessage?: string } }" in bridge
    # Default error message used when userMessage is absent.
    assert '"Native bridge request failed."' in bridge
    # Browser fallback used when window.__TAURI__ is absent.
    assert '"pong (browser fallback)"' in bridge
    assert '"Tauri bridge is unavailable in the current runtime."' in bridge


if __name__ == "__main__":
    for name, value in sorted(globals().items()):
        if name.startswith("test_") and callable(value):
            value()
    print("PASS: desktop contracts unit")
