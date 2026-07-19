#!/usr/bin/env python3
"""Lint ${...} placeholder references in a capture manifest.

Two layers:

1. Static reference lint (always runs): every placeholder must point at a
   section that exists at the time the op executes —
   - ``session.N...``: N must index an existing session_setup op;
   - ``setup.N...``: N must index a setup op of the SAME case that runs
     BEFORE the referencing op (setup ops resolve incrementally; the action
     and probes may reference any setup op);
   - ``action...``: only probes may reference the action (it has not run
     during setup/action resolution);
   - a ``result.<key>`` path on a v2 op is checked to start with ``result``.

2. Response-shape lint (needs --captures): for ``${...result.KEY...}``
   references whose target op is a v2 method, KEY is checked against the set
   of result keys that method has actually produced in the given archived
   capture(s). Methods never observed in the captures are skipped. This
   catches assumed-but-wrong nesting (e.g. ids nested one level deeper than
   the manifest author thought) before a 45-minute CI capture burns on it.

Exit 1 when any finding is reported. Note the limits honestly: a lint pass
cannot prove a setup op will SUCCEED at runtime (an op that returns an error
records ``result: null`` and downstream placeholders still fail) — see the
run-29248166966 incident where all 13 placeholder failures were downstream of
a wedged backend, not shape bugs.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from capture_driver import PLACEHOLDER_RE, parse_manifest


def _op_placeholders(value: Any):
    if isinstance(value, dict):
        for item in value.values():
            yield from _op_placeholders(item)
    elif isinstance(value, list):
        for item in value:
            yield from _op_placeholders(item)
    elif isinstance(value, str):
        for match in PLACEHOLDER_RE.finditer(value):
            yield match.group(1)


def lint_static_references(manifest: dict[str, Any]) -> list[str]:
    findings: list[str] = []
    session_len = len(manifest.get("session_setup", []))

    def check(
        where: str,
        dotted: str,
        visible_setup: int,
        phase: str,
        lane: str | None = None,
        visible_probes: int = 0,
    ) -> None:
        tokens = dotted.split(".")
        section = tokens[0]
        if section == "session":
            if len(tokens) < 2 or not tokens[1].isdigit() or int(tokens[1]) >= session_len:
                findings.append(f"{where}: '{dotted}' references a session_setup op that does not exist")
        elif section == "setup":
            if len(tokens) < 2 or not tokens[1].isdigit():
                findings.append(f"{where}: '{dotted}' has a non-numeric setup index")
            elif int(tokens[1]) >= visible_setup:
                findings.append(
                    f"{where}: '{dotted}' references setup op {tokens[1]} which has not run yet"
                    f" (only {visible_setup} setup op(s) precede this op)"
                )
        elif section == "action":
            if phase != "probe":
                findings.append(f"{where}: '{dotted}' references the action before it has run")
        elif section in ("state", "persistence", "selectors", "multiwindow"):
            if phase != "probe" or section != lane:
                findings.append(f"{where}: '{dotted}' references a probe lane that is not active")
            elif len(tokens) < 2 or not tokens[1].isdigit():
                findings.append(f"{where}: '{dotted}' has a non-numeric probe index")
            elif int(tokens[1]) >= visible_probes:
                findings.append(
                    f"{where}: '{dotted}' references probe {tokens[1]} which has not run yet"
                    f" (only {visible_probes} probe(s) precede this op in {lane})"
                )
        else:
            findings.append(f"{where}: '{dotted}' references unknown section '{section}'")

    for op_index, op in enumerate(manifest.get("session_setup", [])):
        for dotted in _op_placeholders(op):
            # session_setup ops may only reference earlier session ops.
            tokens = dotted.split(".")
            if tokens[0] != "session" or len(tokens) < 2 or not tokens[1].isdigit() or int(tokens[1]) >= op_index:
                findings.append(
                    f"session_setup[{op_index}]: '{dotted}' must reference an earlier session_setup op"
                )
    for case in manifest["cases"]:
        setup = case.get("setup", [])
        for index, op in enumerate(setup):
            for dotted in _op_placeholders(op):
                check(f"{case['id']}.setup[{index}]", dotted, index, "setup")
        for dotted in _op_placeholders(case["action"]):
            check(f"{case['id']}.action", dotted, len(setup), "action")
        for lane, lane_ops in case.get("probes", {}).items():
            for index, op in enumerate(lane_ops):
                for dotted in _op_placeholders(op):
                    check(
                        f"{case['id']}.probes[{lane}][{index}]",
                        dotted,
                        len(setup),
                        "probe",
                        lane,
                        index,
                    )
    return findings


def collect_method_result_keys(
    manifest: dict[str, Any], capture_texts: list[str]
) -> dict[str, set[str]]:
    """Union of observed v2 result keys per method, from archived captures.

    Sources: each case's action response (method taken from the manifest,
    since captures do not record the action method) and every probe record
    (which carries its ``v2:<method>`` op label).
    """
    action_methods = {
        case["id"]: case["action"]["method"]
        for case in manifest["cases"]
        if case["action"].get("op") == "v2"
    }
    shapes: dict[str, set[str]] = {}

    def absorb(method: str, response: Any) -> None:
        if not isinstance(response, dict) or response.get("ok") is not True:
            return
        result = response.get("result")
        if isinstance(result, dict):
            shapes.setdefault(method, set()).update(result.keys())

    for text in capture_texts:
        for line in text.splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            if record.get("type") != "case":
                continue
            observation = record.get("observation", {})
            method = action_methods.get(record.get("id"))
            if method:
                absorb(method, observation.get("response"))
            for lane in ("state", "persistence", "selectors", "multiwindow"):
                for probe in observation.get(lane) or []:
                    op_label = probe.get("op", "")
                    result = probe.get("result", {})
                    if op_label.startswith("v2:") and isinstance(result, dict):
                        absorb(op_label[3:], result.get("response"))
    return shapes


def lint_result_keys(
    manifest: dict[str, Any], shapes: dict[str, set[str]]
) -> list[str]:
    """Check ``${section.N.result.KEY...}`` first keys against observed shapes."""
    findings: list[str] = []
    session_ops = manifest.get("session_setup", [])

    def target_method(dotted: str, setup_ops: list[dict[str, Any]]) -> str | None:
        tokens = dotted.split(".")
        op: dict[str, Any] | None = None
        rest: list[str]
        if tokens[0] == "session" and len(tokens) > 2 and tokens[1].isdigit():
            index = int(tokens[1])
            op = session_ops[index] if index < len(session_ops) else None
            rest = tokens[2:]
        elif tokens[0] == "setup" and len(tokens) > 2 and tokens[1].isdigit():
            index = int(tokens[1])
            op = setup_ops[index] if index < len(setup_ops) else None
            rest = tokens[2:]
        else:
            return None
        if not op or op.get("op") != "v2" or len(rest) < 2 or rest[0] != "result":
            return None
        return f"{op['method']}::{rest[1]}"

    for case in manifest["cases"]:
        setup_ops = case.get("setup", [])
        everywhere = [
            (f"{case['id']}.setup", op) for op in setup_ops
        ] + [(f"{case['id']}.action", case["action"])] + [
            (f"{case['id']}.probes[{lane}]", op)
            for lane, ops in case.get("probes", {}).items()
            for op in ops
        ]
        for where, op in everywhere:
            for dotted in _op_placeholders(op):
                target = target_method(dotted, setup_ops)
                if not target:
                    continue
                method, key = target.split("::", 1)
                if method not in shapes:
                    continue  # never observed; cannot judge
                if key not in shapes[method]:
                    findings.append(
                        f"{where}: '{dotted}' expects result key '{key}' but archived"
                        f" captures show {method} result keys {sorted(shapes[method])}"
                    )
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument(
        "--captures",
        type=Path,
        nargs="*",
        default=[],
        help="archived capture NDJSON files supplying observed v2 result shapes",
    )
    args = parser.parse_args(argv)
    manifest = parse_manifest(json.loads(args.manifest.read_text(encoding="utf-8")))
    findings = lint_static_references(manifest)
    if args.captures:
        shapes = collect_method_result_keys(
            manifest, [path.read_text(encoding="utf-8") for path in args.captures]
        )
        findings += lint_result_keys(manifest, shapes)
    for finding in findings:
        print(f"LINT: {finding}")
    if not findings:
        print("manifest placeholders OK")
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
