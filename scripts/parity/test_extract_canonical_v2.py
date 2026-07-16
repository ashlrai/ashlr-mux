import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
EXTRACTOR = ROOT / "scripts" / "parity" / "extract_canonical_v2.py"

MOBILE_RPC_RELEASE_METHODS = {
    "dogfood.feedback.submit",
    "mobile.terminal.mouse",
    "mobile.terminal.paste_image",
    "mobile.terminal.scroll",
    "notification.reconcile",
    "terminal.mouse",
    "terminal.paste_image",
    "terminal.scroll",
    "workspace.group.action",
    "workspace.move",
}


class CanonicalV2ExtractorTests(unittest.TestCase):
    def test_release_inventory_includes_unadvertised_mobile_rpc_methods(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "canonical_v2.json"
            subprocess.run(
                [sys.executable, str(EXTRACTOR), "--output", str(output)],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )
            document = json.loads(output.read_text(encoding="utf-8"))

        methods = {entry["method"]: entry for entry in document["methods"]}
        self.assertEqual(MOBILE_RPC_RELEASE_METHODS - methods.keys(), set())
        self.assertEqual(document["counts"]["release"], 261)
        self.assertEqual(document["counts"]["advertised_release"], 251)
        self.assertEqual(document["counts"]["unadvertised_release"], 10)
        self.assertEqual(document["counts"]["debug_only"], 42)
        self.assertEqual(document["counts"]["all_release_debug_build"], 303)
        self.assertEqual(len(methods), 303)
        for method in MOBILE_RPC_RELEASE_METHODS:
            self.assertEqual(methods[method]["availability"], "release")
            self.assertEqual(
                methods[method]["source_location"]["symbol"],
                "mobileHostHandleRPC",
            )

    def test_authorization_policy_is_not_dispatch_or_implementation_authority(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "canonical_v2.json"
            subprocess.run(
                [sys.executable, str(EXTRACTOR), "--output", str(output)],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )
            document = json.loads(output.read_text(encoding="utf-8"))

        methods = {entry["method"]: entry for entry in document["methods"]}
        for method in MOBILE_RPC_RELEASE_METHODS:
            for field in ("dispatch_locations", "implementation_locations"):
                symbols = {
                    location.get("symbol") for location in methods[method][field]
                }
                self.assertNotIn(
                    "ticketAuthorizationError",
                    symbols,
                    f"{method} falsely treats authorization policy as {field}",
                )


if __name__ == "__main__":
    unittest.main()
