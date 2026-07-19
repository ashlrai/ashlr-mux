param(
    [Parameter(Mandatory = $true)]
    [int]$TargetProcessId,
    [Parameter(Mandatory = $true)]
    [int64]$TargetStartTimeUtcTicks,
    [Parameter(Mandatory = $true)]
    [string]$TargetExecutable,
    [Parameter(Mandatory = $true)]
    [string]$ViolationLogPath
)

$ErrorActionPreference = 'Stop'
$expectedExecutable = [System.IO.Path]::GetFullPath($TargetExecutable)

Add-Type -TypeDefinition @'
using System;
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
    $callback = [Cmux.CaptureWindowWatchdogNativeMethods+EnumWindowsCallback]{
        param([IntPtr]$window, [IntPtr]$parameter)
        [uint32]$ownerProcessId = 0
        [Cmux.CaptureWindowWatchdogNativeMethods]::GetWindowThreadProcessId(
            $window,
            [ref]$ownerProcessId
        ) | Out-Null
        if ($ownerProcessId -eq $TargetProcessId -and
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
        $rect = [Cmux.CaptureWindowWatchdogNativeMethods+Rect]::new()
        [Cmux.CaptureWindowWatchdogNativeMethods]::GetWindowRect($window, [ref]$rect) | Out-Null
        [ordered]@{
            process_id = $TargetProcessId
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
