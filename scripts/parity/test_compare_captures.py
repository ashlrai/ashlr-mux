import json
import unittest

from compare_captures import (
    CaptureFormatError,
    compare_captures,
    load_capture,
    normalize_racy_window_list_order,
    render_summary,
)
from differential_harness import OBSERVATION_KEYS


def observation(**changes):
    value = {key: None for key in OBSERVATION_KEYS}
    value.update(changes)
    return value


def case_record(case_id, obs=None, approved=None, capture_error=None):
    return {
        "type": "case",
        "id": case_id,
        "platform": "x",
        "observation": obs or observation(),
        "approved_differences": approved or [],
        "capture_error": capture_error,
    }


def capture_text(records):
    header = {"type": "session", "family": "f", "platform": "x", "driver_version": 1}
    return "\n".join(json.dumps(r) for r in [header, *records]) + "\n"


def window_list_probe(rows):
    return {
        "op": "v2:window.list",
        "result": {
            "response": {"result": {"windows": rows}},
        },
    }


def window_row(ref, index, window_id, **changes):
    row = {
        "id": window_id,
        "index": index,
        "key": False,
        "ref": ref,
        "selected_workspace_id": f"{window_id}-workspace",
        "selected_workspace_ref": f"workspace:{ref.split(':')[1]}",
        "visible": True,
        "workspace_count": 1,
    }
    row.update(changes)
    return row


class LoadCaptureTests(unittest.TestCase):
    def test_skips_session_lines_and_indexes_cases(self):
        cases = load_capture(capture_text([case_record("a"), case_record("b")]), "c")
        self.assertEqual(sorted(cases), ["a", "b"])

    def test_rejects_duplicate_case_ids(self):
        with self.assertRaisesRegex(CaptureFormatError, "duplicate case id"):
            load_capture(capture_text([case_record("a"), case_record("a")]), "c")

    def test_rejects_invalid_json_and_unknown_types(self):
        with self.assertRaisesRegex(CaptureFormatError, "invalid JSON"):
            load_capture("not json\n", "c")
        with self.assertRaisesRegex(CaptureFormatError, "unknown record type"):
            load_capture(json.dumps({"type": "mystery"}) + "\n", "c")

    def test_rejects_case_without_observation(self):
        record = case_record("a")
        del record["observation"]
        with self.assertRaisesRegex(CaptureFormatError, "missing observation"):
            load_capture(json.dumps(record) + "\n", "c")


class CompareCapturesTests(unittest.TestCase):
    def test_identical_captures_pass(self):
        left = {"a": case_record("a", observation(response={"v": 1}))}
        right = {"a": case_record("a", observation(response={"v": 1}))}
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 0)
        self.assertEqual(report["identical"], 1)
        self.assertIn("PASS: zero deltas", render_summary(report))

    def test_lane_difference_is_reported_with_detail(self):
        left = {"a": case_record("a", observation(response={"v": 1}, state=[1]))}
        right = {"a": case_record("a", observation(response={"v": 2}, state=[1]))}
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 1)
        result = report["results"][0]
        self.assertEqual(result["mismatches"], ["response"])
        self.assertEqual(result["detail"]["response"]["canonical"], {"v": 1})
        self.assertEqual(result["detail"]["response"]["windows"], {"v": 2})

    def test_missing_case_on_either_side_is_a_delta_never_a_skip(self):
        left = {"only-canonical": case_record("only-canonical")}
        right = {"only-windows": case_record("only-windows")}
        report = compare_captures(left, right)
        self.assertEqual(report["cases"], 2)
        self.assertEqual(report["deltas"], 2)
        by_id = {r["id"]: r for r in report["results"]}
        self.assertEqual(by_id["only-canonical"]["mismatches"], ["missing_windows"])
        self.assertEqual(by_id["only-windows"]["mismatches"], ["missing_canonical"])
        self.assertFalse(report["capture_integrity"]["valid_for_promotion"])
        self.assertEqual(
            report["capture_integrity"]["missing_cases"],
            ["only-canonical", "only-windows"],
        )

    def test_approved_difference_covers_platform_scoped_delta(self):
        approved = [{"path": "/response/title", "rationale": "platform shell name"}]
        left = {
            "a": case_record(
                "a", observation(response={"title": "zsh", "v": 1}), approved=approved
            )
        }
        right = {
            "a": case_record(
                "a", observation(response={"title": "pwsh", "v": 1}), approved=approved
            )
        }
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 0)

    def test_approved_differences_mismatch_between_sides_is_a_delta(self):
        approved = [{"path": "/response/title", "rationale": "platform shell name"}]
        left = {"a": case_record("a", approved=approved)}
        right = {"a": case_record("a", approved=[])}
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 1)
        self.assertEqual(
            report["results"][0]["mismatches"], ["approved_differences_mismatch"]
        )

    def test_capture_error_marks_delta_even_when_observations_match(self):
        left = {"a": case_record("a")}
        right = {"a": case_record("a", capture_error="TransportError: boom")}
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 1)
        result = report["results"][0]
        self.assertIn("capture_error", result["mismatches"])
        summary = render_summary(report)
        self.assertIn("capture_error[windows]: TransportError: boom", summary)
        self.assertFalse(report["capture_integrity"]["valid_for_promotion"])
        self.assertEqual(
            report["capture_integrity"]["capture_error_cases"],
            {"canonical": [], "windows": ["a"]},
        )

    def test_timing_symbolization_applied_at_load_covers_timestamp_noise(self):
        boot_c = "aaaaaaaa-1111-2222-3333-444444444444"
        boot_w = "bbbbbbbb-1111-2222-3333-444444444444"

        def events(boot, ts):
            return [
                {"boot_id": "<uuid-1>", "protocol": "cmux-events", "replay_count": 0},
                {
                    "boot_id": "<uuid-1>",
                    "id": f"{boot}-31",
                    "name": "pane.created",
                    "occurred_at": ts,
                },
            ]

        left_text = capture_text(
            [case_record("a", observation(events=events(boot_c, "2026-07-13T09:00:00.1Z")))]
        )
        right_text = capture_text(
            [case_record("a", observation(events=events(boot_w, "2026-07-13T10:30:59.9Z")))]
        )
        left = load_capture(left_text, "canonical")
        right = load_capture(right_text, "windows")
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 0)

    def test_timing_symbolization_keeps_count_and_name_divergences_strict(self):
        frame = {
            "boot_id": "<uuid-1>",
            "id": "<uuid-1>-1",
            "name": "pane.created",
            "occurred_at": "2026-07-13T09:00:00Z",
        }
        extra = dict(frame, id="<uuid-1>-2", name="session.changed")
        left = load_capture(capture_text([case_record("a", observation(events=[frame]))]), "c")
        right = load_capture(
            capture_text([case_record("a", observation(events=[frame, extra]))]), "w"
        )
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 1)
        self.assertEqual(report["results"][0]["mismatches"], ["events"])

    def test_later_exact_event_case_is_not_shifted_by_earlier_event_count_delta(self):
        boot_c = "<uuid-1>"
        boot_w = "<uuid-8>"

        def event(boot, seq, name):
            return {
                "boot_id": boot,
                "id": f"{boot}-{seq}",
                "name": name,
                "occurred_at": f"2026-07-19T00:00:{seq:02d}Z",
            }

        left = load_capture(
            capture_text(
                [
                    case_record(
                        "a",
                        observation(
                            events=[event(boot_c, 1, "one"), event(boot_c, 2, "extra")]
                        ),
                    ),
                    case_record("b", observation(events=[event(boot_c, 3, "same")])),
                ]
            ),
            "canonical",
        )
        right = load_capture(
            capture_text(
                [
                    case_record("a", observation(events=[event(boot_w, 1, "one")])),
                    case_record("b", observation(events=[event(boot_w, 2, "same")])),
                ]
            ),
            "windows",
        )

        report = compare_captures(left, right)

        by_id = {result["id"]: result for result in report["results"]}
        self.assertFalse(by_id["a"]["identical"])
        self.assertTrue(by_id["b"]["identical"])

    def test_renumbering_ignores_uuids_inside_approved_regions(self):
        # An approved-away probe blob contains platform-different uuid sets;
        # they must not desynchronize the symbol tables for later values.
        approved = [{"path": "/state", "rationale": "platform-scoped probe blob"}]
        left = {
            "a": case_record(
                "a",
                observation(state=[{"noise": "<uuid-1>"}], response={"id": "<uuid-2>"}),
                approved=approved,
            )
        }
        right = {
            "a": case_record(
                "a",
                observation(state=[{"noise": "<uuid-1>", "extra": "<uuid-2>"}], response={"id": "<uuid-3>"}),
                approved=approved,
            )
        }
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 0)

    def test_renumbering_still_aligns_key_order_dependent_allocation(self):
        left = {
            "a": case_record(
                "a", observation(response={"pane_id": "<uuid-2>", "surface_id": "<uuid-1>"})
            )
        }
        right = {
            "a": case_record(
                "a", observation(response={"pane_id": "<uuid-1>", "surface_id": "<uuid-2>"})
            )
        }
        report = compare_captures(left, right)
        self.assertEqual(report["deltas"], 0)

    def test_stable_refs_align_entity_uuids_despite_earlier_platform_only_ids(self):
        left = {
            "a": case_record(
                "a",
                observation(events=[{"boot_id": "<uuid-1>"}, {"noise": "<uuid-2>"}]),
            ),
            "b": case_record(
                "b",
                observation(
                    response={
                        "window_id": "<uuid-4>",
                        "window_ref": "window:3",
                        "workspace_id": "<uuid-5>",
                        "workspace_ref": "workspace:7",
                    }
                ),
            ),
        }
        right = {
            "a": case_record(
                "a",
                observation(events=[{"boot_id": "<uuid-8>"}]),
            ),
            "b": case_record(
                "b",
                observation(
                    response={
                        "window_id": "<uuid-2>",
                        "window_ref": "window:3",
                        "workspace_id": "<uuid-3>",
                        "workspace_ref": "workspace:7",
                    }
                ),
            ),
        }

        report = compare_captures(left, right)

        self.assertEqual(report["deltas"], 1)
        by_id = {result["id"]: result for result in report["results"]}
        self.assertFalse(by_id["a"]["identical"])
        self.assertTrue(by_id["b"]["identical"])

    def test_event_ref_conflicts_do_not_poison_response_identity(self):
        left = {
            "a": case_record(
                "a",
                observation(
                    response={
                        "workspace_id": "<uuid-9>",
                        "workspace_ref": "workspace:7",
                    },
                    events=[
                        {
                            "workspace_id": "<uuid-9>",
                            "workspace_ref": "workspace:2",
                        }
                    ],
                ),
            )
        }
        right = {
            "a": case_record(
                "a",
                observation(
                    response={
                        "workspace_id": "<uuid-2>",
                        "workspace_ref": "workspace:7",
                    },
                    events=[
                        {
                            "workspace_id": "<uuid-2>",
                            "workspace_ref": "workspace:7",
                        }
                    ],
                ),
            )
        }

        result = compare_captures(left, right)["results"][0]

        self.assertEqual(result["mismatches"], ["events"])
        self.assertNotIn("response", result["detail"])

    def test_window_list_order_and_positional_indices_are_normalized_by_ref(self):
        left_rows = [
            window_row("window:3", 0, "<uuid-1>"),
            window_row("window:1", 1, "<uuid-2>"),
            window_row("window:2", 2, "<uuid-3>"),
        ]
        right_rows = [
            window_row("window:1", 0, "<uuid-1>"),
            window_row("window:2", 1, "<uuid-2>"),
            window_row("window:3", 2, "<uuid-3>"),
        ]
        left = {"a": case_record("a", observation(multiwindow=[window_list_probe(left_rows)]))}
        right = {"a": case_record("a", observation(multiwindow=[window_list_probe(right_rows)]))}

        report = compare_captures(left, right)

        self.assertEqual(report["deltas"], 0)

    def test_window_list_normalization_keeps_row_fields_strict(self):
        left_rows = [
            window_row("window:2", 0, "<uuid-1>"),
            window_row("window:1", 1, "<uuid-2>"),
        ]
        right_rows = [
            window_row("window:1", 0, "<uuid-1>"),
            window_row("window:2", 1, "<uuid-2>", workspace_count=2),
        ]
        left = {"a": case_record("a", observation(state=[window_list_probe(left_rows)]))}
        right = {"a": case_record("a", observation(state=[window_list_probe(right_rows)]))}

        report = compare_captures(left, right)

        self.assertEqual(report["deltas"], 1)
        self.assertEqual(report["results"][0]["mismatches"], ["state"])

    def test_window_list_with_invalid_positional_index_is_not_normalized(self):
        probe = window_list_probe(
            [
                window_row("window:2", 1, "<uuid-1>"),
                window_row("window:1", 0, "<uuid-2>"),
            ]
        )

        normalized = normalize_racy_window_list_order(
            observation(multiwindow=[probe])
        )

        self.assertEqual(
            normalized["multiwindow"][0]["result"]["response"]["result"]["windows"],
            probe["result"]["response"]["result"]["windows"],
        )

    def test_unsatisfied_settle_invalidates_capture_for_promotion(self):
        probe = window_list_probe([])
        probe["result"]["settle"] = {"satisfied": False}
        left = {"a": case_record("a", observation(state=[probe]))}
        right = {"a": case_record("a", observation(state=[probe]))}

        report = compare_captures(left, right)

        self.assertEqual(report["deltas"], 0)
        self.assertFalse(report["capture_integrity"]["valid_for_promotion"])
        self.assertEqual(
            report["capture_integrity"]["unsettled_cases"],
            {"canonical": ["a"], "windows": ["a"]},
        )
        self.assertIn("INVALID FOR PROMOTION", render_summary(report))

    def test_satisfied_settle_keeps_capture_promotion_eligible(self):
        probe = window_list_probe([])
        probe["result"]["settle"] = {"satisfied": True}
        left = {"a": case_record("a", observation(state=[probe]))}
        right = {"a": case_record("a", observation(state=[probe]))}

        report = compare_captures(left, right)

        self.assertTrue(report["capture_integrity"]["valid_for_promotion"])

    def test_manifest_overrides_recorded_approved_differences(self):
        left = {"a": case_record("a", observation(response={"title": "zsh"}), approved=[])}
        right = {"a": case_record("a", observation(response={"title": "pwsh"}), approved=[])}
        overrides = {"a": [{"path": "/response/title", "rationale": "platform shell"}]}
        report = compare_captures(left, right, overrides)
        self.assertEqual(report["deltas"], 0)
        # Overrides also skip the recorded-set equality check.
        mismatched = {
            "a": case_record(
                "a",
                observation(response={"title": "pwsh"}),
                approved=[{"path": "/x", "rationale": "stale"}],
            )
        }
        report = compare_captures(left, mismatched, overrides)
        self.assertEqual(report["deltas"], 0)

    def test_manifest_approved_differences_extraction(self):
        from compare_captures import manifest_approved_differences

        manifest = {
            "cases": [
                {"id": "a", "approved_differences": [{"path": "/p", "rationale": "r"}]},
                {"id": "b"},
            ]
        }
        self.assertEqual(
            manifest_approved_differences(manifest),
            {"a": [{"path": "/p", "rationale": "r"}], "b": []},
        )

    def test_result_order_follows_canonical_then_windows_strays(self):
        left = {"a": case_record("a"), "b": case_record("b")}
        right = {"b": case_record("b"), "z": case_record("z")}
        report = compare_captures(left, right)
        self.assertEqual([r["id"] for r in report["results"]], ["a", "b", "z"])


if __name__ == "__main__":
    unittest.main()
