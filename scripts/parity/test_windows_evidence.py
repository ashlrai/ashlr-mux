#!/usr/bin/env python3
"""Focused invariants for the pinned Windows evidence artifact."""

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from extract_windows_evidence import PINNED_WINDOWS_COMMIT, validate


ROOT = Path(__file__).resolve().parents[2]


class WindowsEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.data = json.loads(
            (ROOT / "docs/parity/source/windows_evidence.json").read_text(encoding="utf-8")
        )

    def test_catalog_self_validates(self) -> None:
        self.assertEqual(validate(self.data), [])

    def test_catalog_is_pinned_not_ambient(self) -> None:
        self.assertEqual(
            self.data["generated_from"]["repository_commit"], PINNED_WINDOWS_COMMIT
        )
        self.assertIn("pinned commit", self.data["generated_from"]["source_mode"])

    def test_advertisement_and_routing_are_separate_complete_fields(self) -> None:
        ping = next(
            row for row in self.data["control_socket_methods"] if row["method"] == "system.ping"
        )
        self.assertTrue(ping["advertised"])
        self.assertTrue(ping["routed"])
        self.assertEqual(ping["verification_claim"].split(";")[0], "unverified")

    def test_known_help_and_unsupported_anchors_are_extracted(self) -> None:
        panes = next(row for row in self.data["cli_commands"] if row["command"] == "list-panes")
        self.assertTrue(panes["has_concrete_help"])
        self.assertEqual(panes["control_methods"], ["pane.list"])
        self.assertIn("browser.viewport.set", self.data["explicit_unsupported_methods"])

    def test_window_lifecycle_cli_uses_its_typed_executor(self) -> None:
        rows = {row["command"]: row for row in self.data["cli_commands"]}
        for command in ("new-window", "focus-window", "close-window"):
            with self.subTest(command=command):
                self.assertEqual(rows[command]["executor"], "window_lifecycle")
                self.assertEqual(rows[command]["dispatch_outcome"], "special_executor")
                self.assertNotEqual(
                    rows[command]["dispatch_outcome"],
                    "explicit_socket_command_not_ported",
                )

    def test_reachable_window_namespace_mapping_is_cataloged(self) -> None:
        rows = {row["command"]: row for row in self.data["cli_commands"]}
        window = rows["window"]
        self.assertTrue(window["top_level_known"])
        self.assertEqual(window["dispatch_outcome"], "control_mapping")
        self.assertEqual(window["control_mapping_kind"], "helper_dispatch")
        self.assertEqual(window["mapping_helper"], "window_command")


if __name__ == "__main__":
    unittest.main()
