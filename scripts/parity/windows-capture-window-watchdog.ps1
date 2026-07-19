param(
    [Parameter(Mandatory = $true)]
    [int]$TargetProcessId,
    [Parameter(Mandatory = $true)]
    [int64]$TargetStartTimeUtcTicks,
    [Parameter(Mandatory = $true)]
    [string]$TargetExecutable
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
        public static extern bool ShowWindowAsync(IntPtr window, int command);
    }
}
'@

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

function Hide-VisibleTargetWindows {
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
            $visibleWindows.Add($window)
        }
        return $true
    }
    [Cmux.CaptureWindowWatchdogNativeMethods]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    foreach ($window in $visibleWindows) {
        [Cmux.CaptureWindowWatchdogNativeMethods]::ShowWindowAsync($window, 0) | Out-Null
    }
    return $visibleWindows.Count
}

while ($null -ne (Get-ExactTargetProcess)) {
    if ((Hide-VisibleTargetWindows) -gt 0) {
        $process = Get-ExactTargetProcess
        if ($null -ne $process) {
            Stop-Process -Id $process.Id -Force
        }
        exit 42
    }
    Start-Sleep -Milliseconds 25
}
