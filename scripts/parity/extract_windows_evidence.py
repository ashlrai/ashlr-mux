#!/usr/bin/env python3
"""Deterministically extract Windows/Tauri parity evidence from repository source.

This intentionally records distinct claims for advertisement, dispatch routing,
explicit unsupported responses, CLI classification/mapping/help, and test-source
references.  None of those claims alone is promoted to behavioral verification.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import tempfile
from pathlib import Path
from typing import Iterable


CONTROL = Path("apps/desktop/src-tauri/src/control_socket.rs")
CLASSIFY = Path("crates/cmux-cli/src/classify.rs")
FORWARD = Path("crates/cmux-cli/src/command_forward.rs")
DISPATCH = Path("crates/cmux-cli/src/dispatch.rs")
DEFAULT_OUTPUT = Path("docs/parity/source/windows_evidence.json")
PINNED_WINDOWS_COMMIT = "73dacea41d6a38f75db36d696825aef7547838ce"
SOURCE_ROOTS = (
    "apps/desktop/src-tauri/src",
    "apps/desktop/web/src",
    "crates/cmux-cli",
)
SOURCE_SUFFIXES = {".rs", ".ts", ".tsx"}


def line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def loc(path: Path, text: str, offset: int) -> dict[str, object]:
    return {"path": path.as_posix(), "line": line_number(text, offset)}


def rust_mask(text: str) -> str:
    """Mask Rust comments and strings while retaining punctuation/newlines."""
    out = list(text)
    i = 0
    state = "code"
    block_depth = 0
    raw_hashes = 0
    while i < len(text):
        if state == "code":
            if text.startswith("//", i):
                out[i] = out[i + 1] = " "
                i += 2
                state = "line_comment"
            elif text.startswith("/*", i):
                out[i] = out[i + 1] = " "
                i += 2
                block_depth = 1
                state = "block_comment"
            elif text[i] == '"':
                out[i] = " "
                i += 1
                state = "string"
            elif text[i] == "r":
                match = re.match(r'r(#{0,16})"', text[i:])
                if match:
                    raw_hashes = len(match.group(1))
                    for j in range(i, i + match.end()):
                        out[j] = " "
                    i += match.end()
                    state = "raw_string"
                else:
                    i += 1
            else:
                i += 1
        elif state == "line_comment":
            if text[i] == "\n":
                state = "code"
            else:
                out[i] = " "
            i += 1
        elif state == "block_comment":
            if text.startswith("/*", i):
                out[i] = out[i + 1] = " "
                block_depth += 1
                i += 2
            elif text.startswith("*/", i):
                out[i] = out[i + 1] = " "
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "code"
            else:
                if text[i] != "\n":
                    out[i] = " "
                i += 1
        elif state == "string":
            if text[i] == "\\":
                out[i] = " "
                if i + 1 < len(text):
                    out[i + 1] = " " if text[i + 1] != "\n" else "\n"
                i += 2
            elif text[i] == '"':
                out[i] = " "
                i += 1
                state = "code"
            else:
                if text[i] != "\n":
                    out[i] = " "
                i += 1
        else:
            closing = '"' + ("#" * raw_hashes)
            if text.startswith(closing, i):
                for j in range(i, i + len(closing)):
                    out[j] = " "
                i += len(closing)
                state = "code"
            else:
                if text[i] != "\n":
                    out[i] = " "
                i += 1
    return "".join(out)


def matching_brace(masked: str, opening: int) -> int:
    depth = 0
    for i in range(opening, len(masked)):
        if masked[i] == "{":
            depth += 1
        elif masked[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    raise ValueError(f"unmatched brace at byte {opening}")


def function_body(text: str, function_name: str) -> tuple[int, int]:
    masked = rust_mask(text)
    match = re.search(rf"\bfn\s+{re.escape(function_name)}\b", masked)
    if not match:
        raise ValueError(f"function not found: {function_name}")
    opening = masked.find("{", match.end())
    return opening + 1, matching_brace(masked, opening)


def production_function_source(
    root: Path, function_name: str
) -> tuple[Path, str, tuple[int, int]]:
    candidates = [CONTROL]
    control_modules = root / CONTROL.parent / "control_socket"
    if control_modules.exists():
        candidates.extend(
            path.relative_to(root)
            for path in sorted(control_modules.glob("*.rs"))
            if path.name != "unit_tests.rs"
        )
    matches = []
    for relative in candidates:
        text = (root / relative).read_text(encoding="utf-8")
        try:
            body = function_body(text, function_name)
        except ValueError:
            continue
        matches.append((relative, text, body))
    if len(matches) != 1:
        raise ValueError(
            f"expected one production definition of {function_name}, found {len(matches)}"
        )
    return matches[0]


def named_array(text: str, name: str) -> tuple[list[tuple[str, int]], tuple[int, int]]:
    masked = rust_mask(text)
    match = re.search(rf"\bconst\s+{re.escape(name)}\s*:[^=]+\=\s*&\s*\[", masked)
    if not match:
        raise ValueError(f"array not found: {name}")
    opening = masked.find("[", match.end() - 1)
    depth = 0
    closing = -1
    for i in range(opening, len(masked)):
        if masked[i] == "[":
            depth += 1
        elif masked[i] == "]":
            depth -= 1
            if depth == 0:
                closing = i
                break
    if closing < 0:
        raise ValueError(f"unterminated array: {name}")
    values = [(m.group(1), opening + 1 + m.start()) for m in re.finditer(r'"([^"\\]+)"', text[opening + 1 : closing])]
    return values, (opening, closing)


def split_match_arms(text: str, body_start: int, body_end: int) -> list[tuple[str, int]]:
    masked = rust_mask(text)
    arms: list[tuple[str, int]] = []
    start = body_start
    paren = bracket = brace = angle = 0
    i = body_start
    while i < body_end:
        ch = masked[i]
        if ch == "(": paren += 1
        elif ch == ")": paren -= 1
        elif ch == "[": bracket += 1
        elif ch == "]": bracket -= 1
        elif ch == "{": brace += 1
        elif ch == "}": brace -= 1
        elif ch == "," and paren == bracket == brace == angle == 0:
            if text[start:i].strip():
                arms.append((text[start:i].strip(), start + len(text[start:i]) - len(text[start:i].lstrip())))
            start = i + 1
        i += 1
    if text[start:body_end].strip():
        arms.append((text[start:body_end].strip(), start))
    return arms


def indented_match_arms(text: str, body_start: int, body_end: int, indent: int = 8) -> list[tuple[str, int]]:
    """Split a formatted Rust match, including block arms that omit commas."""
    body = text[body_start:body_end]
    prefix = " " * indent
    candidates: list[int] = []
    for match in re.finditer(r"(?m)^" + re.escape(prefix) + r"(?=\S)", body):
        absolute = body_start + match.start()
        line = text[absolute : text.find("\n", absolute) if "\n" in text[absolute:] else body_end]
        stripped = line.strip()
        if (
            stripped.startswith('"')
            or stripped.startswith("method @")
            or stripped.startswith("method if ")
            or stripped.startswith("_ =>")
        ):
            candidates.append(absolute)
    arms: list[tuple[str, int]] = []
    for index, start in enumerate(candidates):
        end = candidates[index + 1] if index + 1 < len(candidates) else body_end
        segment = text[start:end].strip().rstrip(",")
        if "=>" in segment:
            arms.append((segment, start))
    return arms


def match_body_in_function(text: str, function: str, marker: str) -> tuple[int, int]:
    fn_start, fn_end = function_body(text, function)
    masked = rust_mask(text)
    pos = masked.find(marker, fn_start, fn_end)
    if pos < 0:
        raise ValueError(f"match marker not found in {function}: {marker}")
    opening = masked.find("{", pos + len(marker), fn_end)
    return opening + 1, matching_brace(masked, opening)


def strings_in_pattern(pattern: str) -> list[str]:
    return re.findall(r'"([^"\\]+)"', pattern)


def reference_index(root: Path, roots: Iterable[Path]) -> dict[str, tuple[list[dict], list[dict]]]:
    index: dict[str, tuple[list[dict], list[dict]]] = {}
    for source_root in roots:
        base = root / source_root
        if not base.exists():
            continue
        paths = [base] if base.is_file() else sorted(p for p in base.rglob("*") if p.suffix in {".rs", ".ts", ".tsx"})
        for path in paths:
            text = path.read_text(encoding="utf-8")
            test_boundary = text.find("#[cfg(test)]")
            for match in re.finditer(r'"([^"\\]+)"', text):
                token = match.group(1)
                item = {"path": path.relative_to(root).as_posix(), "line": line_number(text, match.start())}
                is_test = ".test." in path.name or "/tests/" in path.as_posix() or (test_boundary >= 0 and match.start() > test_boundary)
                implementation, tests = index.setdefault(token, ([], []))
                (tests if is_test else implementation).append(item)
    key = lambda item: (item["path"], item["line"])
    for implementation, tests in index.values():
        implementation.sort(key=key)
        tests.sort(key=key)
    return index


def extract_lifecycle_seam_routes(text: str) -> dict[str, dict]:
    """Methods diverted before the legacy match by control_request_route_for_method.

    handle_control_request routes these to
    pane_surface_lifecycle::dispatch_lifecycle_request, so they never reach the
    legacy `match request.method.as_str()` arms the main extractor parses.
    """
    try:
        start, end = match_body_in_function(text, "control_request_route_for_method", "match method")
    except ValueError:
        return {}
    seam: dict[str, dict] = {}
    for arm, offset in indented_match_arms(text, start, end):
        if "=>" not in arm:
            continue
        lhs, rhs = arm.split("=>", 1)
        rhs_variant = rhs.strip().rstrip(",").rsplit("::", 1)[-1]
        if rhs_variant == "Legacy" or not rhs_variant.isidentifier():
            continue
        handler = {
            "PaneSurfaceLifecycle": "pane_surface_lifecycle::dispatch_lifecycle_request",
            "WindowLifecycle": "window_lifecycle::dispatch_window_lifecycle_request",
        }.get(rhs_variant, f"lifecycle_seam::{rhs_variant}")
        for method in strings_in_pattern(lhs):
            seam[method] = {
                "handler": handler,
                "route_source": loc(CONTROL, text, offset),
                "explicit_not_supported": False,
                "unsupported_message": None,
            }
    return seam


def extract_routes(text: str) -> tuple[dict[str, dict], list[dict]]:
    start, end = match_body_in_function(text, "handle_control_request", "match request.method.as_str()")
    routes: dict[str, dict] = {}
    guarded: list[dict] = []
    for arm, offset in indented_match_arms(text, start, end):
        if "=>" not in arm:
            continue
        lhs, rhs = arm.split("=>", 1)
        methods = strings_in_pattern(lhs)
        handler_match = re.search(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(", rhs)
        handler = handler_match.group(1) if handler_match else "inline"
        unsupported = "not_supported" in rhs
        if methods:
            for method in methods:
                routes[method] = {
                    "handler": handler,
                    "route_source": loc(CONTROL, text, offset),
                    "explicit_not_supported": unsupported,
                    "unsupported_message": next(iter(re.findall(r'not_supported\s*\(\s*"([^"]+)"', rhs)), None),
                }
        elif " if " in lhs:
            guarded.append({
                "pattern": " ".join(lhs.split()),
                "handler": handler,
                "explicit_not_supported": unsupported,
                "source": loc(CONTROL, text, offset),
            })
    routes.update(extract_lifecycle_seam_routes(text))
    return routes, guarded


def extract_cli_mappings(text: str) -> dict[str, dict]:
    start, end = match_body_in_function(text, "control_command_for", "match command")
    result: dict[str, dict] = {}
    for arm, offset in indented_match_arms(text, start, end):
        if "=>" not in arm:
            continue
        lhs, rhs = arm.split("=>", 1)
        commands = strings_in_pattern(lhs)
        if not commands:
            continue
        targets = sorted(set(re.findall(r'ControlCommand::new\s*\(\s*"([^"]+)"', rhs)))
        helper_calls = re.findall(r"\b([a-zA-Z_][a-zA-Z0-9_]*)\s*\(", rhs)
        helper = None if targets else next(
            (
                call
                for call in helper_calls
                if call not in {"Some", "Ok", "Err", "new", "json"}
            ),
            None,
        )
        kind = "direct_control" if targets else ("helper_dispatch" if helper else "conditional")
        for command in commands:
            result[command] = {
                "kind": kind,
                "targets": targets,
                "helper": helper,
                "aliases_in_same_arm": sorted(value for value in commands if value != command),
                "source": loc(FORWARD, text, offset),
            }
    return result


def extract_help(text: str) -> dict[str, dict]:
    fn_start, fn_end = function_body(text, "mapped_subcommand_usage")
    masked = rust_mask(text)
    marker = masked.find("match command", fn_start, fn_end)
    opening = masked.find("{", marker, fn_end)
    start, end = opening + 1, matching_brace(masked, opening)
    result: dict[str, dict] = {}
    for arm, offset in indented_match_arms(text, start, end):
        if "=>" not in arm:
            continue
        lhs, rhs = arm.split("=>", 1)
        commands = strings_in_pattern(lhs)
        if not commands:
            continue
        usage_match = re.search(r'"((?:[^"\\]|\\.)*)"', rhs, re.S)
        usage = bytes(usage_match.group(1), "utf-8").decode("unicode_escape") if usage_match else None
        summary = usage.splitlines()[1].strip() if usage and len(usage.splitlines()) > 1 else None
        for command in commands:
            result[command] = {
                "source": loc(DISPATCH, text, offset),
                "usage_summary": summary,
                "aliases_in_same_arm": sorted(value for value in commands if value != command),
            }
    return result


LOCAL_NAMED = {
    "help", "remote-daemon-status", "vm-pty-connect", "docs", "welcome",
    "sessions", "session-debug", "version",
}
SPECIAL_EXECUTORS = {
    "rpc": "raw_rpc", "__tmux-compat": "tmux_compat", "events": "event_stream",
    "ssh": "ssh", "feed-hook": "feed_hook", "feed": "feed", "hooks": "hooks",
    "setup-hooks": "hooks", "uninstall-hooks": "hooks",
    "new-window": "window_lifecycle", "focus-window": "window_lifecycle",
    "close-window": "window_lifecycle",
}


DOMAIN_PATTERNS = {
    "control_socket": ["apps/desktop/src-tauri/src/control_socket.rs", "crates/cmux-cli/src/*.rs"],
    "session_workspace_pane_surface": ["apps/desktop/src-tauri/src/session.rs", "apps/desktop/web/src/session/*", "apps/desktop/web/src/components/Workspace*", "apps/desktop/web/src/components/SplitTree*"],
    "terminal_tmux": ["apps/desktop/src-tauri/src/terminal.rs", "apps/desktop/web/src/components/TerminalSurface*", "crates/cmux-cli/src/tmux_compat.rs"],
    "browser": ["apps/desktop/src-tauri/src/browser.rs", "apps/desktop/src-tauri/src/browser_import.rs", "apps/desktop/web/src/components/BrowserSurface*"],
    "agents_feed_team_workflows": ["apps/desktop/src-tauri/src/agent_session.rs", "apps/desktop/src-tauri/src/feed.rs", "apps/desktop/web/src/components/AgentSessionSurface.tsx", "apps/desktop/web/src/components/FeedPanel*"],
    "settings_configuration": ["apps/desktop/src-tauri/src/app_settings.rs", "apps/desktop/src-tauri/src/config.rs", "apps/desktop/web/src/settings/*", "apps/desktop/web/src/components/Settings*"],
    "sidebar_custom_sidebar": ["apps/desktop/src-tauri/src/right_sidebar.rs", "apps/desktop/src-tauri/src/sidebar_render.rs", "apps/desktop/web/src/sidebar/*", "apps/desktop/web/src/components/Sidebar*", "apps/desktop/web/src/components/CustomSidebarSurface*"],
    "files_markdown_diffs_projects": ["apps/desktop/src-tauri/src/file_explorer.rs", "apps/desktop/src-tauri/src/diff.rs", "apps/desktop/src-tauri/src/markdown.rs", "apps/desktop/src-tauri/src/open_file.rs", "apps/desktop/src-tauri/src/open_folder.rs", "apps/desktop/web/src/components/File*", "apps/desktop/web/src/components/Markdown*", "apps/desktop/web/src/components/Diff*"],
    "notifications": ["apps/desktop/src-tauri/src/notifications.rs", "apps/desktop/web/src/components/NotificationsOverlay*", "apps/desktop/web/src/host/notifications*"],
    "remote_ssh": ["apps/desktop/src-tauri/src/remote_proxy.rs", "crates/cmux-cli/src/ssh.rs", "crates/cmux-cli/src/remote_daemon_status.rs"],
    "auth_ai_accounts": ["apps/desktop/src-tauri/src/auth_environment.rs", "apps/desktop/src-tauri/src/agent_session.rs"],
    "vm_cloud_mobile": ["apps/desktop/src-tauri/src/mobile_pairing.rs", "crates/cmux-cli/src/vm_pty_connect.rs"],
    "native_services_lifecycle": ["apps/desktop/src-tauri/src/global_hotkey.rs", "apps/desktop/src-tauri/src/updater_status.rs", "apps/desktop/src-tauri/src/window.rs", "apps/desktop/src-tauri/src/default_terminal.rs", "apps/desktop/src-tauri/src/lib.rs"],
    "web_ui_shell": ["apps/desktop/web/src/App.tsx", "apps/desktop/web/src/tauri-bridge*", "apps/desktop/web/src/components/WindowTitlebar*", "apps/desktop/web/src/palette/*"],
}


def domain_sources(root: Path) -> list[dict]:
    rows = []
    for domain, patterns in DOMAIN_PATTERNS.items():
        paths: set[str] = set()
        for pattern in patterns:
            paths.update(path.relative_to(root).as_posix() for path in root.glob(pattern) if path.is_file())
        rows.append({
            "domain": domain,
            "sources": sorted(paths),
            "test_sources": sorted(path for path in paths if ".test." in path or "/tests/" in path),
        })
    return rows


def build(root: Path) -> dict:
    control_text = (root / CONTROL).read_text(encoding="utf-8")
    classify_text = (root / CLASSIFY).read_text(encoding="utf-8")
    forward_text = (root / FORWARD).read_text(encoding="utf-8")
    dispatch_text = (root / DISPATCH).read_text(encoding="utf-8")
    advertised_values, _ = named_array(control_text, "CONTROL_SOCKET_METHODS")
    routes, guarded = extract_routes(control_text)
    predicate_path, predicate_text, predicate_body_range = production_function_source(
        root, "is_unported_browser_automation_method"
    )
    predicate_start, predicate_end = predicate_body_range
    predicate_body = predicate_text[predicate_start:predicate_end].strip()
    for route in guarded:
        route["predicate_source"] = loc(predicate_path, predicate_text, predicate_start)
        route["predicate_body"] = predicate_body
        route["currently_active"] = predicate_body != "false"
        route["matched_methods"] = [] if predicate_body == "false" else None
    advertised = {value: loc(CONTROL, control_text, offset) for value, offset in advertised_values}
    method_references = reference_index(root, [Path("apps/desktop/src-tauri/src"), Path("crates/cmux-cli")])

    all_methods = sorted(set(advertised) | set(routes))
    method_rows = []
    for method in all_methods:
        impl_refs, test_refs = method_references.get(method, ([], []))
        route = routes.get(method)
        active_unsupported = bool(route and route["explicit_not_supported"])
        if active_unsupported:
            evidence = "explicit_not_supported"
        elif route and test_refs:
            evidence = "routed_with_test_references"
        elif route:
            evidence = "routed_without_test_reference"
        else:
            evidence = "advertised_only"
        method_rows.append({
            "method": method,
            "advertised": method in advertised,
            "routed": method in routes,
            "handler": route["handler"] if route else None,
            "explicit_not_supported": active_unsupported,
            "unsupported_message": route["unsupported_message"] if route else None,
            "evidence_class": evidence,
            "source_locations": {
                "advertisement": advertised.get(method),
                "route": route["route_source"] if route else None,
                "implementation_references": impl_refs,
                "test_references": test_refs,
            },
            "verification_claim": "unverified; source evidence is not behavioral parity proof",
        })

    top_values, _ = named_array(classify_text, "TOP_LEVEL_COMMAND_NAMES")
    usage_values, _ = named_array(classify_text, "SUBCOMMAND_USAGE_COMMANDS")
    top = {value: loc(CLASSIFY, classify_text, offset) for value, offset in top_values}
    usage = {value: loc(CLASSIFY, classify_text, offset) for value, offset in usage_values}
    mappings = extract_cli_mappings(forward_text)
    helps = extract_help(dispatch_text)
    cli_references = reference_index(root, [Path("crates/cmux-cli")])
    cli_rows = []
    reachable_commands = sorted(set(top) | set(mappings) | set(SPECIAL_EXECUTORS))
    for command in reachable_commands:
        impl_refs, test_refs = cli_references.get(command, ([], []))
        mapping = mappings.get(command)
        if command in LOCAL_NAMED:
            classification = "local_or_no_socket"
        elif command in {"settings", "config", "window"}:
            classification = "hybrid_local_and_socket"
        else:
            classification = "needs_socket"
        executor = SPECIAL_EXECUTORS.get(command)
        if mapping:
            dispatch_outcome = "control_mapping"
        elif executor:
            dispatch_outcome = "special_executor"
        elif classification == "local_or_no_socket":
            dispatch_outcome = "local_executor"
        elif classification == "hybrid_local_and_socket":
            dispatch_outcome = "hybrid; unmapped socket forms can fail not_ported"
        else:
            dispatch_outcome = "explicit_socket_command_not_ported"
        cli_rows.append({
            "command": command,
            "top_level_known": True,
            "classification": classification,
            "executor": executor,
            "dispatch_outcome": dispatch_outcome,
            "control_mapping_kind": mapping["kind"] if mapping else None,
            "control_methods": mapping["targets"] if mapping else [],
            "mapping_helper": mapping["helper"] if mapping else None,
            "mapping_aliases": mapping["aliases_in_same_arm"] if mapping else [],
            "has_help_classification": command in usage,
            "has_concrete_help": command in helps,
            "help_usage_summary": helps.get(command, {}).get("usage_summary"),
            "help_aliases": helps.get(command, {}).get("aliases_in_same_arm", []),
            "source_locations": {
                "top_level_classification": top.get(command),
                "help_classification": usage.get(command),
                "control_mapping": mapping["source"] if mapping else None,
                "concrete_help": helps.get(command, {}).get("source"),
                "implementation_references": impl_refs,
                "test_references": test_refs,
            },
            "verification_claim": "unverified; classification/mapping/help presence is not behavioral parity proof",
        })

    direct_unsupported = sorted(row["method"] for row in method_rows if row["explicit_not_supported"])
    return {
        "schema_version": 1,
        "generated_from": {"repository_commit": PINNED_WINDOWS_COMMIT, "source_mode": "git cat-file source snapshot of pinned commit (not ambient worktree)", "platform": "windows-tauri", "extractor": "scripts/parity/extract_windows_evidence.py"},
        "semantics": {
            "warning": "This catalog is source evidence only. Advertised, routed, compiled, or test-referenced does not mean canonically verified.",
            "method_join_key": "control_socket_methods[].method",
            "cli_join_key": "cli_commands[].command",
        },
        "summary": {
            "advertised_method_count": len(advertised),
            "routed_method_count": len(routes),
            "advertised_and_routed_count": sum(row["advertised"] and row["routed"] for row in method_rows),
            "advertised_only_count": sum(row["advertised"] and not row["routed"] for row in method_rows),
            "routed_not_advertised_count": sum(row["routed"] and not row["advertised"] for row in method_rows),
            "explicit_not_supported_method_count": len(direct_unsupported),
            "guarded_not_supported_path_count": sum(row["explicit_not_supported"] for row in guarded),
            "top_level_cli_count": len(cli_rows),
            "cli_direct_control_mapped_count": sum(row["control_mapping_kind"] == "direct_control" for row in cli_rows),
            "cli_any_control_mapping_count": sum(row["control_mapping_kind"] is not None for row in cli_rows),
            "cli_concrete_help_count": sum(row["has_concrete_help"] for row in cli_rows),
            "cli_explicit_socket_not_ported_count": sum(row["dispatch_outcome"] == "explicit_socket_command_not_ported" for row in cli_rows),
        },
        "control_socket_methods": method_rows,
        "guarded_routes": guarded,
        "explicit_unsupported_methods": direct_unsupported,
        "cli_commands": cli_rows,
        "domain_sources": domain_sources(root),
    }


def validate(data: dict) -> list[str]:
    errors: list[str] = []
    methods = data["control_socket_methods"]
    names = [row["method"] for row in methods]
    commands = [row["command"] for row in data["cli_commands"]]
    if names != sorted(set(names)):
        errors.append("control method keys are not unique and sorted")
    if commands != sorted(set(commands)):
        errors.append("CLI command keys are not unique and sorted")
    summary = data["summary"]
    checks = {
        "advertised_method_count": sum(row["advertised"] for row in methods),
        "routed_method_count": sum(row["routed"] for row in methods),
        "explicit_not_supported_method_count": sum(row["explicit_not_supported"] for row in methods),
        "top_level_cli_count": len(commands),
        "cli_direct_control_mapped_count": sum(row["control_mapping_kind"] == "direct_control" for row in data["cli_commands"]),
        "cli_explicit_socket_not_ported_count": sum(row["dispatch_outcome"] == "explicit_socket_command_not_ported" for row in data["cli_commands"]),
    }
    for key, actual in checks.items():
        if summary[key] != actual:
            errors.append(f"summary {key}={summary[key]} but rows imply {actual}")
    for row in methods:
        if row["explicit_not_supported"] and not row["routed"]:
            errors.append(f"unsupported method is not routed: {row['method']}")
        if row["advertised"] and row["source_locations"]["advertisement"] is None:
            errors.append(f"advertised method lacks source: {row['method']}")
        if row["routed"] and row["source_locations"]["route"] is None:
            errors.append(f"routed method lacks source: {row['method']}")
    if data["explicit_unsupported_methods"] != sorted(row["method"] for row in methods if row["explicit_not_supported"]):
        errors.append("explicit_unsupported_methods disagrees with method rows")
    return errors


def materialize_commit_sources(repository_root: Path, commit: str, destination: Path) -> None:
    """Materialize only source blobs consumed by this extractor."""
    listing = subprocess.check_output(
        ["git", "ls-tree", "-r", "--name-only", "-z", commit, "--", *SOURCE_ROOTS],
        cwd=repository_root,
    )
    paths = [
        entry.decode("utf-8")
        for entry in listing.split(b"\0")
        if entry and Path(entry.decode("utf-8")).suffix in SOURCE_SUFFIXES
    ]
    requests = b"".join(f"{commit}:{path}\n".encode("utf-8") for path in paths)
    batch = subprocess.check_output(
        ["git", "cat-file", "--batch"],
        cwd=repository_root,
        input=requests,
    )
    cursor = 0
    for path in paths:
        header_end = batch.find(b"\n", cursor)
        if header_end < 0:
            raise RuntimeError(f"missing git cat-file header for {path}")
        header = batch[cursor:header_end]
        if header.endswith(b" missing"):
            raise RuntimeError(f"missing source blob at {commit}:{path}")
        size = int(header.rsplit(b" ", 1)[1])
        content_start = header_end + 1
        content_end = content_start + size
        relative = Path(path)
        if relative.is_absolute() or ".." in relative.parts:
            raise RuntimeError(f"unsafe source path in commit: {path}")
        output = destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_bytes(batch[content_start:content_end])
        cursor = content_end + 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--commit", default=PINNED_WINDOWS_COMMIT)
    parser.add_argument("--check", action="store_true", help="validate and require generated output to match disk")
    args = parser.parse_args()
    repository_root = args.root.resolve()
    commit = subprocess.check_output(
        ["git", "rev-parse", f"{args.commit}^{{commit}}"],
        cwd=repository_root,
        text=True,
        encoding="utf-8",
    ).strip()
    with tempfile.TemporaryDirectory(prefix="cmux-windows-evidence-") as temporary:
        snapshot_root = Path(temporary)
        try:
            materialize_commit_sources(repository_root, commit, snapshot_root)
        except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
            print(f"error: Windows commit is unavailable: {commit}: {error}")
            return 1
        data = build(snapshot_root)
    data["generated_from"]["repository_commit"] = commit
    errors = validate(data)
    rendered = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    output = args.output if args.output.is_absolute() else repository_root / args.output
    if args.check:
        if not output.exists():
            errors.append(f"generated output missing: {output}")
        elif output.read_text(encoding="utf-8") != rendered:
            errors.append(f"generated output is stale: {output}")
    else:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8", newline="\n")
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    print(json.dumps(data["summary"], sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
