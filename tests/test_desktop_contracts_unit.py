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


if __name__ == "__main__":
    for name, value in sorted(globals().items()):
        if name.startswith("test_") and callable(value):
            value()
    print("PASS: desktop contracts unit")
