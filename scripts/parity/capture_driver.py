#!/usr/bin/env python3
"""Manifest-driven live capture driver for the frozen-canonical differential gate.

Executes one manifest of parity cases against a live cmux backend (the frozen
canonical macOS app over a unix control socket, or the Windows Tauri backend
over a named pipe) and emits one NDJSON observation per case covering all ten
observation lanes required by scripts/parity/differential_harness.py:

    exit_status stdout stderr error response state events persistence
    selectors multiwindow

Lanes that a case does not probe are recorded as explicit ``null`` — never
omitted — so the harness's lane-presence validation holds.

Wire formats (mirrors crates/cmux-ipc/src/client.rs):
  v1: ``<command line>\n`` with shell-quoted tokens; the reply is opaque text,
      one trailing newline stripped, ``ERROR:`` prefix means failure.
  v2: ``{"id":1,"method":...,"params":...}\n``; the reply is one JSON object
      line: ``{"ok":true,"result":...}`` or ``{"ok":false,"error":{...}}``.
  auth: an optional ``auth <password>`` line per connection; the server replies
      ``OK: Authenticated`` (any non-``ERROR:`` reply is accepted).
  events: a dedicated connection sends ``{"method":"events.stream","params":..}``
      and then reads NDJSON event frames until closed
      (Sources/CmuxEventStream.swift at the pinned canonical commit).

Transport is selected from the ``--socket`` value: a ``\\\\.\\pipe\\`` prefix is a
Windows named pipe, anything else is a unix domain socket path.

UUIDs are symbolized by first-observation (creation) order into ``<uuid-N>``
tokens before the observation is written, so canonical and Windows captures
compare structurally. Refs (``workspace:N`` etc.) are already stable. The exact
``--socket`` value is rewritten to ``<socket>`` wherever it appears.
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import re
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Callable

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

PROBE_LANES = ("state", "events", "persistence", "selectors", "multiwindow")

OP_KINDS = ("v2", "v1", "cli", "restart", "sleep")

UUID_RE = re.compile(
    r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
)

PLACEHOLDER_RE = re.compile(r"\$\{([A-Za-z0-9_.]+)\}")

WINDOWS_PIPE_PREFIX = "\\\\.\\pipe\\"


# ---------------------------------------------------------------------------
# Pure: manifest parsing / validation
# ---------------------------------------------------------------------------


class ManifestError(ValueError):
    """The manifest is structurally invalid."""


#: Settle config keys (root-sanctioned bounded settle semantics for reads that
#: race asynchronous UI teardown. Canonical retains closed windows as invisible,
#: recoverable routes, so lifecycle cleanup waits for non-visibility, not absence.
SETTLE_KEYS = (
    "until_absent",
    "until_present",
    "until_window_not_visible",
    "stable",
    "timeout_s",
    "poll_interval_s",
)
SETTLE_DEFAULT_TIMEOUT_S = 10.0
SETTLE_DEFAULT_POLL_INTERVAL_S = 0.5


def _validate_settle(settle: Any, where: str) -> None:
    if not isinstance(settle, dict):
        raise ManifestError(f"{where}: 'settle' must be an object")
    unknown = [key for key in settle if key not in SETTLE_KEYS]
    if unknown:
        raise ManifestError(f"{where}: unknown settle keys {unknown} (expected {SETTLE_KEYS})")
    for key in ("until_absent", "until_present", "until_window_not_visible"):
        if key in settle and (
            not isinstance(settle[key], list)
            or not settle[key]
            or not all(isinstance(item, str) and item for item in settle[key])
        ):
            raise ManifestError(f"{where}: settle '{key}' must be a non-empty list of strings")
    if "stable" in settle and settle["stable"] is not True:
        raise ManifestError(f"{where}: settle 'stable' must be true when present")
    predicate_keys = ("until_absent", "until_present", "until_window_not_visible", "stable")
    if not any(key in settle for key in predicate_keys):
        raise ManifestError(
            f"{where}: settle requires at least one predicate "
            "(until_absent/until_present/until_window_not_visible/stable)"
        )
    for key in ("timeout_s", "poll_interval_s"):
        if key in settle and (not isinstance(settle[key], (int, float)) or settle[key] <= 0):
            raise ManifestError(f"{where}: settle '{key}' must be a positive number")


def _validate_op(op: Any, where: str) -> dict[str, Any]:
    if not isinstance(op, dict):
        raise ManifestError(f"{where}: op must be an object, got {type(op).__name__}")
    kind = op.get("op")
    if kind not in OP_KINDS:
        raise ManifestError(f"{where}: unknown op kind {kind!r} (expected one of {OP_KINDS})")
    if kind == "v2":
        if not isinstance(op.get("method"), str) or not op["method"]:
            raise ManifestError(f"{where}: v2 op requires a non-empty string 'method'")
        params = op.get("params", {})
        if not isinstance(params, dict):
            raise ManifestError(f"{where}: v2 op 'params' must be an object")
        if "settle" in op:
            _validate_settle(op["settle"], where)
    elif "settle" in op:
        raise ManifestError(f"{where}: 'settle' is only supported on v2 ops")
    elif kind == "v1":
        if not isinstance(op.get("command"), str) or not op["command"]:
            raise ManifestError(f"{where}: v1 op requires a non-empty string 'command'")
        args = op.get("args", [])
        if not isinstance(args, list) or not all(isinstance(a, str) for a in args):
            raise ManifestError(f"{where}: v1 op 'args' must be a list of strings")
    elif kind == "cli":
        argv = op.get("argv")
        if not isinstance(argv, list) or not argv or not all(isinstance(a, str) for a in argv):
            raise ManifestError(f"{where}: cli op requires a non-empty string list 'argv'")
        env = op.get("env", {})
        if not isinstance(env, dict) or not all(
            isinstance(k, str) and isinstance(v, str) for k, v in env.items()
        ):
            raise ManifestError(f"{where}: cli op 'env' must be a string-to-string object")
    elif kind == "sleep":
        seconds = op.get("seconds")
        if not isinstance(seconds, (int, float)) or seconds <= 0:
            raise ManifestError(f"{where}: sleep op requires positive numeric 'seconds'")
    return op


def parse_manifest(payload: Any) -> dict[str, Any]:
    """Validate a decoded manifest document and return it.

    Shape:
      {
        "family": str,
        "session_setup": [op, ...],           # optional, run once, results
                                              # addressable as ${session.N...}
        "cases": [
          {
            "id": str (unique),
            "events": bool,                   # optional: collect event frames
            "events_params": {...},           # optional events.stream params
            "may_disconnect": bool,           # optional: action may end the app;
                                              # only valid on the final case
            "setup": [op, ...],               # optional
            "action": op,                     # required
            "probes": {lane: [op, ...]},      # optional, lanes from PROBE_LANES
            "approved_differences": [{"path": str, "rationale": str}, ...],
          }, ...
        ]
      }
    """
    if not isinstance(payload, dict):
        raise ManifestError("manifest must be a JSON object")
    if not isinstance(payload.get("family"), str) or not payload["family"]:
        raise ManifestError("manifest requires a non-empty string 'family'")
    for index, op in enumerate(payload.get("session_setup", [])):
        _validate_op(op, f"session_setup[{index}]")
    cases = payload.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ManifestError("manifest requires a non-empty 'cases' array")
    seen_ids: set[str] = set()
    for position, case in enumerate(cases):
        where = f"cases[{position}]"
        if not isinstance(case, dict):
            raise ManifestError(f"{where}: case must be an object")
        case_id = case.get("id")
        if not isinstance(case_id, str) or not case_id:
            raise ManifestError(f"{where}: case requires a non-empty string 'id'")
        if case_id in seen_ids:
            raise ManifestError(f"{where}: duplicate case id {case_id!r}")
        seen_ids.add(case_id)
        may_disconnect = case.get("may_disconnect", False)
        if not isinstance(may_disconnect, bool):
            raise ManifestError(f"{where}: 'may_disconnect' must be a boolean")
        if may_disconnect and position != len(cases) - 1:
            raise ManifestError(
                f"{where}: a case that may disconnect must be the final case"
            )
        for index, op in enumerate(case.get("setup", [])):
            _validate_op(op, f"{where}.setup[{index}]")
        if "action" not in case:
            raise ManifestError(f"{where}: case requires an 'action' op")
        _validate_op(case["action"], f"{where}.action")
        probes = case.get("probes", {})
        if not isinstance(probes, dict):
            raise ManifestError(f"{where}: 'probes' must be an object")
        if may_disconnect and probes:
            raise ManifestError(
                f"{where}: a case that may disconnect cannot define post-action probes"
            )
        for lane, ops in probes.items():
            if lane not in PROBE_LANES:
                raise ManifestError(
                    f"{where}: probe lane {lane!r} is not one of {PROBE_LANES}"
                )
            if lane == "events":
                raise ManifestError(
                    f"{where}: the events lane is filled by 'events': true, not probe ops"
                )
            if not isinstance(ops, list):
                raise ManifestError(f"{where}: probes[{lane!r}] must be an array of ops")
            for index, op in enumerate(ops):
                _validate_op(op, f"{where}.probes[{lane!r}][{index}]")
        for index, difference in enumerate(case.get("approved_differences", [])):
            if (
                not isinstance(difference, dict)
                or not isinstance(difference.get("path"), str)
                or not difference["path"].startswith("/")
                or not isinstance(difference.get("rationale"), str)
                or not difference["rationale"].strip()
            ):
                raise ManifestError(
                    f"{where}.approved_differences[{index}]: requires JSON-pointer 'path' and"
                    " non-empty 'rationale'"
                )
    return payload


def manifest_needs_restart(manifest: dict[str, Any]) -> bool:
    """Whether any op anywhere in the manifest is a restart op."""

    def ops(case: dict[str, Any]):
        yield from case.get("setup", [])
        yield case["action"]
        for lane_ops in case.get("probes", {}).values():
            yield from lane_ops

    if any(op.get("op") == "restart" for op in manifest.get("session_setup", [])):
        return True
    return any(
        op.get("op") == "restart" for case in manifest["cases"] for op in ops(case)
    )


# ---------------------------------------------------------------------------
# Pure: wire encoding (mirrors crates/cmux-ipc/src/client.rs)
# ---------------------------------------------------------------------------

_V1_SAFE_RE = re.compile(r"^[A-Za-z0-9_@%+=:,./-]+$")


def shell_quote(value: str) -> str:
    """Quote one v1 token the way the macOS CLI's shellQuote does."""
    if value and _V1_SAFE_RE.match(value):
        return value
    return "'" + value.replace("'", "'\\''") + "'"


def build_v1_command_line(command: str, args: list[str]) -> str:
    """Shell-quote and join a v1 command line (without the trailing newline)."""
    return " ".join(shell_quote(token) for token in [command, *args])


def build_v2_request(method: str, params: dict[str, Any]) -> str:
    """Build the one-line v2 request envelope with the fixed client id 1."""
    return json.dumps(
        {"id": 1, "method": method, "params": params}, separators=(",", ":")
    )


def interpret_v2_response(raw: str) -> dict[str, Any]:
    """Decode a v2 reply into {"ok": bool, "response": Any, "error": Any}.

    ``response`` is the full parsed reply object (id/ok/result/... verbatim) so
    the differential compares the entire canonical envelope, not a projection.
    """
    if raw.startswith("ERROR:"):
        return {"ok": False, "response": None, "error": {"plain_text": raw}}
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return {"ok": False, "response": None, "error": {"invalid_v2_response": raw}}
    if not isinstance(parsed, dict):
        return {"ok": False, "response": None, "error": {"invalid_v2_response": raw}}
    if parsed.get("ok") is True:
        return {"ok": True, "response": parsed, "error": None}
    return {"ok": False, "response": parsed, "error": parsed.get("error")}


def interpret_v1_response(raw: str) -> dict[str, Any]:
    """Decode a v1 reply: ERROR:-prefixed is a failure surfaced verbatim."""
    if raw.startswith("ERROR:"):
        return {"ok": False, "response": raw, "error": raw}
    return {"ok": True, "response": raw, "error": None}


# ---------------------------------------------------------------------------
# Pure: placeholder resolution
# ---------------------------------------------------------------------------


class PlaceholderError(ValueError):
    """A ${...} reference could not be resolved."""


def _lookup_path(context: dict[str, Any], dotted: str) -> Any:
    current: Any = context
    for token in dotted.split("."):
        if isinstance(current, list):
            try:
                current = current[int(token)]
            except (ValueError, IndexError) as error:
                raise PlaceholderError(f"cannot resolve '{dotted}' at token '{token}'") from error
        elif isinstance(current, dict):
            if token not in current:
                raise PlaceholderError(f"cannot resolve '{dotted}' at token '{token}'")
            current = current[token]
        else:
            raise PlaceholderError(f"cannot resolve '{dotted}' at token '{token}'")
    return current


def resolve_placeholders(value: Any, context: dict[str, Any]) -> Any:
    """Substitute ``${section.path}`` references against recorded op results.

    ``context`` maps section names (``session``, ``setup``, ``action``) to
    recorded results. A string that is exactly one placeholder resolves to the
    referenced value with its type preserved; embedded placeholders stringify.
    """
    if isinstance(value, dict):
        return {key: resolve_placeholders(item, context) for key, item in value.items()}
    if isinstance(value, list):
        return [resolve_placeholders(item, context) for item in value]
    if isinstance(value, str):
        exact = PLACEHOLDER_RE.fullmatch(value)
        if exact:
            return _lookup_path(context, exact.group(1))
        return PLACEHOLDER_RE.sub(
            lambda match: str(_lookup_path(context, match.group(1))), value
        )
    return value


# ---------------------------------------------------------------------------
# Pure: symbolization + observation shaping
# ---------------------------------------------------------------------------


class Symbolizer:
    """Rewrites UUIDs to ``<uuid-N>`` by first-seen (creation) order.

    ``register`` walks any recorded value in execution order so ids created
    during setup are allocated before probe output mentions them, keeping the
    numbering aligned with creation order on both platforms. The exact socket
    address is rewritten to ``<socket>``.
    """

    def __init__(self, socket_address: str | None = None) -> None:
        self._table: dict[str, str] = {}
        self._socket_address = socket_address

    def register(self, value: Any) -> None:
        for uuid in self._find_uuids(value):
            self._table.setdefault(uuid, f"<uuid-{len(self._table) + 1}>")

    def apply(self, value: Any) -> Any:
        if isinstance(value, dict):
            return {self._apply_str(k): self.apply(v) for k, v in value.items()}
        if isinstance(value, list):
            return [self.apply(item) for item in value]
        if isinstance(value, str):
            return self._apply_str(value)
        return value

    def _apply_str(self, value: str) -> str:
        if self._socket_address:
            value = value.replace(self._socket_address, "<socket>")

        def replace(match: re.Match[str]) -> str:
            uuid = match.group(0).lower()
            if uuid not in self._table:
                self._table[uuid] = f"<uuid-{len(self._table) + 1}>"
            return self._table[uuid]

        return UUID_RE.sub(replace, value)

    def _find_uuids(self, value: Any):
        if isinstance(value, dict):
            for key, item in value.items():
                yield from self._find_uuids(key)
                yield from self._find_uuids(item)
        elif isinstance(value, list):
            for item in value:
                yield from self._find_uuids(item)
        elif isinstance(value, str):
            for match in UUID_RE.finditer(value):
                yield match.group(0).lower()


class TimingSymbolizer:
    """Sanctioned normalization for event-frame timing nondeterminism ONLY.

    Root decision (2026-07-13): `occurred_at` timestamps and seq-derived event
    ids (`<boot uuid>-<seq>`) are nondeterministic across any two runs even at
    perfect behavior parity, so they are symbolized by stable first-seen order
    (`<ts-N>` / `<event-id-N>`), exactly like UUID symbolization. `boot_id` is
    already covered by the UUID pass. Everything else about the events lane
    stays STRICT: frame counts, event names, frame order, payload keys, and
    the ack's replay/`after_seq`/`latest_seq`/`resume` counters — those are
    real backend divergences and are never normalized here.

    Root ruling addendum (2026-07-13): the ack's ABSOLUTE sequence counters
    (``resume.latest_seq``/``next_seq``/``requested_after_seq``) and each
    frame's ``seq`` are sanctioned for offset-from-subscription symbolization
    (``<seq+N>`` relative to the ack's ``latest_seq``): they encode
    boot-history cardinality (canonical emits more app-launch events than the
    port's bootstrap), not contract behavior. Relative ordering stays fully
    strict — a skipped or reordered seq still deltas. ``after_seq``,
    ``oldest_seq``, ``gap``, and ``replay_count`` stay RAW: they pin the
    replay-default contract and eviction anchor. Boot-emission CONTENT parity
    is deliberately out of this family's scope (future boot/persistence
    family case).

    Application is idempotent (already-symbolized values pass through), so the
    comparator can re-apply it at load time to archived captures produced
    before this normalization existed; the frozen canonical NDJSON is never
    rewritten.
    """

    _EVENT_SEQ_ID_RE = re.compile(
        r"(?:<uuid-\d+>|[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}"
        r"-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})-\d+$"
    )
    _SUBSCRIPTION_ID_RE = re.compile(
        r"(?:<uuid-\d+>|[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}"
        r"-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})"
    )
    _EVENT_ID_SYMBOL_RE = re.compile(r"<event-id-\d+>$")

    def __init__(self) -> None:
        self._ts_counter = 0
        self._id_table: dict[str, str] = {}
        self._seq_base: int | None = None

    #: Absolute counters rebased to the subscription point (ack latest_seq).
    _REBASED_RESUME_KEYS = ("latest_seq", "next_seq", "requested_after_seq")

    @staticmethod
    def _subscription_base(events: Any) -> int | None:
        """The ack frame's latest_seq — the subscription point this lane's
        absolute counters are rebased against. None when no numeric ack is
        found (already-symbolized or ack-less lanes stay untouched)."""
        if not isinstance(events, list):
            return None
        for frame in events:
            if isinstance(frame, dict) and frame.get("protocol") == "cmux-events":
                latest = (frame.get("resume") or {}).get("latest_seq")
                if isinstance(latest, int):
                    return latest
        return None

    def apply(self, events: Any) -> Any:
        """Symbolize the events lane; None (lane not captured) passes through."""
        if events is None:
            return None
        self._ts_counter = 0
        self._id_table.clear()
        self._seq_base = self._subscription_base(events)
        return self._walk(events)

    def _walk(self, value: Any) -> Any:
        if isinstance(value, list):
            return [self._walk(item) for item in value]
        if isinstance(value, dict):
            return {key: self._map(key, item) for key, item in value.items()}
        return value

    def _map(self, key: str, value: Any) -> Any:
        if (
            key == "subscription_id"
            and isinstance(value, str)
            and self._SUBSCRIPTION_ID_RE.fullmatch(value)
        ):
            return "<subscription-id>"
        if key == "occurred_at" and isinstance(value, str):
            # Per-OCCURRENCE, not per-value: whether two adjacent events share
            # the same wall-clock millisecond is itself nondeterministic (a
            # canonical run had surface.created/surface.selected coincide while
            # the Windows run did not), so coincidental equality relations are
            # deliberately not preserved — timestamps carry no contract meaning
            # per the root ruling.
            self._ts_counter += 1
            return f"<ts-{self._ts_counter}>"
        if key == "id" and isinstance(value, str):
            if self._EVENT_ID_SYMBOL_RE.fullmatch(value):
                if value not in self._id_table:
                    self._id_table[value] = f"<event-id-{len(self._id_table) + 1}>"
                return self._id_table[value]
            if self._EVENT_SEQ_ID_RE.fullmatch(value):
                if value not in self._id_table:
                    self._id_table[value] = f"<event-id-{len(self._id_table) + 1}>"
                return self._id_table[value]
            return value
        if (
            key in self._REBASED_RESUME_KEYS or key == "seq"
        ) and isinstance(value, int) and self._seq_base is not None:
            return f"<seq{value - self._seq_base:+d}>"
        return self._walk(value)


class UuidRefCanonicalizer:
    """Names entity UUID symbols by their stable public ``kind:N`` refs.

    Event families can legitimately encounter the same entities in different
    orders before their strict event streams converge. Creation-order UUID
    numbering would then cascade a false mismatch into later responses. A
    co-located ``id``/``ref`` or ``*_id``/``*_ref`` pair is authoritative
    identity evidence, so this pass rewrites that UUID everywhere to a stable
    ``<ref:kind:N>`` token. Conflicting evidence stays unmodified and strict.
    """

    _TOKEN_RE = re.compile(r"<uuid-\d+>")
    _REF_RE = re.compile(r"(?:window|workspace|workspace_group|pane|surface|terminal|tab):\d+")

    def __init__(self) -> None:
        self._table: dict[str, str] = {}
        self._ambiguous: set[str] = set()

    def register(self, value: Any) -> None:
        if isinstance(value, dict):
            for key, item in value.items():
                if key == "id":
                    ref_key = "ref"
                elif key.endswith("_id"):
                    ref_key = f"{key[:-3]}_ref"
                else:
                    ref_key = None
                if ref_key is not None:
                    self._register_pair(item, value.get(ref_key))
                self.register(item)
        elif isinstance(value, list):
            for item in value:
                self.register(item)

    def _register_pair(self, entity_id: Any, entity_ref: Any) -> None:
        if not isinstance(entity_id, str) or not self._TOKEN_RE.fullmatch(entity_id):
            return
        if not isinstance(entity_ref, str) or not self._REF_RE.fullmatch(entity_ref):
            return
        replacement = f"<ref:{entity_ref}>"
        existing = self._table.get(entity_id)
        if existing is not None and existing != replacement:
            self._table.pop(entity_id, None)
            self._ambiguous.add(entity_id)
        elif entity_id not in self._ambiguous:
            self._table[entity_id] = replacement

    def apply(self, value: Any) -> Any:
        if isinstance(value, dict):
            return {self.apply(key): self.apply(item) for key, item in value.items()}
        if isinstance(value, list):
            return [self.apply(item) for item in value]
        if isinstance(value, str):
            return self._TOKEN_RE.sub(
                lambda match: self._table.get(match.group(0), match.group(0)), value
            )
        return value


class UuidRenumberer:
    """Deterministic compare-time renumbering of ``<uuid-N>`` symbols.

    Capture-time UUID symbols are allocated in wire order, but JSON object key
    order is explicitly non-contractual, so two behaviorally identical captures
    can allocate the same entity different symbol numbers (and cascade that
    offset over the whole session). This renumbers each capture's symbols by
    first-seen order under a deterministic traversal — record order, dict keys
    sorted, list order preserved — making symbol numbers independent of wire
    key order. Values are only renamed token-for-token; nothing is hidden.
    Idempotent: renumbering a renumbered capture is a no-op.
    """

    _TOKEN_RE = re.compile(r"<uuid-\d+>")

    def __init__(self) -> None:
        self._table: dict[str, str] = {}

    def register(self, value: Any) -> None:
        if isinstance(value, dict):
            for key in sorted(value):
                self.register(key)
                self.register(value[key])
        elif isinstance(value, list):
            for item in value:
                self.register(item)
        elif isinstance(value, str):
            for match in self._TOKEN_RE.finditer(value):
                token = match.group(0)
                if token not in self._table:
                    self._table[token] = f"<uuid-{len(self._table) + 1}>"

    def apply(self, value: Any) -> Any:
        if isinstance(value, dict):
            return {self.apply(k): self.apply(v) for k, v in value.items()}
        if isinstance(value, list):
            return [self.apply(item) for item in value]
        if isinstance(value, str):
            return self._TOKEN_RE.sub(
                lambda match: self._table.get(match.group(0), match.group(0)), value
            )
        return value


def shape_observation(
    action_result: dict[str, Any],
    probe_results: dict[str, list[dict[str, Any]]],
    events: list[Any] | None,
) -> dict[str, Any]:
    """Assemble the ten-lane observation; unprobed lanes are explicit None."""
    observation: dict[str, Any] = {key: None for key in OBSERVATION_KEYS}
    kind = action_result.get("kind")
    if kind == "cli":
        observation["exit_status"] = action_result.get("exit_status")
        observation["stdout"] = action_result.get("stdout")
        observation["stderr"] = action_result.get("stderr")
    else:
        observation["response"] = action_result.get("response")
        observation["error"] = action_result.get("error")
    for lane in ("state", "persistence", "selectors", "multiwindow"):
        if lane in probe_results:
            observation[lane] = probe_results[lane]
    observation["events"] = events
    missing = [key for key in OBSERVATION_KEYS if key not in observation]
    if missing:  # pragma: no cover - defensive, construction covers all keys
        raise AssertionError(f"observation missing lanes: {missing}")
    return observation


def describe_op(op: dict[str, Any]) -> str:
    kind = op["op"]
    if kind == "v2":
        return f"v2:{op['method']}"
    if kind == "v1":
        return f"v1:{op['command']}"
    if kind == "cli":
        return "cli:" + " ".join(op["argv"])
    return kind


# ---------------------------------------------------------------------------
# Transports (side-effecting)
# ---------------------------------------------------------------------------


def is_windows_pipe_address(address: str) -> bool:
    return address.startswith(WINDOWS_PIPE_PREFIX)


class TransportError(RuntimeError):
    pass


def _read_with_timeout(read_chunk: Callable[[], bytes], deadline: float, until_eof: bool) -> bytes:
    """Read chunks on a worker thread until newline (or EOF) or the deadline."""
    frames: queue.Queue[bytes | None] = queue.Queue()

    def pump() -> None:
        try:
            while True:
                chunk = read_chunk()
                frames.put(chunk)
                if not chunk:
                    return
        except OSError:
            frames.put(b"")

    worker = threading.Thread(target=pump, daemon=True)
    worker.start()
    buffer = b""
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return buffer
        try:
            chunk = frames.get(timeout=remaining)
        except queue.Empty:
            return buffer
        if not chunk:
            return buffer
        buffer += chunk
        if not until_eof and b"\n" in buffer:
            return buffer


def _cancel_pending_io(fd: int) -> None:
    """Cancel outstanding I/O on a Windows fd so close() cannot block behind a
    reader thread parked in ReadFile. No-op off Windows / on failure."""
    if sys.platform != "win32":  # pragma: no cover - Windows-only concern
        return
    try:
        import ctypes
        import msvcrt

        handle = msvcrt.get_osfhandle(fd)
        ctypes.windll.kernel32.CancelIoEx(ctypes.c_void_p(handle), None)
    except OSError:  # pragma: no cover - best effort
        pass


class Connection:
    """One control-socket connection (unix socket or Windows named pipe)."""

    def __init__(self, address: str, timeout: float) -> None:
        self.address = address
        self.timeout = timeout
        if is_windows_pipe_address(address):
            import os

            # Raw fd I/O rather than a Python file object: a buffered file
            # object's close() waits on the io-module lock held by a reader
            # thread still parked in read(), deadlocking every timeout path.
            # os.close on a raw fd invalidates the handle out from under the
            # blocked ReadFile instead.
            deadline = time.monotonic() + timeout
            while True:
                try:
                    self._fd = os.open(address, os.O_RDWR | getattr(os, "O_BINARY", 0))
                    self._sock = None
                    break
                except OSError:
                    if time.monotonic() >= deadline:
                        raise TransportError(f"cannot open pipe {address}")
                    time.sleep(0.2)
        else:
            import socket as socket_module

            sock = socket_module.socket(socket_module.AF_UNIX, socket_module.SOCK_STREAM)
            sock.settimeout(timeout)
            sock.connect(address)
            self._sock = sock
            self._fd = None

    def send_line(self, line: str) -> None:
        payload = line.encode("utf-8") + b"\n"
        if self._sock is not None:
            self._sock.sendall(payload)
        else:
            import os

            written = 0
            while written < len(payload):
                written += os.write(self._fd, payload[written:])

    def _read_chunk(self) -> bytes:
        if self._sock is not None:
            return self._sock.recv(65536)
        import os

        return os.read(self._fd, 65536)

    def read_reply(self, until_eof: bool) -> str:
        raw = _read_with_timeout(
            self._read_chunk, time.monotonic() + self.timeout, until_eof
        )
        text = raw.decode("utf-8", errors="replace")
        if text.endswith("\n"):
            text = text[:-1]
        return text

    def interrupt_read(self) -> None:
        """Wake a thread blocked in the transport's synchronous read."""
        try:
            if self._sock is not None:
                import socket as socket_module

                self._sock.shutdown(socket_module.SHUT_RDWR)
            elif self._fd is not None:
                _cancel_pending_io(self._fd)
        except OSError:
            pass

    def close(self) -> None:
        try:
            if self._sock is not None:
                self._sock.close()
            elif self._fd is not None:
                import os

                # A reader thread may be parked in a synchronous ReadFile on
                # this handle (event stream, timed-out reply). On Windows,
                # CloseHandle does not cancel pending synchronous I/O and can
                # block behind it, so cancel all outstanding I/O first.
                _cancel_pending_io(self._fd)
                os.close(self._fd)
                self._fd = None
        except OSError:
            pass


class EventCollector:
    """Dedicated events.stream connection collecting NDJSON frames."""

    def __init__(self, address: str, params: dict[str, Any], password: str | None, timeout: float) -> None:
        self._connection = Connection(address, timeout)
        if password:
            _authenticate(self._connection, password)
        request = json.dumps(
            {"method": "events.stream", "params": params}, separators=(",", ":")
        )
        self._connection.send_line(request)
        self.frames: list[Any] = []
        self._lock = threading.Lock()
        self._thread = threading.Thread(target=self._pump, daemon=True)
        self._thread.start()

    def _pump(self) -> None:
        buffer = b""
        while True:
            try:
                chunk = self._connection._read_chunk()
            except OSError:
                return
            if not chunk:
                return
            buffer += chunk
            while b"\n" in buffer:
                line, buffer = buffer.split(b"\n", 1)
                text = line.decode("utf-8", errors="replace")
                try:
                    frame: Any = json.loads(text)
                except json.JSONDecodeError:
                    frame = {"unparseable_event_line": text}
                with self._lock:
                    self.frames.append(frame)

    def stop(self) -> list[Any]:
        # Closing a Windows named-pipe fd while the pump is inside synchronous
        # ReadFile can wait behind that read. Cancel it first and let the pump
        # leave before releasing the handle.
        self._connection.interrupt_read()
        self._thread.join(timeout=2)
        self._connection.close()
        with self._lock:
            return list(self.frames)


def _authenticate(connection: Connection, password: str) -> None:
    connection.send_line(f"auth {password}")
    reply = connection.read_reply(until_eof=False)
    if reply.startswith("ERROR:"):
        raise TransportError(f"auth rejected: {reply}")


# ---------------------------------------------------------------------------
# Op execution
# ---------------------------------------------------------------------------


def evaluate_settle(
    settle: dict[str, Any], response: Any, previous_response: Any
) -> bool:
    """Whether one poll satisfies the settle predicates (all specified must hold).

    - ``until_absent``: none of the (placeholder-resolved) needle strings occur
      anywhere in the serialized reply;
    - ``until_present``: all needles occur;
    - ``until_window_not_visible``: every listed window id is absent or has
      ``visible:false`` in a well-formed ``window.list`` result;
    - ``stable``: the parsed reply equals the previous poll's parsed reply
      (needs at least two polls).
    """
    serialized = json.dumps(response, sort_keys=True)
    for needle in settle.get("until_absent", []):
        if needle in serialized:
            return False
    for needle in settle.get("until_present", []):
        if needle not in serialized:
            return False
    not_visible_ids = settle.get("until_window_not_visible", [])
    if not_visible_ids:
        result = response.get("result") if isinstance(response, dict) else None
        windows = result.get("windows") if isinstance(result, dict) else None
        if not isinstance(windows, list) or not all(isinstance(row, dict) for row in windows):
            return False
        for window_id in not_visible_ids:
            if any(
                row.get("id") == window_id and row.get("visible") is not False
                for row in windows
            ):
                return False
    if settle.get("stable") and (previous_response is None or response != previous_response):
        return False
    return True


class Driver:
    #: Consecutive timed-out ops (empty v2/v1 replies or CLI timeouts) after
    #: which the backend is declared unresponsive and every further op fails
    #: fast. A wedged main actor (see canonical run 29248166966: a blocking
    #: modal froze every subsequent socket command) otherwise burns a full
    #: op-timeout per remaining op and poisons the capture silently.
    MAX_CONSECUTIVE_TIMEOUTS = 3

    def __init__(
        self,
        socket_address: str,
        cli_path: str | None,
        password: str | None,
        restart_cmd: str | None,
        op_timeout: float,
    ) -> None:
        self.socket_address = socket_address
        self.cli_path = cli_path
        self.password = password
        self.restart_cmd = restart_cmd
        self.op_timeout = op_timeout
        self._consecutive_timeouts = 0

    def _record_timeout(self) -> None:
        self._consecutive_timeouts += 1

    def _record_reply(self) -> None:
        self._consecutive_timeouts = 0

    def _check_responsive(self) -> None:
        if self._consecutive_timeouts >= self.MAX_CONSECUTIVE_TIMEOUTS:
            raise TransportError(
                f"backend unresponsive: {self._consecutive_timeouts} consecutive op"
                " timeouts (wedged main actor?); failing fast — restart the app or"
                " investigate the last successful case"
            )

    def run_op(self, op: dict[str, Any]) -> dict[str, Any]:
        kind = op["op"]
        if kind == "sleep":
            time.sleep(op["seconds"])
            return {"kind": "sleep", "seconds": op["seconds"]}
        if kind == "restart":
            return self._run_restart()
        if kind == "v2":
            return self._run_v2(op)
        if kind == "v1":
            return self._run_v1(op)
        if kind == "cli":
            return self._run_cli(op)
        raise AssertionError(f"unreachable op kind {kind}")  # pragma: no cover

    def _request(self, line: str, until_eof: bool) -> str:
        connection = Connection(self.socket_address, self.op_timeout)
        try:
            if self.password:
                _authenticate(connection, self.password)
            connection.send_line(line)
            return connection.read_reply(until_eof=until_eof)
        finally:
            connection.close()

    def _v2_once(self, op: dict[str, Any]) -> dict[str, Any]:
        self._check_responsive()
        raw = self._request(build_v2_request(op["method"], op.get("params", {})), until_eof=False)
        if raw == "":
            self._record_timeout()
        else:
            self._record_reply()
        decoded = interpret_v2_response(raw)
        result = decoded["response"].get("result") if isinstance(decoded["response"], dict) else None
        return {
            "kind": "v2",
            "method": op["method"],
            "ok": decoded["ok"],
            "response": decoded["response"],
            "result": result,
            "error": decoded["error"],
        }

    def _run_v2(self, op: dict[str, Any]) -> dict[str, Any]:
        settle = op.get("settle")
        if not settle:
            return self._v2_once(op)
        # Root-sanctioned bounded settle: re-poll the same read until the
        # predicates hold or the bounded timeout elapses. The FINAL snapshot is
        # compared strictly; only the settle parameters and the satisfied flag
        # are recorded (poll counts/elapsed are timing noise). A timeout is
        # visible as satisfied:false — its own failure, never a silent pass.
        timeout_s = float(settle.get("timeout_s", SETTLE_DEFAULT_TIMEOUT_S))
        poll_interval_s = float(settle.get("poll_interval_s", SETTLE_DEFAULT_POLL_INTERVAL_S))
        deadline = time.monotonic() + timeout_s
        previous_response: Any = None
        result = self._v2_once(op)
        satisfied = evaluate_settle(settle, result["response"], previous_response)
        while not satisfied and time.monotonic() < deadline:
            time.sleep(poll_interval_s)
            previous_response = result["response"]
            result = self._v2_once(op)
            satisfied = evaluate_settle(settle, result["response"], previous_response)
        result["settle"] = {
            "predicate": {
                key: settle[key]
                for key in (
                    "until_absent",
                    "until_present",
                    "until_window_not_visible",
                    "stable",
                )
                if key in settle
            },
            "timeout_s": timeout_s,
            "poll_interval_s": poll_interval_s,
            "satisfied": satisfied,
        }
        return result

    def _run_v1(self, op: dict[str, Any]) -> dict[str, Any]:
        self._check_responsive()
        raw = self._request(
            build_v1_command_line(op["command"], op.get("args", [])), until_eof=True
        )
        if raw == "":
            self._record_timeout()
        else:
            self._record_reply()
        decoded = interpret_v1_response(raw)
        return {
            "kind": "v1",
            "command": op["command"],
            "ok": decoded["ok"],
            "response": decoded["response"],
            "error": decoded["error"],
        }

    def _run_cli(self, op: dict[str, Any]) -> dict[str, Any]:
        if not self.cli_path:
            raise TransportError("manifest contains a cli op but --cli was not provided")
        import os

        env = dict(os.environ)
        # Cross-platform socket injection: the macOS CLI and the Rust cmux-cli
        # both honor CMUX_SOCKET_PATH (crates/cmux-cli/src/socket.rs precedence
        # --socket > CMUX_SOCKET_PATH > CMUX_SOCKET > default).
        env.pop("CMUX_SOCKET", None)
        env.pop("CMUX_WORKSPACE_ID", None)
        env.pop("CMUX_SURFACE_ID", None)
        env.pop("CMUX_TAB_ID", None)
        env["CMUX_SOCKET_PATH"] = self.socket_address
        if self.password:
            env["CMUX_SOCKET_PASSWORD"] = self.password
        env.update(op.get("env", {}))
        self._check_responsive()
        try:
            completed = subprocess.run(
                [self.cli_path, *op["argv"]],
                capture_output=True,
                text=True,
                timeout=self.op_timeout,
                env=env,
            )
        except subprocess.TimeoutExpired:
            self._record_timeout()
            raise
        self._record_reply()
        return {
            "kind": "cli",
            "argv": op["argv"],
            "exit_status": completed.returncode,
            "stdout": completed.stdout,
            "stderr": completed.stderr,
        }

    def _run_restart(self) -> dict[str, Any]:
        if not self.restart_cmd:
            raise TransportError(
                "manifest contains a restart op but --restart-cmd was not provided"
            )
        output = (
            {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
            if os.name == "nt"
            else {"capture_output": True}
        )
        completed = subprocess.run(
            self.restart_cmd,
            shell=True,
            text=True,
            timeout=max(self.op_timeout, 300),
            **output,
        )
        if completed.returncode != 0:
            raise TransportError(
                f"restart command failed ({completed.returncode}): {(completed.stderr or '').strip()}"
            )
        # A relaunched app is a fresh backend: re-arm the responsiveness breaker.
        self._record_reply()
        return {"kind": "restart", "exit_status": completed.returncode}


# ---------------------------------------------------------------------------
# Session orchestration
# ---------------------------------------------------------------------------


def run_capture(
    manifest: dict[str, Any],
    driver: Driver,
    platform_label: str,
    output_lines: list[str],
) -> int:
    symbolizer = Symbolizer(socket_address=driver.socket_address)
    timing = TimingSymbolizer()
    output_lines.append(
        json.dumps(
            {
                "type": "session",
                "family": manifest["family"],
                "platform": platform_label,
                "driver_version": 2,
            },
            sort_keys=True,
        )
    )
    failures = 0

    session_results: list[dict[str, Any]] = []
    session_error: str | None = None
    for index, op in enumerate(manifest.get("session_setup", [])):
        try:
            resolved = resolve_placeholders(op, {"session": session_results})
            result = driver.run_op(resolved)
        except Exception as error:  # noqa: BLE001 - evidence beats a traceback
            # A dead session fixture dooms every case, but a partial capture
            # file with explicit per-case errors is far better rerun evidence
            # than an unhandled traceback and no output at all.
            session_error = f"session_setup[{index}] failed: {type(error).__name__}: {error}"
            break
        symbolizer.register(result)
        session_results.append(result)

    if session_error is not None:
        for case in manifest["cases"]:
            observation = shape_observation({"kind": "none"}, {}, None)
            output_lines.append(
                json.dumps(
                    {
                        "type": "case",
                        "id": case["id"],
                        "platform": platform_label,
                        "observation": observation,
                        "approved_differences": case.get("approved_differences", []),
                        "capture_error": session_error,
                    },
                    sort_keys=True,
                )
            )
        return len(manifest["cases"])

    for case in manifest["cases"]:
        context: dict[str, Any] = {"session": session_results, "setup": [], "action": None}
        collector: EventCollector | None = None
        case_error: str | None = None
        action_result: dict[str, Any] = {}
        probe_results: dict[str, list[dict[str, Any]]] = {}
        events: list[Any] | None = None
        try:
            for op in case.get("setup", []):
                result = driver.run_op(resolve_placeholders(op, context))
                symbolizer.register(result)
                context["setup"].append(result)
            if case.get("events"):
                collector = EventCollector(
                    driver.socket_address,
                    case.get("events_params", {"include_heartbeats": False}),
                    driver.password,
                    driver.op_timeout,
                )
                time.sleep(0.2)
            action_result = driver.run_op(resolve_placeholders(case["action"], context))
            symbolizer.register(action_result)
            context["action"] = action_result
            for lane, ops in case.get("probes", {}).items():
                lane_results: list[dict[str, Any]] = []
                context[lane] = []
                for op in ops:
                    result = driver.run_op(resolve_placeholders(op, context))
                    symbolizer.register(result)
                    context[lane].append(result)
                    # Drop the "result" convenience projection (placeholder
                    # ergonomics only); the full reply envelope is already in
                    # "response", and duplicating it doubles diff surface.
                    recorded = {k: v for k, v in result.items() if k != "result"}
                    lane_results.append({"op": describe_op(op), "result": recorded})
                probe_results[lane] = lane_results
            if collector is not None:
                time.sleep(0.3)
                events = timing.apply(collector.stop())
                collector = None
                symbolizer.register(events)
        except Exception as error:  # noqa: BLE001 - capture failure is per-case data
            case_error = f"{type(error).__name__}: {error}"
            failures += 1
        finally:
            if collector is not None:
                collector.stop()

        observation = shape_observation(action_result or {"kind": "none"}, probe_results, events)
        record = {
            "type": "case",
            "id": case["id"],
            "platform": platform_label,
            "observation": symbolizer.apply(observation),
            "approved_differences": case.get("approved_differences", []),
            "capture_error": case_error,
        }
        output_lines.append(json.dumps(record, sort_keys=True))
    return failures


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Manifest-driven live parity capture (canonical macOS socket "
        "or Windows named pipe); emits NDJSON observations for the differential harness."
    )
    parser.add_argument("--manifest", required=True, type=Path, help="case manifest JSON")
    parser.add_argument(
        "--socket",
        required=True,
        help=r"unix socket path, or \\.\pipe\<name> for the Windows backend",
    )
    parser.add_argument("--cli", help="path to the cmux CLI binary for cli ops")
    parser.add_argument("--platform", required=True, help="capture label, e.g. canonical|windows")
    parser.add_argument("--output", type=Path, help="NDJSON output path (default stdout)")
    parser.add_argument("--password", help="control-socket password (auth handshake)")
    parser.add_argument(
        "--restart-cmd",
        help="shell command that quits + relaunches the app and waits for the socket; "
        "required when the manifest contains restart ops",
    )
    parser.add_argument("--op-timeout", type=float, default=30.0, help="seconds per operation")
    args = parser.parse_args(argv)

    manifest = parse_manifest(json.loads(args.manifest.read_text(encoding="utf-8")))
    if manifest_needs_restart(manifest) and not args.restart_cmd:
        parser.error("manifest contains restart ops; --restart-cmd is required")

    driver = Driver(
        socket_address=args.socket,
        cli_path=args.cli,
        password=args.password,
        restart_cmd=args.restart_cmd,
        op_timeout=args.op_timeout,
    )
    output_lines: list[str] = []
    failures = run_capture(manifest, driver, args.platform, output_lines)
    rendered = "\n".join(output_lines) + "\n"
    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        sys.stdout.write(rendered)
    if failures:
        print(f"capture completed with {failures} case error(s)", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
