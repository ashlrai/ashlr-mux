import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / "scripts" / "extract_canonical_cli.py"
CATALOG = REPO / "docs" / "parity" / "source" / "canonical_cli.json"
REVISION = "e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452"


class CanonicalCLIExtractionTests(unittest.TestCase):
    def test_checked_in_catalog_is_deterministic(self):
        subprocess.run(
            [sys.executable, str(SCRIPT), "--repo", str(REPO), "--revision", REVISION, "--check"],
            check=True,
        )

    def test_source_truth_count_and_contract_shape(self):
        catalog = json.loads(CATALOG.read_text(encoding="utf-8"))
        self.assertEqual(catalog["canonical_revision"], REVISION)
        self.assertEqual(catalog["counts"]["accepted_top_level_tokens"], 174)
        self.assertEqual(catalog["counts"]["public_top_level_commands"], 158)
        self.assertEqual(catalog["counts"]["hidden_or_internal_top_level_commands"], 16)
        self.assertEqual(catalog["counts"]["public_target_delta"], 0)
        commands = catalog["commands"]
        self.assertEqual([row["name"] for row in commands], sorted(row["name"] for row in commands))
        self.assertTrue(all(row["source"]["line"] > 0 for row in commands))
        self.assertTrue(all(row["acceptance_contracts"] for row in commands))
        by_name = {row["name"]: row for row in commands}
        self.assertEqual(by_name["cloud"]["canonical_name"], "vm")
        self.assertEqual(by_name["login"]["alias_expansion"], "login")
        self.assertEqual(by_name["browser-back"]["canonical_name"], "browser")
        self.assertEqual(by_name["capture-pane"]["canonical_name"], "read-screen")
        self.assertIn("open", by_name["browser"]["subcommands"])
        self.assertNotIn("cmux", by_name["workspace"]["subcommands"])
        self.assertEqual(by_name["feed"]["subcommands"], ["clear", "tui"])
        self.assertIn("list", by_name["sessions"]["subcommands"])
        self.assertEqual(by_name["settings"]["subcommands"], ["docs", "open", "path"])
        self.assertIn("--workspace", by_name["send"]["significant_flags"])
        self.assertEqual(by_name["__tmux-compat"]["visibility"], "hidden-or-internal")

    def test_generation_is_byte_identical_across_runs(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.json"
            second = Path(directory) / "second.json"
            for output in (first, second):
                subprocess.run(
                    [sys.executable, str(SCRIPT), "--repo", str(REPO), "--revision", REVISION, "--output", str(output)],
                    check=True,
                )
            self.assertEqual(first.read_bytes(), second.read_bytes())


if __name__ == "__main__":
    unittest.main()
