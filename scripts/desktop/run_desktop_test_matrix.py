#!/usr/bin/env python3
"""Run the desktop bootstrap test matrix by layer."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]

SUITES = {
    "unit": [
        ("desktop web unit tests", ["bun", "test", "apps/desktop/web/src/tauri-bridge.test.ts"]),
        ("desktop rust unit tests", ["cargo", "test", "-p", "cmux-desktop", "--lib"]),
        ("desktop contract unit tests", [sys.executable, "tests/test_desktop_contracts_unit.py"]),
    ],
    "integration": [
        ("desktop integration tests", [sys.executable, "tests/test_desktop_integration.py"]),
    ],
    "e2e": [
        ("desktop end-to-end tests", [sys.executable, "tests/test_desktop_e2e.py"]),
    ],
}


def run_step(label: str, args: list[str]) -> None:
    print(f"==> {label}")
    subprocess.run(
        args,
        cwd=ROOT,
        check=True,
    )


def selected_steps(mode: str) -> list[tuple[str, list[str]]]:
    if mode == "all":
        steps: list[tuple[str, list[str]]] = []
        for suite_name in ("unit", "integration", "e2e"):
            steps.extend(SUITES[suite_name])
        return steps

    if mode not in SUITES:
        raise SystemExit(f"unknown desktop test suite: {mode}")

    return SUITES[mode]


def main() -> int:
    mode = sys.argv[1] if len(sys.argv) > 1 else "all"
    for label, args in selected_steps(mode):
        run_step(label, args)
    print(f"PASS: desktop test matrix ({mode})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
