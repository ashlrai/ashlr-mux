#!/usr/bin/env python3
"""Join two capture_driver NDJSON captures and emit the normalized differential.

Thin CLI over scripts/parity/differential_harness.py::compare_observations:
joins the canonical and windows captures by case id, applies each case's
JSON-pointer approved_differences (recorded identically in both captures from
the shared manifest), and reports every non-identical case with per-lane
detail. A case present on only one side is a delta, never a skip; a case with
a recorded capture_error on either side is a delta even if the (partial)
observations happen to match.

Exit status: 0 when every case is identical, 1 otherwise.
"""

from __future__ import annotations

import argparse
import copy
import json
import sys
from pathlib import Path
from typing import Any

from differential_harness import compare_observations, remove_pointer


class CaptureFormatError(ValueError):
    """A capture file is structurally invalid."""


def load_capture(text: str, label: str) -> dict[str, dict[str, Any]]:
    """Parse one NDJSON capture into {case_id: record}. Session lines are
    validated and skipped; duplicate case ids are rejected."""
    cases: dict[str, dict[str, Any]] = {}
    for number, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise CaptureFormatError(f"{label}:{number}: invalid JSON: {error}") from error
        if not isinstance(record, dict):
            raise CaptureFormatError(f"{label}:{number}: record must be an object")
        kind = record.get("type")
        if kind == "session":
            continue
        if kind != "case":
            raise CaptureFormatError(f"{label}:{number}: unknown record type {kind!r}")
        case_id = record.get("id")
        if not isinstance(case_id, str) or not case_id:
            raise CaptureFormatError(f"{label}:{number}: case record requires a string id")
        if case_id in cases:
            raise CaptureFormatError(f"{label}:{number}: duplicate case id {case_id!r}")
        if not isinstance(record.get("observation"), dict):
            raise CaptureFormatError(f"{label}:{number}: case {case_id!r} missing observation")
        cases[case_id] = record
    return cases


def _normalized_lane_values(
    observation: dict[str, Any], approved: list[dict[str, Any]], lane: str
) -> Any:
    """The lane value after removing this case's approved-difference pointers."""
    normalized = copy.deepcopy(observation)
    for difference in approved:
        remove_pointer(normalized, difference["path"])
    return normalized.get(lane)


def compare_captures(
    canonical: dict[str, dict[str, Any]], windows: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    """Join by case id and diff every case. Returns the normalized report."""
    results: list[dict[str, Any]] = []
    # Preserve canonical capture order, then any windows-only strays.
    ordered_ids = list(canonical) + [case_id for case_id in windows if case_id not in canonical]
    for case_id in ordered_ids:
        left = canonical.get(case_id)
        right = windows.get(case_id)
        if left is None or right is None:
            results.append(
                {
                    "id": case_id,
                    "identical": False,
                    "mismatches": ["missing_canonical" if left is None else "missing_windows"],
                    "capture_errors": {
                        "canonical": (left or {}).get("capture_error"),
                        "windows": (right or {}).get("capture_error"),
                    },
                    "detail": {},
                }
            )
            continue

        approved_left = left.get("approved_differences", [])
        approved_right = right.get("approved_differences", [])
        if approved_left != approved_right:
            results.append(
                {
                    "id": case_id,
                    "identical": False,
                    "mismatches": ["approved_differences_mismatch"],
                    "capture_errors": {
                        "canonical": left.get("capture_error"),
                        "windows": right.get("capture_error"),
                    },
                    "detail": {
                        "approved_differences": {
                            "canonical": approved_left,
                            "windows": approved_right,
                        }
                    },
                }
            )
            continue

        case = {"approved_differences": approved_left}
        mismatches = compare_observations(left["observation"], right["observation"], case)
        capture_errors = {
            "canonical": left.get("capture_error"),
            "windows": right.get("capture_error"),
        }
        has_capture_error = any(capture_errors.values())
        detail = {
            lane: {
                "canonical": _normalized_lane_values(
                    left["observation"], approved_left, lane
                ),
                "windows": _normalized_lane_values(
                    right["observation"], approved_left, lane
                ),
            }
            for lane in mismatches
        }
        results.append(
            {
                "id": case_id,
                "identical": not mismatches and not has_capture_error,
                "mismatches": list(mismatches)
                + (["capture_error"] if has_capture_error else []),
                "capture_errors": capture_errors,
                "detail": detail,
            }
        )

    deltas = [r for r in results if not r["identical"]]
    return {
        "cases": len(results),
        "identical": len(results) - len(deltas),
        "deltas": len(deltas),
        "results": results,
    }


def render_summary(report: dict[str, Any]) -> str:
    lines = [
        f"cases: {report['cases']}  identical: {report['identical']}  deltas: {report['deltas']}"
    ]
    for result in report["results"]:
        if result["identical"]:
            continue
        lines.append(f"DELTA {result['id']}: lanes={','.join(result['mismatches'])}")
        for side, error in result["capture_errors"].items():
            if error:
                lines.append(f"  capture_error[{side}]: {error}")
    if report["deltas"] == 0:
        lines.append("PASS: zero deltas")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--canonical", required=True, type=Path)
    parser.add_argument("--windows", required=True, type=Path)
    parser.add_argument("--output", type=Path, help="normalized-diff.json path")
    args = parser.parse_args(argv)

    canonical = load_capture(args.canonical.read_text(encoding="utf-8"), "canonical")
    windows = load_capture(args.windows.read_text(encoding="utf-8"), "windows")
    report = compare_captures(canonical, windows)
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    print(render_summary(report))
    return 1 if report["deltas"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
