import json
import unittest

from capture_driver import (
    OBSERVATION_KEYS,
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
