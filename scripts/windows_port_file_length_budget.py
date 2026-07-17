#!/usr/bin/env python3
"""Keep oversized Windows-port source files from silently growing."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BUDGET = ROOT / ".github/windows-port-file-length-budget.tsv"
SOURCE_ROOTS = (
    Path("apps/desktop/src-tauri/src"),
    Path("apps/desktop/web/src"),
    Path("crates/cmux-cli/src"),
    Path("crates/cmux-config/src"),
    Path("crates/cmux-core/src"),
)
SOURCE_SUFFIXES = {".rs", ".ts", ".tsx"}
DEFAULT_LIMIT = 1_500


def line_count(path: Path) -> int:
    content = path.read_bytes()
    return content.count(b"\n") + int(bool(content) and not content.endswith(b"\n"))


def source_lengths() -> dict[str, int]:
    lengths: dict[str, int] = {}
    for relative_root in SOURCE_ROOTS:
        root = ROOT / relative_root
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if path.is_file() and path.suffix in SOURCE_SUFFIXES:
                relative = path.relative_to(ROOT).as_posix()
                lengths[relative] = line_count(path)
    return lengths


def read_budget(path: Path) -> dict[str, int]:
    entries: dict[str, int] = {}
    for number, raw_line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            raw_limit, relative = line.split("\t", 1)
            limit = int(raw_limit)
        except ValueError as error:
            raise ValueError(f"{path}:{number}: expected max_lines<TAB>path") from error
        if relative in entries:
            raise ValueError(f"{path}:{number}: duplicate path: {relative}")
        entries[relative] = limit
    return entries


def write_budget(path: Path, lengths: dict[str, int], default_limit: int) -> None:
    oversized = sorted(
        ((lines, relative) for relative, lines in lengths.items() if lines > default_limit),
        key=lambda row: (-row[0], row[1]),
    )
    lines = [
        "# Windows-port source file length budget.",
        "# Format: max_lines<TAB>relative path",
        f"# New files may not exceed {default_limit} lines. Reduce entries as files shrink.",
    ]
    lines.extend(f"{count}\t{relative}" for count, relative in oversized)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def check_budget(
    budget: dict[str, int], lengths: dict[str, int], default_limit: int
) -> list[str]:
    errors: list[str] = []
    for relative, allowed in sorted(budget.items()):
        actual = lengths.get(relative)
        if actual is None:
            errors.append(f"stale budget entry for missing file: {relative}")
        elif allowed <= default_limit:
            errors.append(f"unnecessary budget entry at or below {default_limit}: {relative}")
        elif actual > allowed:
            errors.append(f"{relative}: {actual} lines exceeds budget {allowed}")
        elif actual < allowed:
            errors.append(f"{relative}: lower budget from {allowed} to {actual}")
    for relative, actual in sorted(lengths.items()):
        if actual > default_limit and relative not in budget:
            errors.append(
                f"{relative}: {actual} lines exceeds the unbudgeted limit {default_limit}"
            )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--budget", type=Path, default=DEFAULT_BUDGET)
    parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    budget_path = args.budget if args.budget.is_absolute() else ROOT / args.budget
    lengths = source_lengths()
    if args.write:
        write_budget(budget_path, lengths, args.limit)
        print(f"wrote {budget_path}")
        return 0
    try:
        budget = read_budget(budget_path)
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    errors = check_budget(budget, lengths, args.limit)
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"Windows-port file length budget respected ({len(budget)} tracked files).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
