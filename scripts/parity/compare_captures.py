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
import re
import sys
from pathlib import Path
from typing import Any

from capture_driver import TimingSymbolizer, UuidRefCanonicalizer, UuidRenumberer
from differential_harness import compare_observations, remove_pointer


class CaptureFormatError(ValueError):
    """A capture file is structurally invalid."""


_WINDOW_REF_RE = re.compile(r"window:(\d+)$")


def normalize_racy_window_list_order(observation: dict[str, Any]) -> dict[str, Any]:
    """Canonicalize valid ``v2:window.list`` probe rows by stable ref.

    Both implementations enumerate an unordered native window registry, and
    the contract explicitly calls numeric list indices positional and racy.
    Row identity and contents remain strict: normalization is applied only
    when every row has a unique numeric ref and a correct pre-normalization
    positional index. Malformed lists therefore remain visible as deltas.
    """
    normalized = copy.deepcopy(observation)

    def walk(value: Any) -> None:
        if isinstance(value, dict):
            if value.get("op") == "v2:window.list":
                result = value.get("result")
                response = result.get("response") if isinstance(result, dict) else None
                payload = response.get("result") if isinstance(response, dict) else None
                rows = payload.get("windows") if isinstance(payload, dict) else None
                if isinstance(rows, list):
                    keyed_rows: list[tuple[int, dict[str, Any]]] = []
                    for position, row in enumerate(rows):
                        if not isinstance(row, dict) or row.get("index") != position:
                            break
                        match = _WINDOW_REF_RE.fullmatch(str(row.get("ref", "")))
                        if match is None:
                            break
                        keyed_rows.append((int(match.group(1)), row))
                    else:
                        refs = [key for key, _ in keyed_rows]
                        if len(refs) == len(set(refs)):
                            keyed_rows.sort(key=lambda item: item[0])
                            payload["windows"] = [row for _, row in keyed_rows]
                            for position, row in enumerate(payload["windows"]):
                                row["index"] = position
            for item in value.values():
                walk(item)
        elif isinstance(value, list):
            for item in value:
                walk(item)

    walk(normalized)
    return normalized


def has_unsatisfied_settle(value: Any) -> bool:
    """Whether a recorded probe exhausted a bounded settle predicate."""
    if isinstance(value, dict):
        settle = value.get("settle")
        if isinstance(settle, dict) and settle.get("satisfied") is False:
            return True
        return any(has_unsatisfied_settle(item) for item in value.values())
    if isinstance(value, list):
        return any(has_unsatisfied_settle(item) for item in value)
    return False


def load_capture(text: str, label: str) -> dict[str, dict[str, Any]]:
    """Parse one NDJSON capture into {case_id: record}. Session lines are
    validated and skipped; duplicate case ids are rejected.

    The sanctioned events-lane timing normalization (TimingSymbolizer, see
    capture_driver.py) is applied to each case at load time, so
    pre-normalization archives — including the frozen canonical capture,
    which is never rewritten — compare under the same rules as fresh
    captures. Idempotent for captures already symbolized at capture time.
    """
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
        record["observation"]["events"] = TimingSymbolizer().apply(
            record["observation"].get("events")
        )
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


def manifest_approved_differences(manifest: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
    """Per-case approved_differences from the manifest (compare-time source of
    truth). Captures record the pointer set in force at capture time, but the
    reviewed manifest may evolve between captures; passing it overrides the
    recorded sets and skips the both-sides-equal check."""
    return {
        case["id"]: case.get("approved_differences", [])
        for case in manifest.get("cases", [])
    }


def compare_captures(
    canonical: dict[str, dict[str, Any]],
    windows: dict[str, dict[str, Any]],
    approved_overrides: dict[str, list[dict[str, Any]]] | None = None,
) -> dict[str, Any]:
    """Join by case id and diff every case. Returns the normalized report."""
    results: list[dict[str, Any]] = []
    # Preserve canonical capture order, then any windows-only strays.
    ordered_ids = list(canonical) + [case_id for case_id in windows if case_id not in canonical]

    # Renumber <uuid-N> symbols by deterministic traversal order so symbol
    # numbers are independent of non-contractual wire key order. Response and
    # probe identities remain global across the capture. Event-only identities
    # are local to each subscription/case: registering them globally would let
    # one known event-count delta cascade phantom UUID offsets into every later
    # event case. Approved-away regions are excluded from both tables.
    def case_approved(case_id: str, record: dict[str, Any]) -> list[dict[str, Any]]:
        if approved_overrides is not None:
            return approved_overrides.get(case_id, [])
        return record.get("approved_differences", [])

    for side in (canonical, windows):
        for record in side.values():
            record["observation"] = normalize_racy_window_list_order(
                record["observation"]
            )
        ref_canonicalizer = UuidRefCanonicalizer()
        for case_id in ordered_ids:
            record = side.get(case_id)
            if record is None:
                continue
            normalized = copy.deepcopy(record["observation"])
            for difference in case_approved(case_id, record):
                remove_pointer(normalized, difference["path"])
            # Entity refs in response/state probes come from the public handle
            # registry. Event payload refs are themselves under comparison and
            # may be the bug being measured; never let a bad derived-event ref
            # poison otherwise authoritative identity evidence.
            normalized.pop("events", None)
            ref_canonicalizer.register(normalized)
        for record in side.values():
            record["observation"] = ref_canonicalizer.apply(record["observation"])
        renumber = UuidRenumberer()
        for case_id in ordered_ids:
            record = side.get(case_id)
            if record is None:
                continue
            normalized = copy.deepcopy(record["observation"])
            for difference in case_approved(case_id, record):
                remove_pointer(normalized, difference["path"])
            normalized.pop("events", None)
            renumber.register(normalized)
        for record in side.values():
            record["observation"] = renumber.apply(record["observation"])
        for case_id in ordered_ids:
            record = side.get(case_id)
            if record is None:
                continue
            normalized = copy.deepcopy(record["observation"])
            for difference in case_approved(case_id, record):
                remove_pointer(normalized, difference["path"])
            event_renumber = UuidRenumberer()
            event_renumber.register(normalized.get("events"))
            record["observation"]["events"] = event_renumber.apply(
                record["observation"].get("events")
            )
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

        if approved_overrides is not None:
            approved_left = approved_right = approved_overrides.get(case_id, [])
        else:
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
    unsettled_cases = {
        label: [
            case_id
            for case_id, record in side.items()
            if has_unsatisfied_settle(record["observation"])
        ]
        for label, side in (("canonical", canonical), ("windows", windows))
    }
    capture_error_cases = {
        label: [case_id for case_id, record in side.items() if record.get("capture_error")]
        for label, side in (("canonical", canonical), ("windows", windows))
    }
    missing_cases = [
        result["id"]
        for result in results
        if "missing_canonical" in result["mismatches"]
        or "missing_windows" in result["mismatches"]
    ]
    return {
        "cases": len(results),
        "identical": len(results) - len(deltas),
        "deltas": len(deltas),
        "capture_integrity": {
            "valid_for_promotion": not (
                any(unsettled_cases.values())
                or any(capture_error_cases.values())
                or missing_cases
            ),
            "unsettled_cases": unsettled_cases,
            "capture_error_cases": capture_error_cases,
            "missing_cases": missing_cases,
        },
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
    integrity = report.get("capture_integrity", {})
    if not integrity.get("valid_for_promotion", True):
        unsettled = integrity["unsettled_cases"]
        counts = ", ".join(f"{side}={len(cases)}" for side, cases in unsettled.items())
        lines.append(f"INVALID FOR PROMOTION: unsatisfied settle predicates ({counts})")
    if report["deltas"] == 0:
        lines.append("PASS: zero deltas")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--canonical", required=True, type=Path)
    parser.add_argument("--windows", required=True, type=Path)
    parser.add_argument("--output", type=Path, help="normalized-diff.json path")
    parser.add_argument(
        "--manifest",
        type=Path,
        help="manifest whose approved_differences override the recorded sets "
        "(compare-time source of truth when the manifest evolved between captures)",
    )
    args = parser.parse_args(argv)

    canonical = load_capture(args.canonical.read_text(encoding="utf-8"), "canonical")
    windows = load_capture(args.windows.read_text(encoding="utf-8"), "windows")
    overrides = None
    if args.manifest:
        overrides = manifest_approved_differences(
            json.loads(args.manifest.read_text(encoding="utf-8"))
        )
    report = compare_captures(canonical, windows, overrides)
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    print(render_summary(report))
    return 1 if report["deltas"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
