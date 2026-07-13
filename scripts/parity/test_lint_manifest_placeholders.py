import json
import unittest

from lint_manifest_placeholders import (
    collect_method_result_keys,
    lint_result_keys,
    lint_static_references,
)


def manifest(cases, session_setup=None):
    return {
        "family": "f",
        "session_setup": session_setup or [],
        "cases": cases,
    }


def v2(method, params=None):
    return {"op": "v2", "method": method, "params": params or {}}


class StaticReferenceLintTests(unittest.TestCase):
    def test_clean_manifest_has_no_findings(self):
        m = manifest(
            session_setup=[v2("workspace.create")],
            cases=[
                {
                    "id": "a",
                    "setup": [v2("surface.create", {"w": "${session.0.result.workspace_id}"})],
                    "action": v2("surface.close", {"s": "${setup.0.result.surface_id}"}),
                    "probes": {"state": [v2("surface.list", {"x": "${action.result.surface_id}"})]},
                }
            ],
        )
        self.assertEqual(lint_static_references(m), [])

    def test_out_of_range_session_and_setup_indices(self):
        m = manifest(
            session_setup=[v2("workspace.create")],
            cases=[
                {
                    "id": "a",
                    "setup": [v2("surface.create", {"w": "${session.5.result.x}"})],
                    "action": v2("surface.close", {"s": "${setup.9.result.y}"}),
                }
            ],
        )
        findings = lint_static_references(m)
        self.assertEqual(len(findings), 2)
        self.assertIn("does not exist", findings[0])
        self.assertIn("has not run yet", findings[1])

    def test_setup_op_cannot_reference_itself_or_later_setup(self):
        m = manifest(
            cases=[
                {
                    "id": "a",
                    "setup": [v2("surface.create", {"s": "${setup.0.result.surface_id}"})],
                    "action": v2("surface.list"),
                }
            ]
        )
        findings = lint_static_references(m)
        self.assertEqual(len(findings), 1)
        self.assertIn("has not run yet", findings[0])

    def test_action_reference_outside_probes_is_flagged(self):
        m = manifest(
            cases=[
                {
                    "id": "a",
                    "setup": [v2("surface.create", {"s": "${action.result.surface_id}"})],
                    "action": v2("surface.close", {"x": "${action.result.surface_id}"}),
                }
            ]
        )
        findings = lint_static_references(m)
        self.assertEqual(len(findings), 2)
        for finding in findings:
            self.assertIn("before it has run", finding)

    def test_unknown_section_is_flagged(self):
        m = manifest(
            cases=[{"id": "a", "action": v2("x", {"y": "${probes.0.result.z}"})}]
        )
        findings = lint_static_references(m)
        self.assertEqual(len(findings), 1)
        self.assertIn("unknown section", findings[0])


class SettlePlaceholderLintTests(unittest.TestCase):
    def test_settle_needle_placeholders_are_linted(self):
        m = manifest(
            cases=[
                {
                    "id": "a",
                    "setup": [v2("window.create")],
                    "action": {
                        "op": "v2", "method": "window.list", "params": {},
                        "settle": {"until_absent": ["${setup.7.result.window_id}"]},
                    },
                }
            ]
        )
        findings = lint_static_references(m)
        self.assertEqual(len(findings), 1)
        self.assertIn("has not run yet", findings[0])

    def test_settle_result_keys_checked_against_shapes(self):
        m = manifest(
            cases=[
                {
                    "id": "a",
                    "setup": [v2("window.create")],
                    "action": {
                        "op": "v2", "method": "window.list", "params": {},
                        "settle": {"until_absent": ["${setup.0.result.uuid}"]},
                    },
                }
            ]
        )
        findings = lint_result_keys(m, {"window.create": {"window_id", "window_ref"}})
        self.assertEqual(len(findings), 1)
        self.assertIn("'uuid'", findings[0])


class ResultKeyLintTests(unittest.TestCase):
    def capture_text(self):
        records = [
            {"type": "session", "family": "f", "platform": "canonical", "driver_version": 2},
            {
                "type": "case",
                "id": "shape-source",
                "observation": {
                    "response": {"id": 1, "ok": True, "result": {"window_id": "w", "window_ref": "r"}},
                    "state": [
                        {
                            "op": "v2:window.list",
                            "result": {"response": {"id": 1, "ok": True, "result": {"windows": []}}},
                        }
                    ],
                },
            },
        ]
        return "\n".join(json.dumps(r) for r in records)

    def test_collects_shapes_from_actions_and_probes(self):
        m = manifest(cases=[{"id": "shape-source", "action": v2("window.create")}])
        shapes = collect_method_result_keys(m, [self.capture_text()])
        self.assertEqual(shapes["window.create"], {"window_id", "window_ref"})
        self.assertEqual(shapes["window.list"], {"windows"})

    def test_flags_key_not_in_observed_shape(self):
        m = manifest(
            cases=[
                {
                    "id": "a",
                    "setup": [v2("window.create")],
                    "action": v2("window.close", {"w": "${setup.0.result.uuid}"}),
                }
            ]
        )
        findings = lint_result_keys(m, {"window.create": {"window_id", "window_ref"}})
        self.assertEqual(len(findings), 1)
        self.assertIn("'uuid'", findings[0])
        self.assertIn("window.create", findings[0])

    def test_known_key_and_unobserved_method_pass(self):
        m = manifest(
            session_setup=[v2("workspace.create")],
            cases=[
                {
                    "id": "a",
                    "setup": [v2("never.observed")],
                    "action": v2(
                        "x",
                        {
                            "a": "${session.0.result.workspace_id}",
                            "b": "${setup.0.result.anything}",
                        },
                    ),
                }
            ],
        )
        shapes = {"workspace.create": {"workspace_id"}}
        self.assertEqual(lint_result_keys(m, shapes), [])

    def test_non_result_paths_are_ignored(self):
        m = manifest(
            cases=[
                {
                    "id": "a",
                    "setup": [v2("window.list")],
                    "action": v2("x", {"w": "${setup.0.kind}"}),
                }
            ]
        )
        self.assertEqual(lint_result_keys(m, {"window.list": {"windows"}}), [])


if __name__ == "__main__":
    unittest.main()
