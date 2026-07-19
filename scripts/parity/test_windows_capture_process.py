import ctypes
import json
import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("windows-capture-process.ps1")


def windows_process_exists(process_id: int) -> bool:
    process = ctypes.windll.kernel32.OpenProcess(0x00100000, False, process_id)
    if not process:
        return False
    ctypes.windll.kernel32.CloseHandle(process)
    return True


def terminate_windows_process(process_id: int) -> None:
    process = ctypes.windll.kernel32.OpenProcess(0x0001, False, process_id)
    if not process:
        return
    try:
        ctypes.windll.kernel32.TerminateProcess(process, 1)
        ctypes.windll.kernel32.WaitForSingleObject(process, 5_000)
    finally:
        ctypes.windll.kernel32.CloseHandle(process)


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

    def build_visible_window_fixture(
        self,
        directory: Path,
        *,
        show_delay_ms: int = 50,
        offscreen: bool = False,
        window_size_px: int | None = None,
    ) -> Path:
        placement = "offscreen" if offscreen else "onscreen"
        size = window_size_px or "default"
        stem = f"visible-capture-fixture-{show_delay_ms}-{placement}-{size}"
        executable = directory / f"{stem}.exe"
        source = directory / f"{stem}.cs"
        source.write_text(
            """
using System;
using System.Drawing;
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
            var form = new Form
            {
                Text = "cmux visible capture fixture",
                Opacity = 0.0,
                __FORM_PLACEMENT__
                __FORM_SIZE__
            };
            var timer = new Timer { Interval = __SHOW_DELAY_MS__ };
            timer.Tick += (_, __) =>
            {
                timer.Stop();
                ShowWindowAsync(form.Handle, 5);
            };
            timer.Start();
            Application.Run(form);
        }
    }
}
"""
            .replace("__SHOW_DELAY_MS__", str(show_delay_ms))
            .replace(
                "__FORM_PLACEMENT__",
                (
                    "StartPosition = FormStartPosition.Manual,\n"
                    "                Location = new Point(-32000, -32000),"
                    if offscreen
                    else "StartPosition = FormStartPosition.CenterScreen,"
                ),
            )
            .replace(
                "__FORM_SIZE__",
                (
                    "FormBorderStyle = FormBorderStyle.None,\n"
                    f"                Width = {window_size_px},\n"
                    f"                Height = {window_size_px},"
                    if window_size_px is not None
                    else ""
                ),
            )
            .strip(),
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

    def build_descendant_window_fixture(self, directory: Path) -> Path:
        executable = directory / "descendant-window-capture-fixture.exe"
        source = directory / "descendant-window-capture-fixture.cs"
        source.write_text(
            r"""
using System;
using System.Diagnostics;
using System.Drawing;
using System.IO.Pipes;
using System.Windows.Forms;

internal static class Program
{
    [STAThread]
    private static void Main(string[] args)
    {
        Application.EnableVisualStyles();
        if (args.Length == 2 && args[0] == "--child")
        {
            var parent = Process.GetProcessById(int.Parse(args[1]));
            var form = new Form
            {
                Text = "cmux descendant capture fixture",
                Opacity = 0.0,
                StartPosition = FormStartPosition.CenterScreen,
                Width = 640,
                Height = 480,
            };
            var context = new ApplicationContext();
            var showTimer = new Timer { Interval = 1500 };
            showTimer.Tick += (_, __) =>
            {
                showTimer.Stop();
                form.Show();
            };
            var parentMonitor = new Timer { Interval = 100 };
            parentMonitor.Tick += (_, __) =>
            {
                if (parent.HasExited)
                {
                    context.ExitThread();
                }
            };
            showTimer.Start();
            parentMonitor.Start();
            Application.Run(context);
            return;
        }

        using (var pipe = new NamedPipeServerStream(
            Environment.GetEnvironmentVariable("CMUX_CONTROL_PIPE_NAME"),
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous))
        {
            Process.Start(Application.ExecutablePath, "--child " + Process.GetCurrentProcess().Id);
            Application.Run(new ApplicationContext());
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

    def build_persistent_descendant_fixture(self, directory: Path) -> Path:
        executable = directory / "persistent-descendant-capture-fixture.exe"
        source = directory / "persistent-descendant-capture-fixture.cs"
        source.write_text(
            r"""
using System;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using System.Threading;

internal static class Program
{
    private static void Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "--child")
        {
            File.WriteAllText(
                Path.Combine(Environment.CurrentDirectory, "persistent-descendant.pid"),
                Process.GetCurrentProcess().Id.ToString());
            Thread.Sleep(TimeSpan.FromSeconds(30));
            return;
        }

        using (var pipe = new NamedPipeServerStream(
            Environment.GetEnvironmentVariable("CMUX_CONTROL_PIPE_NAME"),
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous))
        {
            Process.Start(Process.GetCurrentProcess().MainModule.FileName, "--child");
            Thread.Sleep(Timeout.Infinite);
        }
    }
}
"""
            .strip(),
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
                    f"-OutputAssembly '{str(executable).replace(chr(39), chr(39) * 2)}' "
                    "-OutputType ConsoleApplication"
                ),
            ],
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(executable.is_file())
        return executable

    def assert_supervisor_preserves_window(
        self, pipe_name: str, **fixture_options: object
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            executable = self.build_visible_window_fixture(
                profile, show_delay_ms=1500, **fixture_options
            )
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
                time.sleep(2.0)
                self.assertTrue(windows_process_exists(int(state["pid"])))
            finally:
                self.run_script(profile, pipe_name=pipe_name)

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
            executable = self.build_visible_window_fixture(
                profile, show_delay_ms=1500
            )
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
                    if not windows_process_exists(process_id):
                        break
                    time.sleep(0.1)

                self.assertFalse(
                    windows_process_exists(process_id),
                    "capture process remained alive after exposing a delayed window",
                )
            finally:
                self.run_script(profile, pipe_name=pipe_name)

    def test_supervisor_rejects_a_window_owned_by_a_descendant_process(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            executable = self.build_descendant_window_fixture(profile)
            pipe_name = "cmux-descendant-window-capture-test"

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
                    if not windows_process_exists(process_id):
                        break
                    time.sleep(0.1)

                self.assertFalse(
                    windows_process_exists(process_id),
                    "capture process remained alive after its child exposed a window",
                )
            finally:
                self.run_script(profile, pipe_name=pipe_name)
                time.sleep(0.5)

    def test_stop_terminates_a_persistent_owned_descendant(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            executable = self.build_persistent_descendant_fixture(profile)
            pipe_name = "cmux-persistent-descendant-capture-test"
            child_pid = None

            try:
                start = self.run_script(
                    profile,
                    action="Start",
                    pipe_name=pipe_name,
                    app_binary=executable,
                    startup_timeout_seconds=3,
                )
                self.assertEqual(start.returncode, 0, start.stderr)
                child_pid_path = profile / "persistent-descendant.pid"
                for _ in range(30):
                    if child_pid_path.is_file():
                        break
                    time.sleep(0.1)
                self.assertTrue(child_pid_path.is_file())
                child_pid = int(child_pid_path.read_text(encoding="utf-8"))
                self.assertTrue(windows_process_exists(child_pid))

                stop = self.run_script(profile, pipe_name=pipe_name)

                self.assertEqual(stop.returncode, 0, stop.stderr)
                self.assertFalse(
                    windows_process_exists(child_pid),
                    "capture Stop left an owned descendant running",
                )
            finally:
                self.run_script(profile, pipe_name=pipe_name)
                if child_pid is not None:
                    terminate_windows_process(child_pid)

    def test_supervisor_allows_rendering_entirely_offscreen(self):
        self.assert_supervisor_preserves_window(
            "cmux-offscreen-capture-test", offscreen=True
        )

    def test_supervisor_ignores_tiny_framework_helper_windows(self):
        self.assert_supervisor_preserves_window(
            "cmux-tiny-window-capture-test", window_size_px=14
        )


if __name__ == "__main__":
    unittest.main()
