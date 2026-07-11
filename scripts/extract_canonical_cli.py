#!/usr/bin/env python3
"""Extract the canonical cmux CLI contract from a frozen git revision.

The extractor deliberately reads every source through ``git show``.  Its output
therefore does not depend on which commit is checked out in the worktree.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path


DEFAULT_REVISION = "e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452"
DEFAULT_SOURCE = "CLI/cmux.swift"
EXPECTED_PUBLIC_COUNT = 158

# These accepted entrypoints are intentionally absent from the public command
# count.  The reasons are part of the generated evidence, not an implicit rule.
HIDDEN_REASONS = {
    "__internal_flags": "private capability probe",
    "__tmux-compat": "private tmux shim dispatcher",
    "__codex-teams-watch": "private Codex Teams watcher",
    "__debug-tmux-compat-env": "private tmux environment diagnostic",
    "__diff-viewer-branch": "private diff-viewer regeneration helper",
    "__diff-viewer-refs": "private diff-viewer reference helper",
    "__sigpipe-inspect": "private SIGPIPE diagnostic",
    "__sigpipe-probe": "private SIGPIPE diagnostic",
    "__sigpipe-stdin-pipe-probe": "private SIGPIPE diagnostic",
    "codex-hook": "backward-compatibility hook alias hidden from help",
    "diff-viewer-server": "diff-viewer implementation helper hidden from command help",
    "feed-hook": "backward-compatibility hook alias hidden from help",
    "setup-hooks": "backward-compatibility hook setup alias hidden from help",
    "ssh-session-end": "internal remote session lifecycle entrypoint",
    "uninstall-hooks": "backward-compatibility hook removal alias hidden from help",
    "vm-ssh-attach": "internal VM SSH transport entrypoint",
}

ALIASES = {
    "browser-back": ("browser", "back"),
    "browser-forward": ("browser", "forward"),
    "browser-reload": ("browser", "reload"),
    "browser-status": ("browser", "status"),
    "capture-pane": ("read-screen", None),
    "cloud": ("vm", None),
    "codex-hook": ("hooks", "codex"),
    "detach-tab": ("move-tab-to-new-workspace", None),
    "disable-browser": ("browser", "disable"),
    "enable-browser": ("browser", "enable"),
    "feed-hook": ("hooks", "feed"),
    "focus-webview": ("browser", "focus-webview"),
    "get-url": ("browser", "get-url"),
    "is-webview-focused": ("browser", "is-webview-focused"),
    "login": ("auth", "login"),
    "logout": ("auth", "logout"),
    "navigate": ("browser", "navigate"),
    "open-browser": ("browser", "open"),
    "remote": ("remotes", None),
    "rename-window": ("rename-workspace", None),
    "rename-tab": ("tab-action", "rename"),
    "session-debug": ("sessions", "debug"),
    "setup-hooks": ("hooks", "setup"),
    "surface-resume": ("surface", "resume"),
    "uninstall-hooks": ("hooks", "uninstall"),
    "vm-ssh-attach": ("vm-pty-attach", None),
}


@dataclass(frozen=True)
class DispatchEntry:
    name: str
    line: int
    grouped_names: tuple[str, ...]
    route: str


def git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True,
        encoding="utf-8",
    )
    return result.stdout


def git_show(repo: Path, revision: str, path: str) -> str:
    return git(repo, "show", f"{revision}:{path}")


def matching_brace_end(lines: list[str], start: int) -> int:
    depth = 0
    for index in range(start, len(lines)):
        depth += lines[index].count("{") - lines[index].count("}")
        if index > start and depth == 0:
            return index
    raise ValueError(f"unclosed Swift block beginning at line {start + 1}")


def parse_case_names(text: str) -> tuple[str, ...]:
    return tuple(re.findall(r'"([^"\\]+)"', text.split(":", 1)[0]))


def main_switch_entries(lines: list[str]) -> list[DispatchEntry]:
    start = next(
        index for index, line in enumerate(lines)
        if line.strip() == "switch command {" and index > 3_000
    )
    end = matching_brace_end(lines, start)
    entries: list[DispatchEntry] = []
    depth = 1
    pending_line: int | None = None
    pending = ""
    for index in range(start + 1, end):
        line = lines[index]
        if depth == 1 and re.match(r"^\s*case\s+", line):
            pending_line = index + 1
            pending = line.strip()[5:]
        elif pending_line is not None and depth == 1:
            pending += " " + line.strip()
        if pending_line is not None and ":" in pending:
            names = parse_case_names(pending)
            for name in names:
                entries.append(DispatchEntry(name, pending_line, names, "socket-switch"))
            pending_line = None
            pending = ""
        depth += line.count("{") - line.count("}")
    return entries


def early_dispatch_entries(lines: list[str]) -> list[DispatchEntry]:
    assignment = next(i for i, line in enumerate(lines) if "let command = args[index]" in line)
    switch = next(
        i for i, line in enumerate(lines)
        if line.strip() == "switch command {" and i > assignment
    )
    found: dict[str, DispatchEntry] = {}
    for index in range(assignment, switch):
        line = lines[index]
        # Only equality tests can admit a command. Comparisons in ternaries on
        # the same branch are harmless duplicates and are de-duplicated below.
        names = tuple(re.findall(r'command\s*==\s*"([^"\\]+)"', line))
        for name in names:
            found.setdefault(name, DispatchEntry(name, index + 1, names, "early-dispatch"))
    return list(found.values())


def parse_help_blocks(lines: list[str]) -> dict[str, dict]:
    start = next(i for i, line in enumerate(lines) if "private func subcommandUsage(" in line)
    switch = next(i for i in range(start, len(lines)) if lines[i].strip() == "switch command {")
    end = matching_brace_end(lines, switch)
    blocks: dict[str, dict] = {}
    current_names: tuple[str, ...] = ()
    current_line = 0
    current_text: list[str] = []
    depth = 1

    def finish() -> None:
        if not current_names:
            return
        text = "\n".join(current_text)
        usage = [match.strip() for match in re.findall(r"^\s*Usage:\s*(.+)$", text, re.MULTILINE)]
        flags = sorted(set(re.findall(r"(?<![\w-])--?[A-Za-z][A-Za-z0-9-]*", text)))
        subcommands = discover_subcommands(text, current_names)
        for name in current_names:
            blocks[name] = {
                "location": {"file": DEFAULT_SOURCE, "line": current_line},
                "usage": usage,
                "flags": flags,
                "subcommands": subcommands,
            }

    for index in range(switch + 1, end):
        line = lines[index]
        if depth == 1 and re.match(r"^\s*case\s+", line):
            finish()
            current_names = parse_case_names(line.strip()[5:])
            current_line = index + 1
            current_text = [line]
        elif current_names:
            current_text.append(line)
        depth += line.count("{") - line.count("}")
    finish()
    return blocks


def discover_subcommands(text: str, command_names: tuple[str, ...]) -> list[str]:
    discovered: set[str] = set()
    value_words = {
        "false", "id", "index", "name", "off", "on", "path", "ref",
        "subcommand", "surface", "true", "workspace",
    }
    for usage in re.findall(r"^\s*(?:Usage:\s*)?cmux\s+[^\s]+\s+(.+)$", text, re.MULTILINE):
        literal = re.match(r"([a-z][a-z0-9-]*)\b", usage)
        if literal and literal.group(1) not in value_words:
            discovered.add(literal.group(1))
        for group in re.findall(r"<([a-z0-9-]+(?:\|[a-z0-9-]+)+)>", usage):
            values = set(group.split("|"))
            if not values & value_words:
                discovered.update(values)
    in_section = False
    row_indent = 0
    for line in text.splitlines():
        stripped = line.strip()
        if stripped in {"Subcommands:", "Commands:"}:
            in_section = True
            row_indent = len(line) - len(line.lstrip()) + 2
            continue
        if in_section and not stripped:
            in_section = False
            continue
        indent = len(line) - len(line.lstrip())
        if in_section and stripped.endswith(":") and indent < row_indent:
            in_section = False
            continue
        if in_section and indent == row_indent:
            compact = re.sub(r"\s*\|\s*", "|", stripped)
            match = re.match(r"([a-z][a-z0-9-]*(?:\|[a-z][a-z0-9-]*)*)\b", compact)
            if match:
                discovered.update(match.group(1).split("|"))
    discovered.difference_update(command_names)
    return sorted(discovered)


def supplemental_help(repo: Path, revision: str, accepted: set[str]) -> dict[str, list[dict]]:
    """Find usage contracts split into CLI extension files or static constants."""
    paths = [
        path for path in git(repo, "ls-tree", "-r", "--name-only", revision, "--", "CLI").splitlines()
        if path.endswith(".swift")
    ]
    records: dict[str, list[dict]] = {}
    pattern = re.compile(r"Usage:\s*cmux\s+([a-z][a-z0-9-]*)\b")
    for path in paths:
        file_lines = git_show(repo, revision, path).splitlines()
        for index, line in enumerate(file_lines):
            match = pattern.search(line)
            if not match or match.group(1) not in accepted:
                continue
            name = match.group(1)
            end = min(len(file_lines), index + 180)
            for cursor in range(index + 1, end):
                if '"""' in file_lines[cursor]:
                    end = cursor + 1
                    break
            text = "\n".join(file_lines[index:end])
            records.setdefault(name, []).append({
                "location": {"file": path, "line": index + 1},
                "flags": sorted(set(re.findall(r"(?<![\w-])--?[A-Za-z][A-Za-z0-9-]*", text))),
                "subcommands": discover_subcommands(text, (name,)),
            })
    return records


def global_usage(lines: list[str]) -> dict[str, dict]:
    start = next(i for i, line in enumerate(lines) if "private func usage()" in line)
    commands = next(i for i in range(start, len(lines)) if lines[i].strip() == "Commands:")
    environment = next(i for i in range(commands, len(lines)) if lines[i].strip() == "Environment:")
    result: dict[str, dict] = {}
    for index in range(commands + 1, environment):
        stripped = lines[index].strip()
        if not stripped or stripped.startswith("#"):
            continue
        match = re.match(r"([a-z][a-z0-9-]*)\b", stripped)
        if not match:
            continue
        name = match.group(1)
        record = result.setdefault(name, {"locations": [], "signatures": [], "flags": set()})
        record["locations"].append({"file": DEFAULT_SOURCE, "line": index + 1})
        record["signatures"].append(stripped)
        record["flags"].update(re.findall(r"(?<![\w-])--?[A-Za-z][A-Za-z0-9-]*", stripped))
    for record in result.values():
        record["flags"] = sorted(record["flags"])
    return result


def family(name: str) -> str:
    if name in {"auth", "login", "logout", "ai-accounts"}: return "account"
    if name.startswith("browser") or name in {"navigate", "get-url", "open-browser", "focus-webview", "is-webview-focused", "disable-browser", "enable-browser"}: return "browser"
    if name.startswith("ssh") or name.startswith("vm") or name in {"cloud", "remotes", "remote", "remote-daemon-status"}: return "remote"
    if name.startswith("workspace") or name.endswith("workspace") or "workspace" in name or name in {"next-window", "previous-window", "last-window", "find-window"}: return "workspace"
    if "pane" in name or name in {"new-split", "split-off"}: return "pane"
    if "surface" in name or name in {"rename-tab", "tab-action", "send", "send-key", "read-screen", "trigger-flash"}: return "surface"
    if name.startswith("sidebar") or name == "right-sidebar": return "sidebar"
    if name in {"notify", "list-notifications", "dismiss-notification", "mark-notification-read", "open-notification", "jump-to-unread", "clear-notifications"}: return "notification"
    if name in {"claude-teams", "codex-teams", "omo", "omx", "omc", "codex", "hooks", "setup-hooks", "uninstall-hooks", "claude-hook", "codex-hook", "feed-hook", "feed"}: return "agent"
    if name in {"settings", "config", "shortcuts", "themes", "reload-config", "docs"}: return "configuration"
    if name in {"window", "list-windows", "current-window", "new-window", "focus-window", "close-window"}: return "window"
    if name in {"canvas", "layout"}: return "layout"
    if name in {"tree", "top", "memory", "status", "ping", "capabilities", "identify", "rpc", "events"}: return "introspection"
    if name in {"markdown", "diff", "project", "open"}: return "content"
    if name in {"set-status", "clear-status", "list-status", "set-progress", "clear-progress", "log", "clear-log", "list-log"}: return "metadata"
    if name in {"capture-pane", "resize-pane", "pipe-pane", "wait-for", "swap-pane", "break-pane", "join-pane", "last-pane", "clear-history", "set-hook", "popup", "bind-key", "unbind-key", "copy-mode", "set-buffer", "paste-buffer", "list-buffers", "respawn-pane", "display-message"}: return "tmux-compat"
    return "system"


def validate(catalog: dict) -> None:
    commands = catalog["commands"]
    names = [command["name"] for command in commands]
    if names != sorted(names):
        raise ValueError("commands are not sorted")
    if len(names) != len(set(names)):
        raise ValueError("duplicate accepted command tokens")
    counts = catalog["counts"]
    public = sum(command["visibility"] == "public" for command in commands)
    hidden = len(commands) - public
    if counts != {
        "accepted_top_level_tokens": len(commands),
        "public_top_level_commands": public,
        "hidden_or_internal_top_level_commands": hidden,
        "requested_public_target": EXPECTED_PUBLIC_COUNT,
        "public_target_delta": public - EXPECTED_PUBLIC_COUNT,
    }:
        raise ValueError("catalog counts are internally inconsistent")
    if set(HIDDEN_REASONS) - set(names):
        raise ValueError("a documented hidden command is no longer accepted")
    for command in commands:
        if not command["acceptance_contracts"]:
            raise ValueError(f"{command['name']} lacks an acceptance-contract pointer")
        has_usage_contract = any(
            contract["kind"] in {"global-help", "subcommand-help", "usage-contract"}
            for contract in command["acceptance_contracts"]
        )
        if (command["visibility"] == "public") != has_usage_contract:
            raise ValueError(f"{command['name']} visibility disagrees with canonical help discoverability")


def build_catalog(repo: Path, revision: str) -> dict:
    resolved = git(repo, "rev-parse", f"{revision}^{{commit}}").strip()
    source = git_show(repo, resolved, DEFAULT_SOURCE)
    lines = source.splitlines()
    entries = main_switch_entries(lines) + early_dispatch_entries(lines)
    by_name: dict[str, DispatchEntry] = {}
    for entry in entries:
        by_name.setdefault(entry.name, entry)
    help_blocks = parse_help_blocks(lines)
    usage = global_usage(lines)
    supplemental = supplemental_help(repo, resolved, set(by_name))
    commands = []
    for name, entry in sorted(by_name.items()):
        hidden_reason = HIDDEN_REASONS.get(name)
        help_record = help_blocks.get(name)
        usage_record = usage.get(name)
        flags = set(help_record["flags"] if help_record else [])
        flags.update(usage_record["flags"] if usage_record else [])
        for extra in supplemental.get(name, []):
            flags.update(extra["flags"])
        contracts = [{
            "kind": "dispatcher",
            "location": {"file": DEFAULT_SOURCE, "line": entry.line},
            "route": entry.route,
        }]
        if help_record:
            contracts.append({"kind": "subcommand-help", "location": help_record["location"]})
        if usage_record:
            contracts.extend({"kind": "global-help", "location": loc} for loc in usage_record["locations"])
        known_contract_locations = {
            (contract["kind"], contract["location"]["file"], contract["location"]["line"])
            for contract in contracts
        }
        for extra in supplemental.get(name, []):
            key = ("usage-contract", extra["location"]["file"], extra["location"]["line"])
            if key not in known_contract_locations:
                contracts.append({"kind": "usage-contract", "location": extra["location"]})
        alias = ALIASES.get(name)
        commands.append({
            "name": name,
            "canonical_name": alias[0] if alias else name,
            "alias_expansion": alias[1] if alias else None,
            "aliases": sorted(alias_name for alias_name, target in ALIASES.items() if target[0] == name),
            "visibility": "hidden-or-internal" if hidden_reason else "public",
            "visibility_evidence": hidden_reason,
            "family": family(name),
            "dispatch_group": list(entry.grouped_names),
            "source": {"file": DEFAULT_SOURCE, "line": entry.line},
            "signatures": usage_record["signatures"] if usage_record else [],
            "subcommands": sorted(set(
                (help_record["subcommands"] if help_record else [])
                + [sub for extra in supplemental.get(name, []) for sub in extra["subcommands"]]
            )),
            "significant_flags": sorted(flags),
            "acceptance_contracts": contracts,
        })
    public = sum(command["visibility"] == "public" for command in commands)
    catalog = {
        "schema_version": 1,
        "canonical_revision": resolved,
        "source": DEFAULT_SOURCE,
        "extraction": {
            "generator": "scripts/extract_canonical_cli.py",
            "source_access": "git show <revision>:<path>",
            "scope": "accepted top-level command tokens, aliases, help-discoverable subcommands and flags",
        },
        "counts": {
            "accepted_top_level_tokens": len(commands),
            "public_top_level_commands": public,
            "hidden_or_internal_top_level_commands": len(commands) - public,
            "requested_public_target": EXPECTED_PUBLIC_COUNT,
            "public_target_delta": public - EXPECTED_PUBLIC_COUNT,
        },
        "count_note": (
            "The frozen dispatcher accepts 174 distinct top-level tokens. Sixteen have no canonical Usage/help "
            "contract because they are private diagnostics, implementation helpers, internal transport entrypoints, "
            "or explicitly help-hidden compatibility hooks; excluding those "
            "produces the requested/source-supported 158 public commands. Hidden commands remain cataloged."
        ),
        "commands": commands,
    }
    validate(catalog)
    return catalog


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--revision", default=DEFAULT_REVISION)
    parser.add_argument("--output", type=Path, default=Path("docs/parity/source/canonical_cli.json"))
    parser.add_argument("--check", action="store_true", help="fail if output differs from generated JSON")
    args = parser.parse_args()
    catalog = build_catalog(args.repo.resolve(), args.revision)
    rendered = json.dumps(catalog, indent=2, sort_keys=False, ensure_ascii=False) + "\n"
    output = args.output if args.output.is_absolute() else args.repo / args.output
    if args.check:
        if not output.exists() or output.read_text(encoding="utf-8") != rendered:
            raise SystemExit(f"canonical CLI catalog is stale: {output}")
        print(f"canonical CLI catalog is current: {output}")
    else:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8", newline="\n")
        print(f"wrote {output} ({catalog['counts']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
