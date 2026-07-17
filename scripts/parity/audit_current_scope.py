#!/usr/bin/env python3
"""Generate a compact, current parity-scope audit from reproducible catalogs."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = ROOT / "docs/parity/current-audit.json"


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", *args],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
    ).strip()


def run(*args: str) -> None:
    subprocess.run(args, cwd=ROOT, check=True)


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def commit_metadata(commit: str) -> dict[str, str]:
    resolved = git("rev-parse", f"{commit}^{{commit}}")
    date, subject = git("show", "-s", "--format=%cI%n%s", resolved).split("\n", 1)
    return {"commit": resolved, "commit_date": date, "commit_subject": subject}


def commits_ahead(base: str, head: str) -> int | None:
    ancestor = subprocess.run(
        ["git", "merge-base", "--is-ancestor", base, head],
        cwd=ROOT,
        check=False,
    )
    if ancestor.returncode != 0:
        return None
    return int(git("rev-list", "--count", f"{base}..{head}"))


def catalog_ids(cli: dict, v2: dict) -> set[str]:
    ids = {f"cli:{entry['name']}" for entry in cli["commands"]}
    ids.update(f"v2:{entry['method']}" for entry in v2["methods"])
    return ids


def build_audit(canonical_commit: str, windows_commit: str) -> dict:
    canonical = commit_metadata(canonical_commit)
    windows = commit_metadata(windows_commit)
    tracked_baseline = load(ROOT / "docs/parity/baseline.json")
    tracked_cli = load(ROOT / "docs/parity/source/canonical_cli.json")
    tracked_v2 = load(ROOT / "docs/parity/source/canonical_v2.json")
    tracked_windows = load(ROOT / "docs/parity/source/windows_evidence.json")

    with tempfile.TemporaryDirectory(prefix="cmux-parity-audit-") as directory:
        temporary = Path(directory)
        cli_path = temporary / "canonical-cli.json"
        v2_path = temporary / "canonical-v2.json"
        windows_path = temporary / "windows-evidence.json"
        baseline_path = temporary / "baseline.json"
        matrix_path = temporary / "matrix.json"

        run(
            sys.executable,
            "scripts/extract_canonical_cli.py",
            "--revision",
            canonical["commit"],
            "--output",
            str(cli_path),
        )
        run(
            sys.executable,
            "scripts/parity/extract_canonical_v2.py",
            "--commit",
            canonical["commit"],
            "--allow-count-drift",
            "--output",
            str(v2_path),
        )
        run(
            sys.executable,
            "scripts/parity/extract_windows_evidence.py",
            "--commit",
            windows["commit"],
            "--output",
            str(windows_path),
        )

        cli = load(cli_path)
        v2 = load(v2_path)
        windows_evidence = load(windows_path)
        temporary_baseline = json.loads(json.dumps(tracked_baseline))
        temporary_baseline["canonical"].update(
            {
                "commit": canonical["commit"],
                "commit_date": canonical["commit_date"].split("T", 1)[0],
                "commit_subject": canonical["commit_subject"],
                "expected_top_level_commands": cli["counts"]["public_top_level_commands"],
                "expected_release_v2_methods": v2["counts"]["release"],
            }
        )
        temporary_baseline["windows"]["head_when_frozen"] = windows["commit"]
        baseline_path.write_text(
            json.dumps(temporary_baseline, indent=2) + "\n", encoding="utf-8"
        )
        run(
            sys.executable,
            "scripts/parity/build_matrix.py",
            "--baseline",
            str(baseline_path),
            "--cli",
            str(cli_path),
            "--v2",
            str(v2_path),
            "--windows",
            str(windows_path),
            "--output",
            str(matrix_path),
        )
        matrix = load(matrix_path)

    tracked_ids = catalog_ids(tracked_cli, tracked_v2)
    current_ids = catalog_ids(cli, v2)
    summary = matrix["summary"]
    tracked_windows_commit = tracked_windows["generated_from"]["repository_commit"]
    return {
        "schema_version": 1,
        "canonical": {
            **canonical,
            "tracked_acceptance_baseline": tracked_baseline["canonical"]["commit"],
            "commits_ahead_of_tracked_baseline": commits_ahead(
                tracked_baseline["canonical"]["commit"], canonical["commit"]
            ),
            "counts": {
                "public_cli_commands": cli["counts"]["public_top_level_commands"],
                "hidden_or_internal_cli_commands": cli["counts"][
                    "hidden_or_internal_top_level_commands"
                ],
                "release_v2_methods": v2["counts"]["release"],
                "debug_only_v2_methods": v2["counts"]["debug_only"],
            },
            "catalog_rows_added_since_baseline": sorted(current_ids - tracked_ids),
            "catalog_rows_removed_since_baseline": sorted(tracked_ids - current_ids),
        },
        "windows": {
            **windows,
            "tracked_evidence_commit": tracked_windows_commit,
            "commits_ahead_of_tracked_evidence": commits_ahead(
                tracked_windows_commit, windows["commit"]
            ),
            "source_evidence": windows_evidence["summary"],
        },
        "matrix_rows": {
            "total": summary["total_entries"],
            "by_kind": summary["by_kind"],
            "by_status": summary["by_status"],
            "strict_verified": summary["strict_parity"]["numerator"],
            "warning": (
                "Rows mix CLI commands, socket methods, debug endpoints, and broad product "
                "umbrellas. They are neither deduplicated user capabilities nor effort-weighted."
            ),
        },
        "user_capability_completion": {
            "status": "not_measured",
            "reason": (
                "The current catalog does not yet deduplicate entry points into observable "
                "user capabilities with acceptance evidence."
            ),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--canonical-commit", default="origin/main")
    parser.add_argument("--windows-commit", default="HEAD")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rendered = json.dumps(
        build_audit(args.canonical_commit, args.windows_commit),
        indent=2,
        sort_keys=True,
    ) + "\n"
    if args.check:
        if not args.output.exists() or args.output.read_text(encoding="utf-8") != rendered:
            print(f"current parity audit is stale: {args.output}", file=sys.stderr)
            return 1
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8", newline="\n")
        print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
