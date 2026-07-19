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

function Stop-OwnedProcess {
    $state = Read-OwnedProcessState
    $process = Get-OwnedProcess $state
    if ($null -ne $process) {
        Stop-Process -Id $process.Id -Force
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        while (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
            Start-Sleep -Milliseconds 100
            $process.Refresh()
        }
        if (-not $process.HasExited) {
            throw "Capture-owned cmux-desktop PID $($process.Id) did not exit within 20 seconds."
        }
    }
    if (Test-Path -LiteralPath $statePath -PathType Leaf) {
        Remove-Item -LiteralPath $statePath
    }
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
    $env:CMUX_SOCKET_PASSWORD = $null
    $env:LOCALAPPDATA = $localAppData
    $env:APPDATA = $roamingAppData
    $env:USERPROFILE = $homePath
    $env:HOME = $homePath
    $env:RUST_BACKTRACE = '1'

    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ')
    $stdoutPath = Join-Path $profilePath "cmux-desktop.$stamp.stdout.log"
    $stderrPath = Join-Path $profilePath "cmux-desktop.$stamp.stderr.log"
    $process = Start-Process -FilePath $executable -WorkingDirectory $profilePath -PassThru `
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

    $deadline = [DateTime]::UtcNow.AddSeconds($StartupTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) {
            $process.WaitForExit()
            Remove-Item -LiteralPath $statePath -ErrorAction SilentlyContinue
            throw "Capture-owned cmux-desktop exited with code $($process.ExitCode) before $pipePath became ready."
        }
        if (Test-PipeReady) {
            return
        }
        Start-Sleep -Milliseconds 250
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
