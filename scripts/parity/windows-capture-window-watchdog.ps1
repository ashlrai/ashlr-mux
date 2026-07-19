param(
    [Parameter(Mandatory = $true)]
    [int]$TargetProcessId,
    [Parameter(Mandatory = $true)]
    [int64]$TargetStartTimeUtcTicks,
    [Parameter(Mandatory = $true)]
    [string]$TargetExecutable,
    [Parameter(Mandatory = $true)]
    [string]$ViolationLogPath,
    [Parameter(Mandatory = $true)]
    [string]$ReadyPath
)

$ErrorActionPreference = 'Stop'
$expectedExecutable = [System.IO.Path]::GetFullPath($TargetExecutable)

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace Cmux
{
    public static class CaptureWindowWatchdogNativeMethods
    {
        [StructLayout(LayoutKind.Sequential)]
        public struct Rect
        {
            public int Left;
            public int Top;
            public int Right;
            public int Bottom;
        }

        public delegate bool EnumWindowsCallback(IntPtr window, IntPtr parameter);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr parameter);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool IsWindowVisible(IntPtr window);

        [DllImport("user32.dll")]
        public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetWindowRect(IntPtr window, out Rect rect);

        [DllImport("user32.dll")]
        public static extern int GetSystemMetrics(int index);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool ShowWindowAsync(IntPtr window, int command);

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
        public struct ProcessEntry32
        {
            public uint Size;
            public uint Usage;
            public uint ProcessId;
            public IntPtr DefaultHeapId;
            public uint ModuleId;
            public uint Threads;
            public uint ParentProcessId;
            public int BasePriority;
            public uint Flags;

            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
            public string ExecutableFile;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint processId);

        [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool Process32First(IntPtr snapshot, ref ProcessEntry32 entry);

        [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool Process32Next(IntPtr snapshot, ref ProcessEntry32 entry);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);

        public static uint[] GetProcessTree(uint rootProcessId)
        {
            const uint SnapshotProcesses = 0x00000002;
            var snapshot = CreateToolhelp32Snapshot(SnapshotProcesses, 0);
            var processIds = new HashSet<uint> { rootProcessId };
            if (snapshot == new IntPtr(-1))
            {
                return new List<uint>(processIds).ToArray();
            }

            var parentByProcess = new Dictionary<uint, uint>();
            try
            {
                var entry = new ProcessEntry32
                {
                    Size = (uint)Marshal.SizeOf(typeof(ProcessEntry32))
                };
                if (Process32First(snapshot, ref entry))
                {
                    do
                    {
                        parentByProcess[entry.ProcessId] = entry.ParentProcessId;
                        entry.Size = (uint)Marshal.SizeOf(typeof(ProcessEntry32));
                    }
                    while (Process32Next(snapshot, ref entry));
                }
            }
            finally
            {
                CloseHandle(snapshot);
            }

            var added = true;
            while (added)
            {
                added = false;
                foreach (var process in parentByProcess)
                {
                    if (!processIds.Contains(process.Key) && processIds.Contains(process.Value))
                    {
                        processIds.Add(process.Key);
                        added = true;
                    }
                }
            }
            return new List<uint>(processIds).ToArray();
        }
    }
}
'@

$virtualLeft = [Cmux.CaptureWindowWatchdogNativeMethods]::GetSystemMetrics(76)
$virtualTop = [Cmux.CaptureWindowWatchdogNativeMethods]::GetSystemMetrics(77)
$virtualRight = $virtualLeft + [Cmux.CaptureWindowWatchdogNativeMethods]::GetSystemMetrics(78)
$virtualBottom = $virtualTop + [Cmux.CaptureWindowWatchdogNativeMethods]::GetSystemMetrics(79)

function Get-ExactTargetProcess {
    $process = Get-Process -Id $TargetProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        return $null
    }
    $actualExecutable = [System.IO.Path]::GetFullPath($process.Path)
    $actualStart = $process.StartTime.ToUniversalTime().Ticks
    if (-not $actualExecutable.Equals($expectedExecutable, [System.StringComparison]::OrdinalIgnoreCase) -or
        $actualStart -ne $TargetStartTimeUtcTicks) {
        return $null
    }
    return $process
}

function Hide-VisibleOnscreenTargetWindows {
    $visibleWindows = [System.Collections.Generic.List[System.IntPtr]]::new()
    $ownedProcessIds = [Cmux.CaptureWindowWatchdogNativeMethods]::GetProcessTree(
        [uint32]$TargetProcessId
    )
    $callback = [Cmux.CaptureWindowWatchdogNativeMethods+EnumWindowsCallback]{
        param([IntPtr]$window, [IntPtr]$parameter)
        [uint32]$ownerProcessId = 0
        [Cmux.CaptureWindowWatchdogNativeMethods]::GetWindowThreadProcessId(
            $window,
            [ref]$ownerProcessId
        ) | Out-Null
        if ($ownedProcessIds -contains $ownerProcessId -and
            [Cmux.CaptureWindowWatchdogNativeMethods]::IsWindowVisible($window)) {
            $rect = [Cmux.CaptureWindowWatchdogNativeMethods+Rect]::new()
            $hasRect = [Cmux.CaptureWindowWatchdogNativeMethods]::GetWindowRect(
                $window,
                [ref]$rect
            )
            $intersectsDesktop = -not $hasRect -or (
                $rect.Right -gt $virtualLeft -and
                $rect.Left -lt $virtualRight -and
                $rect.Bottom -gt $virtualTop -and
                $rect.Top -lt $virtualBottom
            )
            $canExposePageContent = -not $hasRect -or (
                ($rect.Right - $rect.Left) -ge 64 -and
                ($rect.Bottom - $rect.Top) -ge 64
            )
            if ($intersectsDesktop -and $canExposePageContent) {
                $visibleWindows.Add($window)
            }
        }
        return $true
    }
    [Cmux.CaptureWindowWatchdogNativeMethods]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    foreach ($window in $visibleWindows) {
        [uint32]$ownerProcessId = 0
        [Cmux.CaptureWindowWatchdogNativeMethods]::GetWindowThreadProcessId(
            $window,
            [ref]$ownerProcessId
        ) | Out-Null
        $rect = [Cmux.CaptureWindowWatchdogNativeMethods+Rect]::new()
        [Cmux.CaptureWindowWatchdogNativeMethods]::GetWindowRect($window, [ref]$rect) | Out-Null
        [ordered]@{
            process_id = $TargetProcessId
            window_process_id = $ownerProcessId
            window_handle = $window.ToInt64()
            left = $rect.Left
            top = $rect.Top
            right = $rect.Right
            bottom = $rect.Bottom
            detected_at_utc = [DateTime]::UtcNow.ToString('O')
        } | ConvertTo-Json | Set-Content -LiteralPath $ViolationLogPath -Encoding UTF8
        [Cmux.CaptureWindowWatchdogNativeMethods]::ShowWindowAsync($window, 0) | Out-Null
    }
    return $visibleWindows.Count
}

[DateTime]::UtcNow.ToString('O') | Set-Content -LiteralPath $ReadyPath -Encoding ASCII
while ($null -ne (Get-ExactTargetProcess)) {
    if ((Hide-VisibleOnscreenTargetWindows) -gt 0) {
        $process = Get-ExactTargetProcess
        if ($null -ne $process) {
            Stop-Process -Id $process.Id -Force
        }
        exit 42
    }
    Start-Sleep -Milliseconds 25
}
