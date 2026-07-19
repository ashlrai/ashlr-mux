import json
import os
import subprocess
import tempfile
import time
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
        app_binary: Path | None = None,
        startup_timeout_seconds: int | None = None,
    ) -> subprocess.CompletedProcess[str]:
        command = [
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
        ]
        if app_binary is not None:
            command.extend(["-AppBinary", str(app_binary)])
        if startup_timeout_seconds is not None:
            command.extend(["-StartupTimeoutSeconds", str(startup_timeout_seconds)])
        # The launcher intentionally leaves a GUI child alive after Start.
        # File-backed streams prevent that grandchild from holding Python's
        # capture pipes open after the PowerShell parent has returned.
        with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as stdout:
            with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as stderr:
                result = subprocess.run(
                    command,
                    stdout=stdout,
                    stderr=stderr,
                    text=True,
                    timeout=15,
                )
                stdout.seek(0)
                stderr.seek(0)
                result.stdout = stdout.read()
                result.stderr = stderr.read()
                return result

    def build_visible_window_fixture(self, directory: Path) -> Path:
        executable = directory / "visible-capture-fixture.exe"
        source = directory / "visible-capture-fixture.cs"
        source.write_text(
            """
using System;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Windows.Forms;

internal static class Program
{
    [DllImport("user32.dll")]
    private static extern bool ShowWindowAsync(IntPtr window, int command);

    [STAThread]
    private static void Main()
    {
        Application.EnableVisualStyles();
        using (var pipe = new NamedPipeServerStream(
            Environment.GetEnvironmentVariable("CMUX_CONTROL_PIPE_NAME"),
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous))
        {
        var form = new Form { Text = "cmux visible capture fixture" };
        var timer = new Timer { Interval = 50 };
        timer.Tick += (_, __) => ShowWindowAsync(form.Handle, 5);
        timer.Start();
        Application.Run(form);
        }
    }
}
""".strip(),
            encoding="utf-8",
        )
        result = subprocess.run(
            [
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                (
                    f"Add-Type -Path '{str(source).replace(chr(39), chr(39) * 2)}' "
                    "-ReferencedAssemblies System.Windows.Forms,System.Drawing "
                    f"-OutputAssembly '{str(executable).replace(chr(39), chr(39) * 2)}' "
                    "-OutputType WindowsApplication"
                ),
            ],
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(executable.is_file())
        return executable

    def build_delayed_visible_window_fixture(self, directory: Path) -> Path:
        executable = directory / "delayed-visible-capture-fixture.exe"
        source = directory / "delayed-visible-capture-fixture.cs"
        source.write_text(
            """
using System;
using System.IO.Pipes;
using System.Windows.Forms;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        Application.EnableVisualStyles();
        using (var pipe = new NamedPipeServerStream(
            Environment.GetEnvironmentVariable("CMUX_CONTROL_PIPE_NAME"),
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous))
        {
            var context = new ApplicationContext();
            var timer = new Timer { Interval = 1500 };
            timer.Tick += (_, __) =>
            {
                timer.Stop();
                var form = new Form { Text = "cmux delayed visible capture fixture" };
                form.FormClosed += (___, ____) => context.ExitThread();
                form.Show();
            };
            timer.Start();
            Application.Run(context);
        }
    }
}
""".strip(),
            encoding="utf-8",
        )
        result = subprocess.run(
            [
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                (
                    f"Add-Type -Path '{str(source).replace(chr(39), chr(39) * 2)}' "
                    "-ReferencedAssemblies System.Windows.Forms,System.Drawing "
                    f"-OutputAssembly '{str(executable).replace(chr(39), chr(39) * 2)}' "
                    "-OutputType WindowsApplication"
                ),
            ],
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(executable.is_file())
        return executable

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

    def test_start_hides_and_rejects_a_visible_capture_window(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            executable = self.build_visible_window_fixture(profile)

            try:
                result = self.run_script(
                    profile,
                    action="Start",
                    pipe_name="cmux-visible-capture-test",
                    app_binary=executable,
                    startup_timeout_seconds=3,
                )

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("exposed a visible window", result.stderr)
                self.assertFalse((profile / "cmux-capture-process.json").exists())
            finally:
                self.run_script(profile, pipe_name="cmux-visible-capture-test")

    def test_supervisor_rejects_a_window_exposed_after_startup(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            executable = self.build_delayed_visible_window_fixture(profile)
            pipe_name = "cmux-delayed-visible-capture-test"

            try:
                result = self.run_script(
                    profile,
                    action="Start",
                    pipe_name=pipe_name,
                    app_binary=executable,
                    startup_timeout_seconds=3,
                )
                self.assertEqual(result.returncode, 0, result.stderr)

                state = json.loads(
                    (profile / "cmux-capture-process.json").read_text(
                        encoding="utf-8-sig"
                    )
                )
                process_id = int(state["pid"])
                for _ in range(50):
                    probe = subprocess.run(
                        [
                            "powershell.exe",
                            "-NoProfile",
                            "-NonInteractive",
                            "-Command",
                            f"if (Get-Process -Id {process_id} -ErrorAction SilentlyContinue) {{ exit 1 }}",
                        ],
                        timeout=5,
                    )
                    if probe.returncode == 0:
                        break
                    time.sleep(0.1)

                self.assertEqual(
                    probe.returncode,
                    0,
                    "capture process remained alive after exposing a delayed window",
                )
            finally:
                self.run_script(profile, pipe_name=pipe_name)


if __name__ == "__main__":
    unittest.main()
