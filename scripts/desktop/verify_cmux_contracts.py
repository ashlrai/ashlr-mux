#!/usr/bin/env python3
"""Lightweight contract checks for the Windows desktop scaffold."""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_PATH = ROOT / "web" / "data" / "cmux.schema.json"
FIXTURE_PATH = ROOT / "contracts" / "fixtures" / "cmux.config.bootstrap.json"
REQUEST_PATH = ROOT / "contracts" / "golden" / "socket-v2" / "ping-request.jsonl"
RESPONSE_PATH = ROOT / "contracts" / "golden" / "socket-v2" / "ping-response.jsonl"


def validate_bootstrap_fixture(schema: dict[str, object], fixture: dict[str, object]) -> None:
    if fixture.get("$schema") != schema.get("$id"):
        raise ValueError("bootstrap fixture should point at the canonical cmux schema id")

    schema_version = fixture.get("schemaVersion")
    if not isinstance(schema_version, int) or schema_version < 1:
        raise ValueError("bootstrap fixture must declare schemaVersion >= 1")

    allowed_top_level = set(schema.get("properties", {}).keys()) | {"$schema"}
    unexpected = sorted(set(fixture.keys()) - allowed_top_level)
    if unexpected:
        raise ValueError(f"bootstrap fixture has unexpected keys: {unexpected}")


def validate_socket_fixture(body: str, *, needle: str, label: str) -> None:
    if not body.endswith("\n") or needle not in body:
        raise ValueError(f"{label} should be newline-delimited and contain {needle}")


def main() -> int:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    fixture = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))
    request_body = REQUEST_PATH.read_text(encoding="utf-8")
    response_body = RESPONSE_PATH.read_text(encoding="utf-8")

    try:
        validate_bootstrap_fixture(schema, fixture)
        validate_socket_fixture(
            request_body,
            needle='"method":"ping"',
            label="socket v2 request fixture",
        )
        validate_socket_fixture(
            response_body,
            needle='"pong":true',
            label="socket v2 response fixture",
        )
    except ValueError as error:
        raise SystemExit(str(error)) from error

    print("PASS: cmux desktop contracts scaffold")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
