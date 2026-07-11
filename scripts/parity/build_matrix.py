#!/usr/bin/env python3
"""Join pinned canonical catalogs with conservative Windows evidence."""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[2]
ALLOWED_STATUSES = {
    "missing",
    "red",
    "implemented_unverified",
    "verified",
    "platform_equivalent",
    "not_applicable",
}
STRICT_RESOLVED = {"verified", "platform_equivalent"}
COMPLETION_RESOLVED = STRICT_RESOLVED | {"not_applicable"}
CORE_LOCAL_DOMAINS = {
    "workspace",
    "pane",
    "surface",
    "pane_surface",
    "window",
    "session",
    "session_events",
    "terminal",
    "tmux",
    "configuration",
    "settings",
    "sidebar",
    "custom_sidebar",
    "files_projects",
    "notifications",
    "browser",
}


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def source(path: str, line: int | None = None, symbol: str | None = None, commit: str | None = None) -> dict[str, Any]:
    return {"path": path, "line": line, "symbol": symbol, "commit": commit}


def normalized_source(value: dict[str, Any], commit: str | None = None) -> dict[str, Any]:
    return source(
        value.get("path") or value.get("file"),
        value.get("line"),
        value.get("symbol"),
        commit,
    )


def collect_locations(value: Any) -> list[dict[str, Any]]:
    found: list[dict[str, Any]] = []
    if isinstance(value, dict):
        if isinstance(value.get("path"), str):
            found.append(normalized_source(value))
        else:
            for child in value.values():
                found.extend(collect_locations(child))
    elif isinstance(value, list):
        for child in value:
            found.extend(collect_locations(child))
    unique: dict[tuple[Any, ...], dict[str, Any]] = {}
    for location in found:
        key = (location["path"], location["line"], location["symbol"])
        unique[key] = location
    return sorted(unique.values(), key=lambda item: (item["path"], item["line"] or 0, item["symbol"] or ""))


def priority_for(family: str) -> int:
    family = family.lower()
    if "debug" in family:
        return 4
    if "mobile" in family:
        return 3
    if any(token in family for token in ("workspace", "pane", "surface", "window", "session", "terminal", "system")):
        return 0
    if any(token in family for token in ("tmux", "agent", "feed", "hook", "notification", "sidebar", "config", "setting", "remote", "ssh")):
        return 1
    if any(token in family for token in ("browser", "auth", "account", "cloud", "vm", "file", "project")):
        return 2
    return 2


def windows_cli_is_implemented(item: dict[str, Any]) -> bool:
    if not item.get("top_level_known"):
        return False
    if item.get("dispatch_outcome") == "explicit_socket_command_not_ported":
        return False
    if item.get("dispatch_outcome") == "control_mapping":
        return True
    if item.get("executor") or item.get("control_methods"):
        return True
    return item.get("classification") in {"local_or_no_socket", "hybrid_local_and_socket"}


def cli_entries(catalog: dict[str, Any], windows: dict[str, Any], commit: str) -> list[dict[str, Any]]:
    windows_by_name = {item["command"]: item for item in windows.get("cli_commands", [])}
    entries = []
    for item in catalog["commands"]:
        name = item["name"]
        evidence = windows_by_name.get(name)
        family = item.get("family") or "cli"
        canonical_sources = [normalized_source(item["source"], commit)]
        for contract in item.get("acceptance_contracts", []):
            location = contract.get("location") if isinstance(contract, dict) else None
            if isinstance(location, dict) and (location.get("path") or location.get("file")):
                canonical_sources.append(normalized_source(location, commit))
        windows_sources = collect_locations(evidence.get("source_locations")) if evidence else []
        status = "implemented_unverified" if evidence and windows_cli_is_implemented(evidence) else "missing"
        dependencies = [f"v2:{method}" for method in (evidence or {}).get("control_methods", [])]
        entries.append(
            {
                "id": f"cli:{name}",
                "kind": "cli_command" if item.get("visibility") == "public" else "cli_contract",
                "domain": family,
                "family": family,
                "name": name,
                "aliases": item.get("aliases", []),
                "canonical_sources": _unique_sources(canonical_sources),
                "windows_sources": windows_sources,
                "status": status,
                "acceptance_tests": [
                    f"differential.cli.{name}.process_contract",
                    f"differential.cli.{name}.state_events_persistence",
                ],
                "dependencies": dependencies,
                "priority": priority_for(family),
                "batch": family,
                "latest_verifying_commit": None,
                "rationale": None,
                "user_approval": None,
                "metadata": {
                    "visibility": item.get("visibility"),
                    "canonical_name": item.get("canonical_name"),
                    "alias_expansion": item.get("alias_expansion"),
                    "signatures": item.get("signatures", []),
                    "subcommands": item.get("subcommands", []),
                    "significant_flags": item.get("significant_flags", []),
                    "windows_evidence": evidence,
                },
            }
        )
    return entries


def v2_entries(catalog: dict[str, Any], windows: dict[str, Any], commit: str) -> list[dict[str, Any]]:
    windows_by_name = {item["method"]: item for item in windows.get("control_socket_methods", [])}
    entries = []
    for item in catalog["methods"]:
        name = item["method"]
        evidence = windows_by_name.get(name)
        family = item.get("family") or item.get("domain") or "v2"
        implemented = bool(
            evidence
            and evidence.get("advertised")
            and evidence.get("routed")
            and not evidence.get("explicit_not_supported")
        )
        canonical_locations = [item["source_location"]]
        canonical_locations.extend(item.get("dispatch_locations", []))
        canonical_locations.extend(item.get("implementation_locations", []))
        canonical_locations.extend(item.get("contract_test_pointers", []))
        entries.append(
            {
                "id": f"v2:{name}",
                "kind": "v2_method",
                "domain": item.get("domain") or family,
                "family": family,
                "name": name,
                "aliases": [],
                "canonical_sources": _unique_sources(
                    normalized_source(location, commit) for location in canonical_locations
                ),
                "windows_sources": collect_locations(evidence.get("source_locations")) if evidence else [],
                "status": "implemented_unverified" if implemented else "missing",
                "acceptance_tests": [
                    f"differential.v2.{name}.payload_error",
                    f"differential.v2.{name}.state_events_persistence",
                ],
                "dependencies": [],
                "priority": priority_for(family),
                "batch": family,
                "latest_verifying_commit": None,
                "rationale": None,
                "user_approval": None,
                "metadata": {
                    "availability": item.get("availability"),
                    "test_only": item.get("test_only", False),
                    "windows_evidence": evidence,
                },
            }
        )
    return entries


def product_entries(seed: dict[str, Any], commit: str) -> list[dict[str, Any]]:
    entries = []
    for item in seed["entries"]:
        entries.append(
            {
                "id": item["id"],
                "kind": "product_behavior",
                "domain": item["domain"],
                "family": item["family"],
                "name": item["name"],
                "aliases": [],
                "canonical_sources": [source(item["canonical_path"], commit=commit)],
                "windows_sources": [source(item["windows_path"])],
                "status": "missing",
                "acceptance_tests": [
                    f"differential.{item['id']}.live",
                    f"differential.{item['id']}.restore_multiwindow",
                ],
                "dependencies": item.get("dependencies", []),
                "priority": item["priority"],
                "batch": item["batch"],
                "latest_verifying_commit": None,
                "rationale": None,
                "user_approval": None,
                "metadata": {},
            }
        )
    return entries


def _unique_sources(values: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    unique: dict[tuple[Any, ...], dict[str, Any]] = {}
    for value in values:
        key = (value["path"], value["line"], value["symbol"], value["commit"])
        unique[key] = value
    return sorted(unique.values(), key=lambda item: (item["path"], item["line"] or 0, item["symbol"] or ""))


def apply_overrides(entries: list[dict[str, Any]], overrides: dict[str, Any]) -> None:
    by_id = {entry["id"]: entry for entry in entries}
    unknown = sorted(set(overrides.get("entries", {})) - set(by_id))
    if unknown:
        raise ValueError(f"overrides reference unknown entries: {', '.join(unknown)}")
    allowed = {
        "status",
        "acceptance_tests",
        "dependencies",
        "priority",
        "batch",
        "latest_verifying_commit",
        "rationale",
        "user_approval",
        "windows_sources",
        "metadata",
    }
    for entry_id, patch in overrides.get("entries", {}).items():
        invalid = sorted(set(patch) - allowed)
        if invalid:
            raise ValueError(f"override {entry_id} has unsupported fields: {', '.join(invalid)}")
        by_id[entry_id].update(patch)


def validate_entries(entries: list[dict[str, Any]]) -> None:
    ids = [entry["id"] for entry in entries]
    duplicates = sorted(name for name, count in Counter(ids).items() if count > 1)
    if duplicates:
        raise ValueError(f"duplicate matrix IDs: {', '.join(duplicates)}")
    for entry in entries:
        status = entry["status"]
        if status not in ALLOWED_STATUSES:
            raise ValueError(f"{entry['id']} has invalid status {status}")
        if not entry["canonical_sources"]:
            raise ValueError(f"{entry['id']} has no canonical source")
        if not entry["acceptance_tests"]:
            raise ValueError(f"{entry['id']} has no acceptance tests")
        commit = entry.get("latest_verifying_commit")
        if status in STRICT_RESOLVED:
            if not isinstance(commit, str) or len(commit) != 40:
                raise ValueError(f"{entry['id']} resolved status requires a verifying commit")
        if status == "platform_equivalent" and not entry.get("rationale"):
            raise ValueError(f"{entry['id']} platform_equivalent requires rationale")
        if status == "not_applicable":
            if not entry.get("rationale") or not entry.get("user_approval"):
                raise ValueError(f"{entry['id']} not_applicable requires rationale and user approval")


def ratio(numerator: int, denominator: int) -> dict[str, Any]:
    percentage = round((100 * numerator / denominator), 4) if denominator else 100.0
    return {"numerator": numerator, "denominator": denominator, "percentage": percentage}


def summary(entries: list[dict[str, Any]], cli_catalog: dict[str, Any], v2_catalog: dict[str, Any]) -> dict[str, Any]:
    by_status = Counter(entry["status"] for entry in entries)
    by_kind = Counter(entry["kind"] for entry in entries)
    domain_status: dict[str, Counter[str]] = defaultdict(Counter)
    for entry in entries:
        domain_status[entry["domain"]][entry["status"]] += 1
    strict = sum(entry["status"] in STRICT_RESOLVED for entry in entries)
    completion = sum(entry["status"] in COMPLETION_RESOLVED for entry in entries)
    core = [entry for entry in entries if entry["domain"] in CORE_LOCAL_DOMAINS]
    core_strict = sum(entry["status"] in STRICT_RESOLVED for entry in core)
    return {
        "total_entries": len(entries),
        "by_status": dict(sorted(by_status.items())),
        "by_kind": dict(sorted(by_kind.items())),
        "by_domain": {domain: dict(sorted(counts.items())) for domain, counts in sorted(domain_status.items())},
        "unresolved_entries": sum(entry["status"] not in COMPLETION_RESOLVED for entry in entries),
        "strict_parity": ratio(strict, len(entries)),
        "completion_resolution": ratio(completion, len(entries)),
        "core_local_workflow": ratio(core_strict, len(core)),
        "canonical_targets": {
            "public_top_level_commands": cli_catalog["counts"]["public_top_level_commands"],
            "hidden_or_internal_top_level_commands": cli_catalog["counts"]["hidden_or_internal_top_level_commands"],
            "release_v2_methods": v2_catalog["counts"]["release"],
            "debug_only_v2_methods": v2_catalog["counts"]["debug_only"],
        },
    }


def build(args: argparse.Namespace) -> dict[str, Any]:
    baseline = load_json(args.baseline)
    cli = load_json(args.cli)
    v2 = load_json(args.v2)
    windows = load_json(args.windows)
    products = load_json(args.products)
    overrides = load_json(args.overrides)
    canonical_commit = baseline["canonical"]["commit"]
    if cli.get("canonical_revision") != canonical_commit:
        raise ValueError("canonical CLI catalog does not match frozen baseline")
    if v2.get("canonical_commit") != canonical_commit:
        raise ValueError("canonical v2 catalog does not match frozen baseline")
    expected_cli = baseline["canonical"]["expected_top_level_commands"]
    expected_v2 = baseline["canonical"]["expected_release_v2_methods"]
    if cli["counts"]["public_top_level_commands"] != expected_cli:
        raise ValueError("canonical public CLI count does not match frozen expectation")
    if v2["counts"]["release"] != expected_v2:
        raise ValueError("canonical release-v2 count does not match frozen expectation")
    entries = cli_entries(cli, windows, canonical_commit)
    entries.extend(v2_entries(v2, windows, canonical_commit))
    entries.extend(product_entries(products, canonical_commit))
    entries.sort(key=lambda entry: entry["id"])
    apply_overrides(entries, overrides)
    validate_entries(entries)
    generated_from = windows.get("generated_from", {})
    windows_commit = (
        generated_from.get("repository_commit")
        or generated_from.get("commit")
        or baseline["windows"]["head_when_frozen"]
    )
    return {
        "schema_version": 1,
        "baseline": {
            "canonical_commit": canonical_commit,
            "windows_commit": windows_commit,
        },
        "entries": entries,
        "summary": summary(entries, cli, v2),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=Path, default=ROOT / "docs/parity/baseline.json")
    parser.add_argument("--cli", type=Path, default=ROOT / "docs/parity/source/canonical_cli.json")
    parser.add_argument("--v2", type=Path, default=ROOT / "docs/parity/source/canonical_v2.json")
    parser.add_argument("--windows", type=Path, default=ROOT / "docs/parity/source/windows_evidence.json")
    parser.add_argument("--products", type=Path, default=ROOT / "docs/parity/product_domains.json")
    parser.add_argument("--overrides", type=Path, default=ROOT / "docs/parity/overrides.json")
    parser.add_argument("--output", type=Path, default=ROOT / "docs/parity/parity-matrix.json")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rendered = json.dumps(build(args), indent=2, sort_keys=True) + "\n"
    if args.check:
        if not args.output.exists() or args.output.read_text(encoding="utf-8") != rendered:
            print(f"matrix drift: regenerate {args.output}", file=sys.stderr)
            return 1
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
