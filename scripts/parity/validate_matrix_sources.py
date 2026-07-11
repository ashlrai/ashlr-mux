#!/usr/bin/env python3
"""Validate every matrix source path and line against its pinned Git tree."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--matrix", type=Path, default=ROOT / "docs/parity/parity-matrix.json"
    )
    args = parser.parse_args()
    matrix = json.loads(args.matrix.read_text(encoding="utf-8"))
    baseline = matrix["baseline"]
    cache: dict[tuple[str, str], int | None] = {}
    errors: list[str] = []

    def line_count(commit: str, path: str) -> int | None:
        key = (commit, path)
        if key not in cache:
            result = subprocess.run(
                ["git", "show", f"{commit}:{path}"],
                cwd=ROOT,
                text=True,
                encoding="utf-8",
                errors="replace",
                capture_output=True,
                check=False,
            )
            cache[key] = None if result.returncode else result.stdout.count("\n") + 1
        return cache[key]

    for entry in matrix["entries"]:
        for field, commit in (
            ("canonical_sources", baseline["canonical_commit"]),
            ("windows_sources", baseline["windows_commit"]),
        ):
            for location in entry[field]:
                lines = line_count(commit, location["path"])
                if lines is None:
                    errors.append(f"{entry['id']} {field}: missing {location['path']}")
                elif location.get("line") and location["line"] > lines:
                    errors.append(
                        f"{entry['id']} {field}: {location['path']}:{location['line']} exceeds {lines} lines"
                    )
    if errors:
        print("\n".join(errors))
        return 1
    print(
        f"matrix sources valid: {len(matrix['entries'])} entries, {len(cache)} pinned blobs"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
