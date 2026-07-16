#!/usr/bin/env python3
"""Extract the canonical cmux socket-v2 catalog from a frozen Git commit.

The extractor deliberately reads Git objects rather than the checkout, making the
result independent of the branch's implementation and working-tree state.
"""

from __future__ import annotations

import argparse
import io
import json
import re
import subprocess
from pathlib import Path


DEFAULT_COMMIT = "e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452"
DEFAULT_OUTPUT = Path("docs/parity/source/canonical_v2.json")
CAPABILITY_PATH = "Sources/TerminalController.swift"
DEBUG_PATH = "Sources/TerminalController+DebugMethodNames.swift"
EXPECTED_RELEASE = 261
EXPECTED_DEBUG = 42
METHOD_RE = re.compile(r'"([A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)"')
FUNC_RE = re.compile(r"\bfunc\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", *args], text=True, encoding="utf-8", errors="strict"
    )


def blob(commit: str, path: str) -> str:
    return git("show", f"{commit}:{path}")


def blobs(commit: str, paths: list[str]) -> dict[str, str]:
    """Read many blobs through one cat-file process (important on Windows)."""
    if not paths:
        return {}
    requests = "".join(f"{commit}:{path}\n" for path in paths).encode()
    completed = subprocess.run(
        ["git", "cat-file", "--batch"],
        input=requests,
        stdout=subprocess.PIPE,
        check=True,
    )
    stream = io.BytesIO(completed.stdout)
    result: dict[str, str] = {}
    for path in paths:
        header = stream.readline().decode("ascii").rstrip("\n")
        parts = header.split()
        if len(parts) != 3 or parts[1] != "blob":
            raise RuntimeError(f"cannot read {commit}:{path}: {header}")
        size = int(parts[2])
        data = stream.read(size)
        stream.read(1)  # batch protocol's trailing newline
        result[path] = data.decode("utf-8", errors="strict")
    return result


def source_paths(commit: str) -> list[str]:
    paths = git("ls-tree", "-r", "--name-only", commit).splitlines()
    return sorted(
        path
        for path in paths
        if path.endswith(".swift")
        and (
            path.startswith("Sources/")
            or (
                path.startswith("Packages/macOS/CmuxControlSocket/Sources/")
            )
        )
    )


def pointer_paths(commit: str) -> list[str]:
    paths = git("ls-tree", "-r", "--name-only", commit).splitlines()
    roots = ("tests/", "tests_v2/", "cmuxTests/", "contracts/")
    text_suffixes = (".swift", ".py", ".json", ".jsonl", ".md", ".txt", ".sh")
    return sorted(
        path
        for path in paths
        if path.startswith(roots) and path.endswith(text_suffixes)
    )


def extract_array(
    text: str, start_marker: str, *, stop_marker: str | None = None
) -> list[tuple[str, int]]:
    lines = text.splitlines()
    start = next(i for i, line in enumerate(lines) if start_marker in line)
    found: list[tuple[str, int]] = []
    for index in range(start + 1, len(lines)):
        line = lines[index]
        if stop_marker and stop_marker in line:
            break
        if line.strip() == "]":
            break
        found.extend((method, index + 1) for method in METHOD_RE.findall(line))
    return found


def extract_function_case_methods(text: str, signature: str) -> list[tuple[str, int]]:
    """Extract concrete dotted wire methods from case arms in one Swift function."""
    lines = text.splitlines()
    start = next(i for i, line in enumerate(lines) if signature in line)
    depth = 0
    found: dict[str, int] = {}
    for index in range(start, len(lines)):
        line = lines[index]
        depth += line.count("{")
        depth -= line.count("}")
        if line.lstrip().startswith("case "):
            for method in METHOD_RE.findall(line):
                found.setdefault(method, index + 1)
        if index > start and depth == 0:
            break
    return list(found.items())


def enclosing_symbol(lines: list[str], index: int) -> str | None:
    for candidate in range(index, -1, -1):
        match = FUNC_RE.search(lines[candidate])
        if match:
            return match.group(1)
    return None


def is_dispatch_occurrence(lines: list[str], index: int, method: str) -> bool:
    line = lines[index]
    if re.search(r"\b(case|if|guard)\b", line) and f'"{method}"' in line:
        return True
    # Swift case lists commonly wrap across several lines before their colon.
    for candidate in range(index - 1, max(-1, index - 12), -1):
        prior = lines[candidate]
        if "case " in prior:
            return ":" not in " ".join(lines[candidate:index])
        if "switch " in prior or "default:" in prior:
            break
    return False


def location(path: str, line: int, symbol: str | None = None) -> dict[str, object]:
    value: dict[str, object] = {"path": path, "line": line}
    if symbol:
        value["symbol"] = symbol
    return value


def family(method: str) -> tuple[str, str]:
    parts = method.split(".")
    domain = parts[0]
    nested_families = {"workspace", "surface", "browser", "mobile", "remote"}
    if len(parts) >= 3 and domain in nested_families:
        return domain, ".".join(parts[:2])
    return domain, domain


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--commit", default=DEFAULT_COMMIT)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    commit = git("rev-parse", f"{args.commit}^{{commit}}").strip()
    capabilities = blob(commit, CAPABILITY_PATH)
    advertised_release = extract_array(
        capabilities, "var methods: [String] = [", stop_marker="#if DEBUG"
    )
    advertised_release_names = {name for name, _ in advertised_release}
    mobile_rpc = extract_function_case_methods(capabilities, "func mobileHostHandleRPC(")
    unadvertised_release = [
        (name, line) for name, line in mobile_rpc if name not in advertised_release_names
    ]
    release = advertised_release + unadvertised_release
    debug_text = blob(commit, DEBUG_PATH)
    debug = extract_array(debug_text, "v2DebugMethodNames: [String] = [")

    release_names = [name for name, _ in release]
    debug_names = [name for name, _ in debug]
    errors: list[str] = []
    if len(release_names) != EXPECTED_RELEASE:
        errors.append(f"release count: expected {EXPECTED_RELEASE}, got {len(release_names)}")
    if len(debug_names) != EXPECTED_DEBUG:
        errors.append(f"debug count: expected {EXPECTED_DEBUG}, got {len(debug_names)}")
    if len(set(release_names)) != len(release_names):
        errors.append("release capability array contains duplicate methods")
    if len(set(debug_names)) != len(debug_names):
        errors.append("debug method array contains duplicate methods")
    overlap = sorted(set(release_names) & set(debug_names))
    if overlap:
        errors.append(f"release/debug overlap: {', '.join(overlap)}")

    source_index = {
        path: text.splitlines()
        for path, text in blobs(commit, source_paths(commit)).items()
    }
    pointer_candidates = pointer_paths(commit)
    pointer_blobs = blobs(commit, pointer_candidates)
    pointer_index = {path: text.splitlines() for path, text in pointer_blobs.items()}

    entries: list[dict[str, object]] = []
    cataloged = [
        (name, line, "release", "v2Capabilities")
        for name, line in advertised_release
    ]
    cataloged += [
        (name, line, "release", "mobileHostHandleRPC")
        for name, line in unadvertised_release
    ]
    cataloged += [
        (name, line, "debug_only", "v2DebugMethodNames") for name, line in debug
    ]
    cataloged_names = {name for name, _, _, _ in cataloged}
    source_occurrences: dict[str, list[tuple[str, int]]] = {
        name: [] for name in cataloged_names
    }
    for path, lines in source_index.items():
        for index, line in enumerate(lines):
            for candidate in METHOD_RE.findall(line):
                if candidate in source_occurrences:
                    source_occurrences[candidate].append((path, index))
    pointer_occurrences: dict[str, list[tuple[str, int]]] = {
        name: [] for name in cataloged_names
    }
    for path, lines in pointer_index.items():
        for index, line in enumerate(lines):
            # Tests use both quote styles; extracting tokens is cheaper than
            # scanning every test line once per method.
            candidates = set(METHOD_RE.findall(line))
            candidates.update(
                re.findall(r"'([A-Za-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)'", line)
            )
            for candidate in candidates:
                if candidate in pointer_occurrences:
                    pointer_occurrences[candidate].append((path, index))

    for method, source_line, availability, source_symbol in cataloged:
        dispatch: list[dict[str, object]] = []
        for path, index in source_occurrences[method]:
            lines = source_index[path]
            if source_symbol in ("v2Capabilities", "v2DebugMethodNames") and path in (
                CAPABILITY_PATH,
                DEBUG_PATH,
            ) and index + 1 == source_line:
                continue
            symbol = enclosing_symbol(lines, index)
            item = location(path, index + 1, symbol)
            if is_dispatch_occurrence(lines, index, method):
                dispatch.append(item)

        # A dispatch arm is also the canonical implementation entrypoint even
        # when it immediately delegates to a helper whose name does not contain
        # the wire method. Unrelated policy/focus sets are intentionally not
        # labeled implementations merely because they repeat the method name.
        implementation_locations = list(dispatch)

        pointers = [
            location(path, index + 1)
            for path, index in pointer_occurrences[method]
        ]

        domain, method_family = family(method)
        source_path = CAPABILITY_PATH if availability == "release" else DEBUG_PATH
        entry = {
            "method": method,
            "availability": availability,
            "test_only": False,
            "domain": domain,
            "family": method_family,
            "source_location": location(source_path, source_line, source_symbol),
            "dispatch_locations": dispatch,
            "implementation_locations": implementation_locations,
            "contract_test_pointers": pointers,
        }
        entries.append(entry)
        if not dispatch:
            errors.append(f"{method}: no implementation/dispatch arm found")

    availability_order = {"release": 0, "debug_only": 1}
    entries.sort(
        key=lambda item: (availability_order[str(item["availability"])], item["method"])
    )
    document = {
        "schema_version": 1,
        "canonical_commit": commit,
        "protocol": "cmux-socket",
        "protocol_version": 2,
        "extraction": {
            "generator": "scripts/parity/extract_canonical_v2.py",
            "release_source": CAPABILITY_PATH,
            "debug_source": DEBUG_PATH,
            "classification_rule": "release is the union of the unconditional v2Capabilities array and concrete case methods dispatched by mobileHostHandleRPC; debug_only is appended under #if DEBUG; test_only is reserved for endpoint methods defined only in test targets (none found)",
        },
        "counts": {
            "release": len(release_names),
            "advertised_release": len(advertised_release),
            "unadvertised_release": len(unadvertised_release),
            "debug_only": len(debug_names),
            "test_only": 0,
            "all_release_debug_build": len(release_names) + len(debug_names),
        },
        "discrepancy": None if len(release_names) == EXPECTED_RELEASE else {
            "stated_release_count": EXPECTED_RELEASE,
            "source_release_count": len(release_names),
        },
        "methods": entries,
    }
    rendered = json.dumps(document, indent=2, ensure_ascii=False) + "\n"

    if errors:
        for error in errors:
            print(f"ERROR: {error}")
        return 1
    if args.check:
        if not args.output.exists():
            print(f"ERROR: missing generated catalog: {args.output}")
            return 1
        if args.output.read_text(encoding="utf-8") != rendered:
            print(f"ERROR: generated catalog is stale: {args.output}")
            return 1
        print(
            f"OK: {len(release_names)} release, {len(debug_names)} debug-only, "
            "0 test-only methods; catalog is current"
        )
        return 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(rendered, encoding="utf-8", newline="\n")
    print(
        f"wrote {args.output}: {len(release_names)} release, "
        f"{len(debug_names)} debug-only, 0 test-only methods"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
