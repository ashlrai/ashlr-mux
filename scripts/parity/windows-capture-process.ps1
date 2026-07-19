param(
    [ValidateSet('Start', 'Restart', 'Stop')]
    [string]$Action = 'Restart',
    [Parameter(Mandatory = $true)]
    [string]$PipeName,
    [Parameter(Mandatory = $true)]
    [string]$ProfileRoot,
    [string]$AppBinary = 'target\debug\cmux-desktop.exe',
    [int]$StartupTimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($PipeName) -or $PipeName.Contains('\')) {
    throw 'PipeName must be non-empty and must not contain a backslash.'
}
$pipePath = "\\.\pipe\$PipeName"
if ($pipePath.Length -gt 256) {
    throw 'The full named-pipe path must not exceed 256 characters.'
}
if ($StartupTimeoutSeconds -lt 1) {
    throw 'StartupTimeoutSeconds must be positive.'
}

$profilePath = [System.IO.Path]::GetFullPath($ProfileRoot)
$profileRootPath = [System.IO.Path]::GetPathRoot($profilePath)
if ($profilePath.TrimEnd('\') -eq $profileRootPath.TrimEnd('\')) {
    throw 'ProfileRoot must not be a filesystem root.'
}
$statePath = Join-Path $profilePath 'cmux-capture-process.json'

function Read-OwnedProcessState {
    if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) {
        return $null
    }
    try {
        return Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    } catch {
        throw "Capture process state is unreadable: $statePath"
    }
}

function Get-OwnedProcess([object]$state) {
    if ($null -eq $state) {
        return $null
    }
    $process = Get-Process -Id ([int]$state.pid) -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        return $null
    }
    $actualPath = [System.IO.Path]::GetFullPath($process.Path)
    $expectedPath = [System.IO.Path]::GetFullPath([string]$state.executable)
    $actualStart = $process.StartTime.ToUniversalTime().Ticks
    if (-not $actualPath.Equals($expectedPath, [System.StringComparison]::OrdinalIgnoreCase) -or
        $actualStart -ne [int64]$state.start_time_utc_ticks) {
        throw "PID $($state.pid) no longer identifies the capture-owned process; refusing to stop it."
    }
    return $process
}

function Get-OwnedSupervisorProcess([object]$state) {
    if ($null -eq $state -or $null -eq $state.supervisor_pid) {
        return $null
    }
    $process = Get-Process -Id ([int]$state.supervisor_pid) -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        return $null
    }
    try {
        $actualPathValue = $process.Path
        $actualStartTime = $process.StartTime
        if ($null -eq $actualStartTime) {
            return $null
        }
        $actualStart = $actualStartTime.ToUniversalTime().Ticks
    } catch {
        return $null
    }
    if ([string]::IsNullOrWhiteSpace($actualPathValue)) {
        if ($process.HasExited) {
            return $null
        }
        throw "PID $($state.supervisor_pid) no longer identifies the capture supervisor; refusing to stop it."
    }
    $actualPath = [System.IO.Path]::GetFullPath($actualPathValue)
    $expectedPath = [System.IO.Path]::GetFullPath([string]$state.supervisor_executable)
    if (-not $actualPath.Equals($expectedPath, [System.StringComparison]::OrdinalIgnoreCase) -or
        $actualStart -ne [int64]$state.supervisor_start_time_utc_ticks) {
        throw "PID $($state.supervisor_pid) no longer identifies the capture supervisor; refusing to stop it."
    }
    return $process
}

function Get-OwnedProcessTree([System.Diagnostics.Process]$rootProcess) {
    $snapshotTakenAtUtcTicks = [DateTime]::UtcNow.Ticks
    $rootStartTimeUtcTicks = $rootProcess.StartTime.ToUniversalTime().Ticks
    $processSnapshot = Get-CimInstance Win32_Process
    $ownedIds = [System.Collections.Generic.HashSet[int]]::new()
    $ownedIds.Add($rootProcess.Id) | Out-Null

    $added = $true
    while ($added) {
        $added = $false
        foreach ($candidate in $processSnapshot) {
            if (-not $ownedIds.Contains([int]$candidate.ProcessId) -and
                $ownedIds.Contains([int]$candidate.ParentProcessId)) {
                $ownedIds.Add([int]$candidate.ProcessId) | Out-Null
                $added = $true
            }
        }
    }

    $ownedProcesses = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
    $ownedProcesses.Add($rootProcess)
    foreach ($processId in $ownedIds) {
        if ($processId -eq $rootProcess.Id) {
            continue
        }
        $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
        if ($null -eq $process) {
            continue
        }
        try {
            $startTime = $process.StartTime
            if ($null -eq $startTime) {
                continue
            }
            $startTimeUtcTicks = $startTime.ToUniversalTime().Ticks
        } catch {
            continue
        }
        if ($startTimeUtcTicks -ge $rootStartTimeUtcTicks -and
            $startTimeUtcTicks -le $snapshotTakenAtUtcTicks) {
            $ownedProcesses.Add($process)
        }
    }
    return $ownedProcesses.ToArray()
}

function Stop-OwnedProcess {
    $state = Read-OwnedProcessState
    $process = Get-OwnedProcess $state
    if ($null -ne $process) {
        $ownedProcesses = @(Get-OwnedProcessTree $process)
        $ownedProcesses | Stop-Process -Force -ErrorAction SilentlyContinue
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        while ([DateTime]::UtcNow -lt $deadline) {
            $runningProcesses = @($ownedProcesses | Where-Object { -not $_.HasExited })
            if ($runningProcesses.Count -eq 0) {
                break
            }
            Start-Sleep -Milliseconds 100
            $runningProcesses | ForEach-Object { $_.Refresh() }
        }
        $runningProcesses = @($ownedProcesses | Where-Object { -not $_.HasExited })
        if ($runningProcesses.Count -gt 0) {
            $runningIds = ($runningProcesses | ForEach-Object { $_.Id }) -join ', '
            throw "Capture-owned process tree PIDs $runningIds did not exit within 20 seconds."
        }
    }
    $supervisor = Get-OwnedSupervisorProcess $state
    if ($null -ne $supervisor) {
        Stop-Process -Id $supervisor.Id -Force
        $supervisor.WaitForExit(5000) | Out-Null
    }
    if (Test-Path -LiteralPath $statePath -PathType Leaf) {
        Remove-Item -LiteralPath $statePath
    }
}

function Start-WindowSupervisor([System.Diagnostics.Process]$process, [hashtable]$state) {
    $watchdogPath = Join-Path $PSScriptRoot 'windows-capture-window-watchdog.ps1'
    $powershellPath = (Get-Command powershell.exe -ErrorAction Stop).Source
    $watchdogStdoutPath = Join-Path $profilePath 'cmux-capture-window-watchdog.stdout.log'
    $watchdogStderrPath = Join-Path $profilePath 'cmux-capture-window-watchdog.stderr.log'
    $violationLogPath = Join-Path $profilePath 'cmux-capture-window-violation.json'
    $readyPath = Join-Path $profilePath 'cmux-capture-window-watchdog.ready'
    foreach ($staleArtifact in @($violationLogPath, $readyPath)) {
        if (Test-Path -LiteralPath $staleArtifact -PathType Leaf) {
            Remove-Item -LiteralPath $staleArtifact
        }
    }
    $arguments = @(
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        "`"$watchdogPath`"",
        '-TargetProcessId',
        [string]$process.Id,
        '-TargetStartTimeUtcTicks',
        [string]$process.StartTime.ToUniversalTime().Ticks,
        '-TargetExecutable',
        "`"$($process.Path)`"",
        '-ViolationLogPath',
        "`"$violationLogPath`"",
        '-ReadyPath',
        "`"$readyPath`""
    )
    $supervisor = Start-Process -FilePath $powershellPath -ArgumentList $arguments -PassThru `
        -WindowStyle Hidden -RedirectStandardOutput $watchdogStdoutPath `
        -RedirectStandardError $watchdogStderrPath
    $supervisor.Refresh()
    $state.supervisor_pid = $supervisor.Id
    $state.supervisor_start_time_utc_ticks = $supervisor.StartTime.ToUniversalTime().Ticks
    $state.supervisor_executable = $powershellPath
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath -Encoding UTF8

    $readyDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while ([DateTime]::UtcNow -lt $readyDeadline) {
        $supervisor.Refresh()
        if ($supervisor.HasExited) {
            Stop-OwnedProcess
            throw 'Capture window watchdog exited before it became ready.'
        }
        if (Test-Path -LiteralPath $readyPath -PathType Leaf) {
            Remove-Item -LiteralPath $readyPath
            return
        }
        Start-Sleep -Milliseconds 25
    }
    Stop-OwnedProcess
    throw 'Capture window watchdog was not ready within 10 seconds.'
}

function Test-PipeReady {
    try {
        return [System.IO.Directory]::GetFiles('\\.\pipe\') -contains $pipePath
    } catch {
        return $false
    }
}

function Start-OwnedProcess {
    $existingState = Read-OwnedProcessState
    $existingProcess = Get-OwnedProcess $existingState
    if ($null -ne $existingProcess) {
        throw "Capture-owned cmux-desktop PID $($existingProcess.Id) is already running."
    }
    if ($null -ne $existingState) {
        Remove-Item -LiteralPath $statePath
    }
    if (Test-PipeReady) {
        throw "Named pipe $pipePath is already in use; choose a task-unique PipeName."
    }

    $executable = (Resolve-Path -LiteralPath $AppBinary -ErrorAction Stop).Path
    New-Item -ItemType Directory -Force -Path $profilePath | Out-Null
    $homePath = Join-Path $profilePath 'home'
    $localAppData = Join-Path $homePath 'AppData\Local'
    $roamingAppData = Join-Path $homePath 'AppData\Roaming'
    foreach ($directory in @($homePath, $localAppData, $roamingAppData)) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }

    $env:CMUX_CONTROL_PIPE_NAME = $PipeName
    $env:CMUX_TEST_DISABLE_SINGLE_INSTANCE = '1'
    $env:CMUX_PARITY_CAPTURE_HEADLESS = '1'
    $env:CMUX_SOCKET_PASSWORD = $null
    $env:LOCALAPPDATA = $localAppData
    $env:APPDATA = $roamingAppData
    $env:USERPROFILE = $homePath
    $env:HOME = $homePath
    $env:RUST_BACKTRACE = '1'

    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
    $stdoutPath = Join-Path $profilePath "cmux-desktop.$stamp.stdout.log"
    $stderrPath = Join-Path $profilePath "cmux-desktop.$stamp.stderr.log"
    $process = Start-Process -FilePath $executable -WorkingDirectory $profilePath -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    $process.Refresh()
    $state = [ordered]@{
        pid = $process.Id
        start_time_utc_ticks = $process.StartTime.ToUniversalTime().Ticks
        executable = $executable
        pipe = $pipePath
        profile_root = $profilePath
    }
    $state | ConvertTo-Json | Set-Content -LiteralPath $statePath -Encoding UTF8
    Start-WindowSupervisor $process $state

    $deadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) {
            $process.WaitForExit()
            $visibleWindowWasRejected = Test-Path -LiteralPath (
                Join-Path $profilePath 'cmux-capture-window-violation.json'
            ) -PathType Leaf
            Stop-OwnedProcess
            if ($visibleWindowWasRejected) {
                throw "Capture-owned process PID $($process.Id) exposed a visible window; it was hidden and stopped."
            }
            throw "Capture-owned cmux-desktop exited with code $($process.ExitCode) before $pipePath became ready."
        }
        if (Test-PipeReady) {
            $stabilityDeadline = [DateTime]::UtcNow.AddMilliseconds(750)
            while ([DateTime]::UtcNow -lt $stabilityDeadline) {
                $process.Refresh()
                if ($process.HasExited) {
                    $process.WaitForExit()
                    $visibleWindowWasRejected = Test-Path -LiteralPath (
                        Join-Path $profilePath 'cmux-capture-window-violation.json'
                    ) -PathType Leaf
                    Stop-OwnedProcess
                    if ($visibleWindowWasRejected) {
                        throw "Capture-owned process PID $($process.Id) exposed a visible window; it was hidden and stopped."
                    }
                    throw "Capture-owned cmux-desktop exited with code $($process.ExitCode) after $pipePath became ready."
                }
                Start-Sleep -Milliseconds 50
            }
            return
        }
        Start-Sleep -Milliseconds 50
    }
    Stop-OwnedProcess
    throw "Named pipe $pipePath was not ready within $StartupTimeoutSeconds seconds."
}

switch ($Action) {
    'Start' { Start-OwnedProcess }
    'Restart' {
        Stop-OwnedProcess
        Start-OwnedProcess
    }
    'Stop' { Stop-OwnedProcess }
}
