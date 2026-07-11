import os
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify-test-manifest.ps1")


@unittest.skipUnless(os.name == "nt", "Windows PowerShell is only available on Windows")
class VerifyTestManifestTests(unittest.TestCase):
    def test_windows_powershell_does_not_take_non_windows_skip(self) -> None:
        missing_executable = Path(tempfile.gettempdir()) / "missing-cmux-test-harness.exe"
        result = subprocess.run(
            [
                "powershell.exe",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(SCRIPT),
                "-TestExecutable",
                str(missing_executable),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        output = result.stdout + result.stderr

        self.assertNotEqual(result.returncode, 0, output)
        self.assertNotIn("Skipping desktop test manifest verification", output)
        self.assertIn("test executable not found", output)


if __name__ == "__main__":
    unittest.main()
