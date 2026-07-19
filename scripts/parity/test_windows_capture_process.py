import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("windows-capture-process.ps1")


@unittest.skipUnless(os.name == "nt", "Windows process supervisor")
class WindowsCaptureProcessTests(unittest.TestCase):
    def run_script(
        self,
        profile: Path,
        *,
        action: str = "Stop",
        pipe_name: str = "cmux-capture-test",
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(SCRIPT),
                "-Action",
                action,
                "-PipeName",
                pipe_name,
                "-ProfileRoot",
                str(profile),
            ],
            capture_output=True,
            text=True,
            timeout=15,
        )

    def test_rejects_namespace_separator_in_pipe_name(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_script(Path(directory), pipe_name=r"bad\pipe")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must not contain a backslash", result.stderr)

    def test_stop_removes_stale_owned_process_state(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            state_path = profile / "cmux-capture-process.json"
            state_path.write_text(
                json.dumps(
                    {
                        "pid": 2_147_483_647,
                        "start_time_utc_ticks": 0,
                        "executable": str(profile / "cmux-desktop.exe"),
                    }
                ),
                encoding="utf-8",
            )

            result = self.run_script(profile)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(state_path.exists())

    def test_stop_refuses_live_pid_with_wrong_process_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            state_path = profile / "cmux-capture-process.json"
            state_path.write_text(
                json.dumps(
                    {
                        "pid": os.getpid(),
                        "start_time_utc_ticks": 0,
                        "executable": str(profile / "not-this-process.exe"),
                    }
                ),
                encoding="utf-8",
            )

            result = self.run_script(profile)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing to stop it", result.stderr)
            self.assertTrue(state_path.exists())


if __name__ == "__main__":
    unittest.main()
