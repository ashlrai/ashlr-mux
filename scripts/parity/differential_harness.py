#!/usr/bin/env python3
"""Run parity cases against canonical and Windows runner adapters.

Runner protocol: read one case object from stdin and write one observation
object to stdout. The harness compares every required observation lane and
normalizes only JSON-pointer paths explicitly approved by the case.
"""

from __future__ import annotations

import argparse
import copy
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


OBSERVATION_KEYS = (
    "exit_status",
    "stdout",
    "stderr",
    "error",
    "response",
    "state",
    "events",
    "persistence",
    "selectors",
    "multiwindow",
)


def _decode_pointer_token(token: str) -> str:
    return token.replace("~1", "/").replace("~0", "~")


def remove_pointer(document: Any, pointer: str) -> None:
    if not pointer.startswith("/"):
        raise ValueError(f"approved difference must be a JSON pointer: {pointer}")
    tokens = [_decode_pointer_token(token) for token in pointer[1:].split("/")]
    parent = document
    for token in tokens[:-1]:
        if isinstance(parent, list):
            parent = parent[int(token)]
        elif isinstance(parent, dict) and token in parent:
            parent = parent[token]
        else:
            return
    leaf = tokens[-1]
    if isinstance(parent, list):
        index = int(leaf)
        if 0 <= index < len(parent):
            parent.pop(index)
    elif isinstance(parent, dict):
        parent.pop(leaf, None)


def validate_observation(observation: dict[str, Any], label: str) -> None:
    missing = [key for key in OBSERVATION_KEYS if key not in observation]
    if missing:
        raise ValueError(f"{label} observation missing keys: {', '.join(missing)}")


def compare_observations(
    canonical: dict[str, Any], windows: dict[str, Any], case: dict[str, Any]
) -> list[str]:
    validate_observation(canonical, "canonical")
    validate_observation(windows, "windows")
    left = copy.deepcopy(canonical)
    right = copy.deepcopy(windows)
    for difference in case.get("approved_differences", []):
        pointer = difference.get("path")
        rationale = difference.get("rationale")
        if not isinstance(pointer, str) or not isinstance(rationale, str) or not rationale.strip():
            raise ValueError("approved differences require non-empty path and rationale")
        remove_pointer(left, pointer)
        remove_pointer(right, pointer)
    return [key for key in OBSERVATION_KEYS if left[key] != right[key]]


def run_adapter(executable: str, case: dict[str, Any]) -> dict[str, Any]:
    result = subprocess.run(
        [executable],
        input=json.dumps(case),
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"runner {executable} exited {result.returncode}: {result.stderr.strip()}"
        )
    try:
        observation = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"runner {executable} returned invalid JSON: {error}") from error
    if not isinstance(observation, dict):
        raise RuntimeError(f"runner {executable} must return a JSON object")
    return observation


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", required=True, type=Path)
    parser.add_argument("--canonical-runner", required=True)
    parser.add_argument("--windows-runner", required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    payload = json.loads(args.cases.read_text(encoding="utf-8"))
    cases = payload.get("cases", [])
    results = []
    failures = 0
    for case in cases:
        canonical = run_adapter(args.canonical_runner, case)
        windows = run_adapter(args.windows_runner, case)
        mismatches = compare_observations(canonical, windows, case)
        failures += bool(mismatches)
        results.append({"id": case["id"], "mismatches": mismatches, "passed": not mismatches})
    report = {"cases": len(results), "passed": len(results) - failures, "failed": failures, "results": results}
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
