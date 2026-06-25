[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AppPath,
    [int]$TimeoutSeconds = 20
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $AppPath)) {
    throw "App bundle entrypoint does not exist: $AppPath"
}

$process = Start-Process -FilePath $AppPath -PassThru
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if ($process.HasExited) {
            throw "Desktop smoke launch failed because the app exited early."
        }
    } until ($process.MainWindowHandle -ne 0 -or (Get-Date) -ge $deadline)

    if ($process.MainWindowHandle -eq 0) {
        throw "Desktop smoke launch timed out waiting for a visible window."
    }

    Write-Host "PASS: desktop bootstrap launched (PID=$($process.Id))"
}
finally {
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
    }
}

