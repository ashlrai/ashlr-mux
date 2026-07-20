import json
import threading
import unittest

from capture_driver import (
    OBSERVATION_KEYS,
    TimingSymbolizer,
    UuidRefCanonicalizer,
    UuidRenumberer,
    ManifestError,
    PlaceholderError,
    Symbolizer,
    build_v1_command_line,
    build_v2_request,
    describe_op,
    interpret_v1_response,
    interpret_v2_response,
    is_windows_pipe_address,
    manifest_needs_restart,
    parse_manifest,
    resolve_placeholders,
    shape_observation,
    shell_quote,
)


def manifest(**overrides):
    payload = {
        "family": "pane_surface_lifecycle",
        "cases": [
            {
                "id": "case-1",
                "action": {"op": "v2", "method": "surface.list", "params": {}},
            }
        ],
    }
    payload.update(overrides)
    return payload


class ManifestParsingTests(unittest.TestCase):
    def test_valid_manifest_round_trips(self):
        payload = manifest(
            session_setup=[{"op": "v2", "method": "workspace.create", "params": {}}],
            cases=[
                {
                    "id": "a",
                    "events": True,
                    "setup": [{"op": "sleep", "seconds": 0.1}],
                    "action": {"op": "cli", "argv": ["tab-action", "--help"]},
                    "probes": {"state": [{"op": "v1", "command": "list_windows"}]},
                    "approved_differences": [
                        {"path": "/state/0/result", "rationale": "platform-scoped"}
                    ],
                }
            ],
        )
        self.assertIs(parse_manifest(payload), payload)

    def test_rejects_duplicate_case_ids(self):
        case = {"id": "dup", "action": {"op": "v2", "method": "m", "params": {}}}
        with self.assertRaisesRegex(ManifestError, "duplicate case id"):
            parse_manifest(manifest(cases=[case, dict(case)]))

    def test_rejects_unknown_op_kind(self):
        with self.assertRaisesRegex(ManifestError, "unknown op kind"):
            parse_manifest(manifest(cases=[{"id": "x", "action": {"op": "teleport"}}]))

    def test_rejects_probe_on_non_probe_lane(self):
        case = {
            "id": "x",
            "action": {"op": "v2", "method": "m", "params": {}},
            "probes": {"stdout": []},
        }
        with self.assertRaisesRegex(ManifestError, "probe lane 'stdout'"):
            parse_manifest(manifest(cases=[case]))

    def test_rejects_events_probe_ops(self):
        case = {
            "id": "x",
            "action": {"op": "v2", "method": "m", "params": {}},
            "probes": {"events": []},
        }
        with self.assertRaisesRegex(ManifestError, "events lane"):
            parse_manifest(manifest(cases=[case]))

    def test_rejects_approved_difference_without_rationale(self):
        case = {
            "id": "x",
            "action": {"op": "v2", "method": "m", "params": {}},
            "approved_differences": [{"path": "/response/a"}],
        }
        with self.assertRaisesRegex(ManifestError, "rationale"):
            parse_manifest(manifest(cases=[case]))

    def test_rejects_missing_action(self):
        with self.assertRaisesRegex(ManifestError, "requires an 'action'"):
            parse_manifest(manifest(cases=[{"id": "x"}]))

    def test_accepts_final_case_that_may_disconnect(self):
        payload = manifest(
            cases=[
                {
                    "id": "x",
                    "events": True,
                    "may_disconnect": True,
                    "action": {"op": "v2", "method": "window.close", "params": {}},
                }
            ]
        )
        self.assertIs(parse_manifest(payload), payload)

    def test_rejects_non_boolean_may_disconnect(self):
        case = {
            "id": "x",
            "may_disconnect": "yes",
            "action": {"op": "v2", "method": "window.close", "params": {}},
        }
        with self.assertRaisesRegex(ManifestError, "'may_disconnect' must be a boolean"):
            parse_manifest(manifest(cases=[case]))

    def test_rejects_nonfinal_case_that_may_disconnect(self):
        terminating_case = {
            "id": "terminating",
            "may_disconnect": True,
            "action": {"op": "v2", "method": "window.close", "params": {}},
        }
        ordinary_case = {
            "id": "ordinary",
            "action": {"op": "v2", "method": "window.list", "params": {}},
        }
        with self.assertRaisesRegex(ManifestError, "must be the final case"):
            parse_manifest(manifest(cases=[terminating_case, ordinary_case]))

    def test_rejects_probes_after_case_that_may_disconnect(self):
        case = {
            "id": "x",
            "may_disconnect": True,
            "action": {"op": "v2", "method": "window.close", "params": {}},
            "probes": {
                "state": [{"op": "v2", "method": "window.list", "params": {}}]
            },
        }
        with self.assertRaisesRegex(ManifestError, "cannot define post-action probes"):
            parse_manifest(manifest(cases=[case]))

    def test_detects_restart_ops_anywhere(self):
        self.assertFalse(manifest_needs_restart(parse_manifest(manifest())))
        case = {
            "id": "x",
            "action": {"op": "v2", "method": "m", "params": {}},
            "probes": {"persistence": [{"op": "restart"}]},
        }
        self.assertTrue(manifest_needs_restart(parse_manifest(manifest(cases=[case]))))


class WireEncodingTests(unittest.TestCase):
    def test_shell_quote_passes_safe_tokens_verbatim(self):
        self.assertEqual(shell_quote("workspace:1"), "workspace:1")
        self.assertEqual(shell_quote("a-b_c.d/e,f@g%h+i=j"), "a-b_c.d/e,f@g%h+i=j")

    def test_shell_quote_wraps_unsafe_and_empty_tokens(self):
        self.assertEqual(shell_quote(""), "''")
        self.assertEqual(shell_quote("two words"), "'two words'")
        self.assertEqual(shell_quote("don't"), "'don'\\''t'")

    def test_build_v1_command_line_joins_quoted_tokens(self):
        self.assertEqual(
            build_v1_command_line("notify_target", ["hello world", "ok"]),
            "notify_target 'hello world' ok",
        )

    def test_build_v2_request_is_single_line_with_fixed_id(self):
        line = build_v2_request("pane.create", {"direction": "right"})
        self.assertNotIn("\n", line)
        self.assertEqual(
            json.loads(line),
            {"id": 1, "method": "pane.create", "params": {"direction": "right"}},
        )

    def test_interpret_v2_success_keeps_full_envelope(self):
        decoded = interpret_v2_response('{"id":1,"ok":true,"result":{"x":1}}')
        self.assertTrue(decoded["ok"])
        self.assertEqual(decoded["response"]["result"], {"x": 1})
        self.assertIsNone(decoded["error"])

    def test_interpret_v2_failure_extracts_error_object(self):
        decoded = interpret_v2_response(
            '{"id":1,"ok":false,"error":{"code":"invalid_params","message":"nope"}}'
        )
        self.assertFalse(decoded["ok"])
        self.assertEqual(decoded["error"]["code"], "invalid_params")

    def test_interpret_v2_plain_text_error_and_invalid_json(self):
        self.assertEqual(
            interpret_v2_response("ERROR: no")["error"], {"plain_text": "ERROR: no"}
        )
        self.assertIn("invalid_v2_response", interpret_v2_response("not json")["error"])
        self.assertIn("invalid_v2_response", interpret_v2_response("[1,2]")["error"])

    def test_interpret_v1_error_prefix_is_failure(self):
        self.assertFalse(interpret_v1_response("ERROR: bad")["ok"])
        ok = interpret_v1_response("row one\nrow two")
        self.assertTrue(ok["ok"])
        self.assertEqual(ok["response"], "row one\nrow two")


class PlaceholderTests(unittest.TestCase):
    def setUp(self):
        self.context = {
            "session": [{"result": {"workspace_id": "W-1", "index": 3}}],
            "setup": [{"result": {"surface_id": "S-9"}}],
            "action": {"result": {"ok": True}},
        }

    def test_exact_placeholder_preserves_type(self):
        self.assertEqual(
            resolve_placeholders("${session.0.result.index}", self.context), 3
        )

    def test_nested_structures_and_embedded_placeholders(self):
        value = {
            "params": {
                "surface_id": "${setup.0.result.surface_id}",
                "label": "surface ${setup.0.result.surface_id} here",
                "list": ["${session.0.result.workspace_id}"],
            }
        }
        resolved = resolve_placeholders(value, self.context)
        self.assertEqual(resolved["params"]["surface_id"], "S-9")
        self.assertEqual(resolved["params"]["label"], "surface S-9 here")
        self.assertEqual(resolved["params"]["list"], ["W-1"])

    def test_unresolvable_reference_raises(self):
        with self.assertRaises(PlaceholderError):
            resolve_placeholders("${setup.5.result}", self.context)
        with self.assertRaises(PlaceholderError):
            resolve_placeholders("${session.0.result.missing}", self.context)

    def test_non_placeholder_values_pass_through(self):
        self.assertEqual(resolve_placeholders(42, self.context), 42)
        self.assertEqual(resolve_placeholders("plain", self.context), "plain")


class SymbolizerTests(unittest.TestCase):
    UUID_A = "AAAAAAAA-1111-2222-3333-444444444444"
    UUID_B = "bbbbbbbb-1111-2222-3333-444444444444"

    def test_symbolizes_by_first_seen_order_case_insensitively(self):
        symbolizer = Symbolizer()
        symbolizer.register({"created": self.UUID_A})
        symbolizer.register([self.UUID_B])
        applied = symbolizer.apply(
            {"rows": [self.UUID_B, self.UUID_A.lower(), self.UUID_A]}
        )
        self.assertEqual(applied["rows"], ["<uuid-2>", "<uuid-1>", "<uuid-1>"])

    def test_unregistered_uuid_allocates_next_symbol(self):
        symbolizer = Symbolizer()
        symbolizer.register(self.UUID_A)
        applied = symbolizer.apply(f"x {self.UUID_B} y")
        self.assertEqual(applied, "x <uuid-2> y")

    def test_socket_address_is_rewritten(self):
        symbolizer = Symbolizer(socket_address="/tmp/cmux-capture.sock")
        self.assertEqual(
            symbolizer.apply("listening on /tmp/cmux-capture.sock"),
            "listening on <socket>",
        )

    def test_dict_keys_are_symbolized(self):
        symbolizer = Symbolizer()
        applied = symbolizer.apply({self.UUID_A: 1})
        self.assertEqual(applied, {"<uuid-1>": 1})


class TimingSymbolizerTests(unittest.TestCase):
    """Sanctioned events-lane timing normalization: occurred_at + seq-derived
    event ids only. Counts, names, order, payload keys, and resume counters
    stay strict."""

    BOOT = "aaaaaaaa-1111-2222-3333-444444444444"

    def frames(self):
        return [
            {
                "boot_id": self.BOOT,
                "protocol": "cmux-events",
                "replay_count": 3,
                "resume": {"after_seq": 0, "latest_seq": 14, "next_seq": 15},
            },
            {
                "boot_id": self.BOOT,
                "id": f"{self.BOOT}-31",
                "name": "pane.created",
                "occurred_at": "2026-07-13T09:09:30.494Z",
                "payload": {"origin": "terminal_split"},
            },
            {
                "boot_id": self.BOOT,
                "id": f"{self.BOOT}-32",
                "name": "surface.created",
                "occurred_at": "2026-07-13T09:09:30.494Z",
            },
        ]

    def test_symbolizes_occurred_at_per_occurrence(self):
        timing = TimingSymbolizer()
        out = timing.apply(self.frames())
        self.assertEqual(out[1]["occurred_at"], "<ts-1>")
        # Per-occurrence: an identical raw timestamp still gets the next
        # symbol — coincidental sub-millisecond equality between adjacent
        # events is nondeterministic and deliberately not preserved.
        self.assertEqual(out[2]["occurred_at"], "<ts-2>")
        out2 = timing.apply([{"occurred_at": "2026-07-13T09:09:31.000Z"}])
        self.assertEqual(out2[0]["occurred_at"], "<ts-1>")

    def test_symbolizes_seq_derived_event_ids_raw_and_uuid_symbolized(self):
        timing = TimingSymbolizer()
        out = timing.apply(self.frames())
        self.assertEqual(out[1]["id"], "<event-id-1>")
        self.assertEqual(out[2]["id"], "<event-id-2>")
        # The same pattern with an already-uuid-symbolized boot part (archived
        # captures) also matches.
        out2 = timing.apply([{"id": "<uuid-8>-31", "name": "x"}])
        self.assertEqual(out2[0]["id"], "<event-id-1>")

    def test_strict_fields_untouched(self):
        out = TimingSymbolizer().apply(self.frames())
        self.assertEqual(out[0]["replay_count"], 3)
        # after_seq stays raw (pins the replay-default contract); the absolute
        # latest/next counters rebase per the 2026-07-13 root ruling addendum.
        self.assertEqual(
            out[0]["resume"],
            {"after_seq": 0, "latest_seq": "<seq+0>", "next_seq": "<seq+1>"},
        )
        self.assertEqual(out[1]["name"], "pane.created")
        self.assertEqual(out[1]["payload"], {"origin": "terminal_split"})
        self.assertEqual(out[0]["boot_id"], self.BOOT)  # uuid pass owns boot_id
        self.assertEqual(len(out), 3)

    def test_non_seq_ids_untouched(self):
        out = TimingSymbolizer().apply([{"id": "surface-2"}, {"id": self.BOOT}])
        self.assertEqual(out[0]["id"], "surface-2")
        self.assertEqual(out[1]["id"], self.BOOT)

    def test_idempotent_on_already_symbolized_capture(self):
        timing = TimingSymbolizer()
        once = timing.apply(self.frames())
        again = TimingSymbolizer().apply(once)
        self.assertEqual(once, again)

    def test_none_lane_passes_through(self):
        self.assertIsNone(TimingSymbolizer().apply(None))

    def test_subscription_ids_are_local_symbols_for_each_apply(self):
        timing = TimingSymbolizer()
        first = timing.apply([{"subscription_id": self.BOOT}])
        second = timing.apply([{"subscription_id": "<uuid-9>"}])
        self.assertEqual(first[0]["subscription_id"], "<subscription-id>")
        self.assertEqual(second[0]["subscription_id"], "<subscription-id>")


class SeqRebaseTests(unittest.TestCase):
    """Sanctioned offset-from-subscription symbolization of absolute seq counters."""

    def lane(self, base, extra_resume=None):
        resume = {"after_seq": None, "gap": False, "latest_seq": base,
                  "next_seq": base + 1, "oldest_seq": 1, "requested_after_seq": base}
        resume.update(extra_resume or {})
        return [
            {"boot_id": "<uuid-1>", "protocol": "cmux-events", "replay_count": 0, "resume": resume},
            {"boot_id": "<uuid-1>", "id": "<uuid-1>-31", "name": "pane.created",
             "occurred_at": "2026-07-13T09:00:00Z", "seq": base + 1},
            {"boot_id": "<uuid-1>", "id": "<uuid-1>-32", "name": "surface.created",
             "occurred_at": "2026-07-13T09:00:01Z", "seq": base + 2},
        ]

    def test_counters_rebase_to_subscription_point_across_boot_cardinality(self):
        canonical = TimingSymbolizer().apply(self.lane(30))
        windows = TimingSymbolizer().apply(self.lane(16))
        for out in (canonical, windows):
            self.assertEqual(out[0]["resume"]["latest_seq"], "<seq+0>")
            self.assertEqual(out[0]["resume"]["next_seq"], "<seq+1>")
            self.assertEqual(out[0]["resume"]["requested_after_seq"], "<seq+0>")
            self.assertEqual(out[1]["seq"], "<seq+1>")
            self.assertEqual(out[2]["seq"], "<seq+2>")
        self.assertEqual(canonical, windows)

    def test_after_oldest_gap_and_replay_count_stay_raw(self):
        out = TimingSymbolizer().apply(self.lane(30))
        self.assertIsNone(out[0]["resume"]["after_seq"])
        self.assertEqual(out[0]["resume"]["oldest_seq"], 1)
        self.assertIs(out[0]["resume"]["gap"], False)
        self.assertEqual(out[0]["replay_count"], 0)

    def test_relative_ordering_stays_strict(self):
        skipped = self.lane(30)
        skipped[2]["seq"] = 33  # a gap: 31 then 33
        normal = TimingSymbolizer().apply(self.lane(30))
        gapped = TimingSymbolizer().apply(skipped)
        self.assertEqual(normal[2]["seq"], "<seq+2>")
        self.assertEqual(gapped[2]["seq"], "<seq+3>")
        self.assertNotEqual(normal[2]["seq"], gapped[2]["seq"])

    def test_no_ack_lane_leaves_counters_raw(self):
        lane = [{"name": "pane.created", "seq": 31,
                 "occurred_at": "2026-07-13T09:00:00Z", "id": "<uuid-1>-31"}]
        out = TimingSymbolizer().apply(lane)
        self.assertEqual(out[0]["seq"], 31)

    def test_idempotent_after_rebase(self):
        once = TimingSymbolizer().apply(self.lane(30))
        again = TimingSymbolizer().apply(once)
        self.assertEqual(once, again)


class UuidRenumbererTests(unittest.TestCase):
    """Compare-time renumbering: symbol numbers become independent of wire
    key order; token renames only, values preserved."""

    def test_sorted_key_traversal_aligns_key_order_variants(self):
        # Same entities, opposite wire key order: renumbering aligns them.
        left, right = UuidRenumberer(), UuidRenumberer()
        left.register({"pane_id": "<uuid-2>", "surface_id": "<uuid-1>"})
        right.register({"surface_id": "<uuid-2>", "pane_id": "<uuid-1>"})
        self.assertEqual(
            left.apply({"pane_id": "<uuid-2>", "surface_id": "<uuid-1>"}),
            right.apply({"surface_id": "<uuid-2>", "pane_id": "<uuid-1>"})
            | {"pane_id": right.apply("<uuid-1>")},
        )
        # pane_id sorts before surface_id, so pane gets <uuid-1> on both sides.
        self.assertEqual(left.apply("<uuid-2>"), "<uuid-1>")
        self.assertEqual(right.apply("<uuid-1>"), "<uuid-1>")

    def test_idempotent(self):
        value = {"a": "<uuid-1>", "b": "<uuid-2>", "c": "x <uuid-1> y"}
        renumber = UuidRenumberer()
        renumber.register(value)
        once = renumber.apply(value)
        again = UuidRenumberer()
        again.register(once)
        self.assertEqual(again.apply(once), once)

    def test_list_order_preserved_and_unknown_tokens_pass_through(self):
        renumber = UuidRenumberer()
        renumber.register(["<uuid-9>", "<uuid-3>"])
        self.assertEqual(renumber.apply(["<uuid-9>", "<uuid-3>"]), ["<uuid-1>", "<uuid-2>"])
        self.assertEqual(renumber.apply("<uuid-7>"), "<uuid-7>")

    def test_non_symbol_strings_untouched(self):
        renumber = UuidRenumberer()
        renumber.register({"ref": "surface:4", "id": "surface-2"})
        self.assertEqual(
            renumber.apply({"ref": "surface:4", "id": "surface-2"}),
            {"ref": "surface:4", "id": "surface-2"},
        )


class UuidRefCanonicalizerTests(unittest.TestCase):
    def test_entity_tokens_are_canonicalized_by_their_stable_refs(self):
        left = {
            "window_id": "<uuid-9>",
            "window_ref": "window:3",
            "workspace": {"id": "<uuid-4>", "ref": "workspace:7"},
        }
        right = {
            "window_id": "<uuid-2>",
            "window_ref": "window:3",
            "workspace": {"id": "<uuid-8>", "ref": "workspace:7"},
        }
        canonicalizers = [UuidRefCanonicalizer(), UuidRefCanonicalizer()]
        for canonicalizer, value in zip(canonicalizers, [left, right], strict=True):
            canonicalizer.register(value)
        self.assertEqual(
            canonicalizers[0].apply(left),
            canonicalizers[1].apply(right),
        )
        self.assertEqual(
            canonicalizers[0].apply("owner=<uuid-4>"),
            "owner=<ref:workspace:7>",
        )

    def test_conflicting_ref_evidence_leaves_the_uuid_token_strict(self):
        canonicalizer = UuidRefCanonicalizer()
        canonicalizer.register(
            [
                {"id": "<uuid-4>", "ref": "workspace:7"},
                {"workspace_id": "<uuid-4>", "workspace_ref": "workspace:8"},
            ]
        )
        self.assertEqual(canonicalizer.apply("<uuid-4>"), "<uuid-4>")

    def test_non_entity_refs_and_non_uuid_ids_are_untouched(self):
        value = {"id": "surface-2", "ref": "surface:4", "note": "workspace:7"}
        canonicalizer = UuidRefCanonicalizer()
        canonicalizer.register(value)
        self.assertEqual(canonicalizer.apply(value), value)


class ObservationShapingTests(unittest.TestCase):
    def test_all_ten_lanes_always_present(self):
        observation = shape_observation({"kind": "v2", "response": {"ok": True}}, {}, None)
        self.assertEqual(tuple(observation.keys()), OBSERVATION_KEYS)
        for lane in ("exit_status", "stdout", "stderr", "state", "events"):
            self.assertIsNone(observation[lane])

    def test_cli_action_fills_process_lanes_only(self):
        observation = shape_observation(
            {"kind": "cli", "exit_status": 2, "stdout": "out", "stderr": "err"},
            {},
            None,
        )
        self.assertEqual(observation["exit_status"], 2)
        self.assertEqual(observation["stdout"], "out")
        self.assertEqual(observation["stderr"], "err")
        self.assertIsNone(observation["response"])
        self.assertIsNone(observation["error"])

    def test_v2_action_fills_response_and_error_lanes(self):
        observation = shape_observation(
            {"kind": "v2", "response": {"ok": False}, "error": {"code": "not_found"}},
            {"state": [{"op": "v2:surface.list", "result": {}}]},
            [{"name": "surface.created"}],
        )
        self.assertEqual(observation["error"], {"code": "not_found"})
        self.assertEqual(observation["state"][0]["op"], "v2:surface.list")
        self.assertEqual(observation["events"], [{"name": "surface.created"}])
        self.assertIsNone(observation["exit_status"])

    def test_observation_is_harness_compatible(self):
        import differential_harness

        observation = shape_observation({"kind": "v2", "response": {}}, {}, None)
        differential_harness.validate_observation(observation, "capture")


class RunCaptureTests(unittest.TestCase):
    """End-to-end shaping through run_capture with a stubbed driver."""

    class StubDriver:
        socket_address = "/tmp/stub.sock"
        password = None
        op_timeout = 1.0

        def __init__(self):
            self.calls = []

        def run_op(self, op):
            self.calls.append(op)
            if op["op"] == "v2" and op["method"] == "workspace.create":
                return {
                    "kind": "v2",
                    "method": op["method"],
                    "ok": True,
                    "response": {
                        "id": 1,
                        "ok": True,
                        "result": {"workspace_id": "aaaaaaaa-1111-2222-3333-444444444444"},
                    },
                    "result": {"workspace_id": "aaaaaaaa-1111-2222-3333-444444444444"},
                    "error": None,
                }
            return {
                "kind": "v2",
                "method": op["method"],
                "ok": True,
                "response": {"id": 1, "ok": True, "result": {"echo": op.get("params")}},
                "result": {"echo": op.get("params")},
                "error": None,
            }

    def test_run_capture_emits_session_header_and_shaped_cases(self):
        from capture_driver import run_capture

        payload = parse_manifest(
            {
                "family": "pane_surface_lifecycle",
                "session_setup": [
                    {"op": "v2", "method": "workspace.create", "params": {}}
                ],
                "cases": [
                    {
                        "id": "case-1",
                        "action": {
                            "op": "v2",
                            "method": "surface.list",
                            "params": {
                                "workspace_id": "${session.0.result.workspace_id}"
                            },
                        },
                        "probes": {
                            "state": [
                                {"op": "v2", "method": "surface.current", "params": {}}
                            ]
                        },
                    }
                ],
            }
        )
        driver = self.StubDriver()
        lines: list[str] = []
        failures = run_capture(payload, driver, "canonical", lines)
        self.assertEqual(failures, 0)
        self.assertEqual(len(lines), 2)

        header = json.loads(lines[0])
        self.assertEqual(header["type"], "session")
        self.assertEqual(header["platform"], "canonical")

        record = json.loads(lines[1])
        self.assertEqual(record["id"], "case-1")
        observation = record["observation"]
        self.assertEqual(sorted(observation), sorted(OBSERVATION_KEYS))
        # Placeholder resolved against the session fixture result.
        action_call = driver.calls[1]
        self.assertEqual(
            action_call["params"]["workspace_id"],
            "aaaaaaaa-1111-2222-3333-444444444444",
        )
        # The fixture UUID (first created) symbolized as <uuid-1> in output.
        self.assertEqual(
            observation["response"]["result"]["echo"]["workspace_id"], "<uuid-1>"
        )
        # Probe record present, without the duplicated "result" projection.
        probe = observation["state"][0]
        self.assertEqual(probe["op"], "v2:surface.current")
        self.assertNotIn("result", probe["result"])
        self.assertIn("response", probe["result"])
        # Unprobed lanes are explicit nulls.
        for lane in ("exit_status", "stdout", "stderr", "events", "persistence"):
            self.assertIsNone(observation[lane])
        self.assertIsNone(record["capture_error"])

    def test_event_collector_interrupts_reader_before_closing_connection(self):
        from capture_driver import EventCollector

        calls = []

        class ConnectionStub:
            def interrupt_read(self):
                calls.append("interrupt")

            def close(self):
                calls.append("close")

        class ThreadStub:
            def join(self, timeout):
                calls.append(("join", timeout))

        collector = EventCollector.__new__(EventCollector)
        collector._connection = ConnectionStub()
        collector._thread = ThreadStub()
        collector._lock = threading.Lock()
        collector.frames = [{"name": "window.closed"}]

        self.assertEqual(collector.stop(), [{"name": "window.closed"}])
        self.assertEqual(calls, ["interrupt", ("join", 2), "close"])

    def test_event_collector_discards_setup_events_but_preserves_subscription_ack(self):
        from capture_driver import EventCollector

        collector = EventCollector.__new__(EventCollector)
        collector._lock = threading.Lock()
        collector.frames = [
            {"type": "ack", "protocol": "cmux-events"},
            {"type": "event", "name": "notification.cleared"},
        ]

        collector.reset_after_setup(quiet_seconds=0.0)

        self.assertEqual(
            collector.frames,
            [{"type": "ack", "protocol": "cmux-events"}],
        )

    def test_run_capture_records_case_error_and_continues(self):
        from capture_driver import run_capture

        class FailingDriver(self.StubDriver):
            def run_op(self, op):
                if op["op"] == "v2" and op["method"] == "explodes":
                    raise RuntimeError("boom")
                return super().run_op(op)

        payload = parse_manifest(
            {
                "family": "pane_surface_lifecycle",
                "cases": [
                    {"id": "bad", "action": {"op": "v2", "method": "explodes", "params": {}}},
                    {"id": "good", "action": {"op": "v2", "method": "surface.list", "params": {}}},
                ],
            }
        )
        driver = FailingDriver()
        lines: list[str] = []
        failures = run_capture(payload, driver, "windows", lines)
        self.assertEqual(failures, 1)
        bad = json.loads(lines[1])
        self.assertIn("boom", bad["capture_error"])
        self.assertEqual(sorted(bad["observation"]), sorted(OBSERVATION_KEYS))
        good = json.loads(lines[2])
        self.assertIsNone(good["capture_error"])


class DriverHardeningTests(unittest.TestCase):
    def make_driver(self):
        from capture_driver import Driver

        return Driver(
            socket_address="/tmp/never-used.sock",
            cli_path=None,
            password=None,
            restart_cmd=None,
            op_timeout=0.1,
        )

    def test_breaker_trips_after_consecutive_timeouts(self):
        from capture_driver import TransportError

        driver = self.make_driver()
        driver._consecutive_timeouts = driver.MAX_CONSECUTIVE_TIMEOUTS
        with self.assertRaisesRegex(TransportError, "backend unresponsive"):
            driver.run_op({"op": "v2", "method": "surface.list", "params": {}})
        with self.assertRaisesRegex(TransportError, "backend unresponsive"):
            driver.run_op({"op": "v1", "command": "ping"})

    def test_breaker_resets_on_reply_and_restart(self):
        driver = self.make_driver()
        driver._consecutive_timeouts = 2
        driver._record_reply()
        self.assertEqual(driver._consecutive_timeouts, 0)
        driver._record_timeout()
        driver._record_timeout()
        self.assertEqual(driver._consecutive_timeouts, 2)

    def test_session_setup_failure_marks_every_case_and_writes_output(self):
        from capture_driver import run_capture

        class BoomDriver(RunCaptureTests.StubDriver):
            def run_op(self, op):
                raise RuntimeError("session boom")

        payload = parse_manifest(
            {
                "family": "f",
                "session_setup": [{"op": "v2", "method": "workspace.create", "params": {}}],
                "cases": [
                    {"id": "a", "action": {"op": "v2", "method": "m", "params": {}}},
                    {"id": "b", "action": {"op": "v2", "method": "m", "params": {}}},
                ],
            }
        )
        lines: list[str] = []
        failures = run_capture(payload, BoomDriver(), "canonical", lines)
        self.assertEqual(failures, 2)
        self.assertEqual(len(lines), 3)  # header + both cases
        for line in lines[1:]:
            record = json.loads(line)
            self.assertIn("session_setup[0] failed", record["capture_error"])
            self.assertEqual(sorted(record["observation"]), sorted(OBSERVATION_KEYS))


class SettleTests(unittest.TestCase):
    """Root-sanctioned bounded settle semantics for reads racing async teardown."""

    def test_validation_accepts_predicates_and_rejects_garbage(self):
        ok = {
            "id": "a",
            "action": {"op": "v2", "method": "window.list", "params": {},
                       "settle": {"until_absent": ["x"], "timeout_s": 5}},
        }
        parse_manifest({"family": "f", "cases": [ok]})
        parse_manifest(
            {
                "family": "f",
                "cases": [
                    {
                        "id": "a",
                        "action": {
                            "op": "v2",
                            "method": "window.list",
                            "params": {},
                            "settle": {"until_window_not_visible": ["x"]},
                        },
                    }
                ],
            }
        )
        for bad_settle, why in [
            ({}, "requires at least one predicate"),
            ({"until_absent": []}, "non-empty list"),
            ({"until_window_not_visible": []}, "non-empty list"),
            ({"stable": False}, "must be true"),
            ({"until_present": ["x"], "timeout_s": 0}, "positive number"),
            ({"until_absent": ["x"], "bogus": 1}, "unknown settle keys"),
        ]:
            case = {"id": "a", "action": {"op": "v2", "method": "m", "params": {}, "settle": bad_settle}}
            with self.assertRaisesRegex(ManifestError, why):
                parse_manifest({"family": "f", "cases": [case]})
        with self.assertRaisesRegex(ManifestError, "only supported on v2"):
            parse_manifest({"family": "f", "cases": [
                {"id": "a", "action": {"op": "cli", "argv": ["x"], "settle": {"stable": True}}}
            ]})

    def test_evaluate_settle_predicates(self):
        from capture_driver import evaluate_settle

        response = {"ok": True, "result": {"windows": [{"id": "keep-1"}, {"id": "zombie-2"}]}}
        self.assertFalse(evaluate_settle({"until_absent": ["zombie-2"]}, response, None))
        self.assertTrue(evaluate_settle({"until_absent": ["gone-3"]}, response, None))
        self.assertTrue(evaluate_settle({"until_present": ["keep-1", "zombie-2"]}, response, None))
        self.assertFalse(evaluate_settle({"until_present": ["keep-1", "gone-3"]}, response, None))
        # stable: needs a previous identical poll.
        self.assertFalse(evaluate_settle({"stable": True}, response, None))
        self.assertFalse(evaluate_settle({"stable": True}, response, {"ok": True, "result": {}}))
        self.assertTrue(evaluate_settle({"stable": True}, response, dict(response)))
        # Combined: every specified predicate must hold.
        self.assertFalse(
            evaluate_settle({"until_absent": ["zombie-2"], "stable": True}, response, dict(response))
        )

    def test_window_not_visible_accepts_hidden_or_absent_and_rejects_visible(self):
        from capture_driver import evaluate_settle

        response = {
            "ok": True,
            "result": {"windows": [
                {"id": "visible", "visible": True},
                {"id": "hidden", "visible": False},
            ]},
        }
        self.assertFalse(
            evaluate_settle({"until_window_not_visible": ["visible"]}, response, None)
        )
        self.assertTrue(
            evaluate_settle({"until_window_not_visible": ["hidden"]}, response, None)
        )
        self.assertTrue(
            evaluate_settle({"until_window_not_visible": ["absent"]}, response, None)
        )
        self.assertFalse(
            evaluate_settle(
                {"until_window_not_visible": ["hidden"]},
                {"ok": True, "result": {}},
                None,
            )
        )

    class ScriptedDriver:
        """Driver with _v2_once replaced by a scripted reply sequence."""

        def __init__(self, replies):
            from capture_driver import Driver

            self.driver = Driver(
                socket_address="/tmp/unused.sock", cli_path=None, password=None,
                restart_cmd=None, op_timeout=1.0,
            )
            self.calls = 0
            replies = list(replies)

            def scripted(_op):
                reply = replies[min(self.calls, len(replies) - 1)]
                self.calls += 1
                return {"kind": "v2", "method": "window.list", "ok": True,
                        "response": reply, "result": reply.get("result"), "error": None}

            self.driver._v2_once = scripted

    def test_settle_polls_until_predicate_holds_and_records_parameters(self):
        zombie = {"ok": True, "result": {"windows": [{"id": "dead-window"}]}}
        clean = {"ok": True, "result": {"windows": []}}
        scripted = self.ScriptedDriver([zombie, zombie, clean])
        result = scripted.driver.run_op({
            "op": "v2", "method": "window.list", "params": {},
            "settle": {"until_absent": ["dead-window"], "timeout_s": 5, "poll_interval_s": 0.01},
        })
        self.assertEqual(scripted.calls, 3)
        self.assertEqual(result["response"], clean)
        self.assertEqual(result["settle"], {
            "predicate": {"until_absent": ["dead-window"]},
            "timeout_s": 5.0,
            "poll_interval_s": 0.01,
            "satisfied": True,
        })

    def test_settle_stable_mode_requires_two_identical_polls(self):
        a = {"ok": True, "result": {"n": 1}}
        b = {"ok": True, "result": {"n": 2}}
        scripted = self.ScriptedDriver([a, b, b])
        result = scripted.driver.run_op({
            "op": "v2", "method": "window.list", "params": {},
            "settle": {"stable": True, "timeout_s": 5, "poll_interval_s": 0.01},
        })
        self.assertEqual(scripted.calls, 3)
        self.assertTrue(result["settle"]["satisfied"])
        self.assertEqual(result["response"], b)

    def test_settle_timeout_records_satisfied_false_with_last_snapshot(self):
        zombie = {"ok": True, "result": {"windows": [{"id": "dead-window"}]}}
        scripted = self.ScriptedDriver([zombie])
        result = scripted.driver.run_op({
            "op": "v2", "method": "window.list", "params": {},
            "settle": {"until_absent": ["dead-window"], "timeout_s": 0.05, "poll_interval_s": 0.01},
        })
        self.assertFalse(result["settle"]["satisfied"])
        self.assertEqual(result["response"], zombie)
        self.assertGreaterEqual(scripted.calls, 2)

    def test_unsettled_v2_op_records_no_settle_key(self):
        clean = {"ok": True, "result": {}}
        scripted = self.ScriptedDriver([clean])
        result = scripted.driver.run_op({"op": "v2", "method": "window.list", "params": {}})
        self.assertNotIn("settle", result)
        self.assertEqual(scripted.calls, 1)


class MiscTests(unittest.TestCase):
    def test_pipe_address_detection(self):
        self.assertTrue(is_windows_pipe_address("\\\\.\\pipe\\cmux"))
        self.assertFalse(is_windows_pipe_address("/tmp/cmux-debug.sock"))

    def test_describe_op(self):
        self.assertEqual(
            describe_op({"op": "v2", "method": "pane.create"}), "v2:pane.create"
        )
        self.assertEqual(describe_op({"op": "v1", "command": "ping"}), "v1:ping")
        self.assertEqual(
            describe_op({"op": "cli", "argv": ["tab-action", "--help"]}),
            "cli:tab-action --help",
        )
        self.assertEqual(describe_op({"op": "restart"}), "restart")


if __name__ == "__main__":
    unittest.main()
