# PowerShell port of scripts/dev-local.sh. Loads dev env, claims a per-worktree
# PID lock, brings up the local Postgres + migrations, runs `next dev`, and tears
# everything down on exit. The bash original relies on POSIX trap/pkill/parent
# death; on Windows we approximate process-tree teardown with taskkill /T (see
# Stop-NextTree). Job-Object-based teardown (KILL_ON_JOB_CLOSE) is DEFERRED to
# coordinate with the M3 supervisor helper.
$ErrorActionPreference = "Stop"

$ROOT_DIR = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path

# Dot-source so the secret/env exports land in this process.
. (Join-Path $ROOT_DIR "scripts/load-dev-env.ps1")
if ([string]::IsNullOrEmpty($env:CMUX_PORT)) {
    [Console]::Error.WriteLine("cmux web dev: failed to load dev env (see above)")
    exit 1
}

$PwshExe = (Get-Process -Id $PID).Path
$DbLocalScript = Join-Path $ROOT_DIR "scripts/db-local.ps1"
$DrizzleConfig = Join-Path $ROOT_DIR "drizzle.config.ts"

$next_proc = $null
$started_db = 0
$cleanup_watcher_job = $null
$db_watchdog_job = $null
$dev_lock_file = ""

function Get-DevLockKey {
    $branch = git -C (Join-Path $ROOT_DIR "..") branch --show-current 2>$null
    if ($LASTEXITCODE -ne 0) { $branch = "" }
    $branch = "$branch".Trim()
    if ([string]::IsNullOrEmpty($branch)) {
        $branch = Split-Path -Leaf (Resolve-Path -LiteralPath (Join-Path $ROOT_DIR "..")).Path
    }
    $slug = $branch.ToLowerInvariant()
    $slug = $slug -creplace '[^a-z0-9]+', '-'
    $slug = $slug -creplace '^-+', ''
    $slug = $slug -creplace '-+$', ''
    $slug = $slug -creplace '-+', '-'
    if ($slug.Length -gt 48) { $slug = $slug.Substring(0, 48) }
    if ([string]::IsNullOrEmpty($slug)) { $slug = "worktree" }
    return "$slug-dev-$($env:CMUX_PORT)"
}

function Set-DevLock {
    # ${TMPDIR:-/tmp}/cmux-web-dev -> %LOCALAPPDATA%\cmux\web-dev on Windows.
    $lock_dir = Join-Path $env:LOCALAPPDATA "cmux\web-dev"
    New-Item -ItemType Directory -Force -Path $lock_dir | Out-Null
    $script:dev_lock_file = Join-Path $lock_dir ((Get-DevLockKey) + ".pid")
    Set-Content -LiteralPath $script:dev_lock_file -Value $PID -Encoding ascii
}

function Test-OwnsDevLock {
    if ([string]::IsNullOrEmpty($script:dev_lock_file)) { return $false }
    if (-not (Test-Path -LiteralPath $script:dev_lock_file)) { return $false }
    $content = (Get-Content -LiteralPath $script:dev_lock_file -ErrorAction SilentlyContinue | Select-Object -First 1)
    return ("$content".Trim() -eq "$PID")
}

function Invoke-DbLocal {
    param([Parameter(Mandatory = $true)][string]$DbCommand, [switch]$Quiet)
    if ($Quiet) {
        & $PwshExe -NoProfile -File $DbLocalScript $DbCommand *> $null
    } else {
        & $PwshExe -NoProfile -File $DbLocalScript $DbCommand
    }
    return $LASTEXITCODE
}

function Stop-LocalServices {
    if (-not (Test-OwnsDevLock)) {
        Write-Output "cmux web dev: skipped local service stop because another dev process owns CMUX_PORT=$($env:CMUX_PORT)"
        return
    }
    try { Invoke-DbLocal -DbCommand "down" -Quiet | Out-Null } catch { }
    Write-Output "cmux web dev: stopped local Postgres for CMUX_PORT=$($env:CMUX_PORT)"
}

# The watcher/watchdog scriptblocks run in background jobs (child pwsh that
# inherits this process' environment). They re-derive nothing beyond what is
# passed in; owns-lock is checked against the lock file content == parent PID.
$OwnsLockProbe = {
    param($lockFile, $parentPid)
    if ([string]::IsNullOrEmpty($lockFile) -or -not (Test-Path -LiteralPath $lockFile)) { return $false }
    $c = (Get-Content -LiteralPath $lockFile -ErrorAction SilentlyContinue | Select-Object -First 1)
    return ("$c".Trim() -eq "$parentPid")
}

function Start-CleanupWatcher {
    if ($env:CMUX_DEV_STOP_DB_ON_EXIT -eq "0") { return }
    $parent_pid = $PID
    $script:cleanup_watcher_job = Start-Job -ScriptBlock {
        param($parentPid, $lockFile, $pwshExe, $dbScript, $ownsProbe)
        $probe = [scriptblock]::Create($ownsProbe)
        while ($true) {
            try { $null = Get-Process -Id $parentPid -ErrorAction Stop } catch { break }
            Start-Sleep -Seconds 1
        }
        if (& $probe $lockFile $parentPid) {
            & $pwshExe -NoProfile -File $dbScript down *> $null
        }
    } -ArgumentList $parent_pid, $script:dev_lock_file, $PwshExe, $DbLocalScript, $OwnsLockProbe.ToString()
}

function Start-DbWatchdog {
    if ($env:CMUX_DEV_WATCH_DB -eq "0") { return }
    $parent_pid = $PID
    $script:db_watchdog_job = Start-Job -ScriptBlock {
        param($parentPid, $lockFile, $pwshExe, $dbScript, $drizzleConfig, $cmuxPort, $ownsProbe)
        $probe = [scriptblock]::Create($ownsProbe)
        while ($true) {
            try { $null = Get-Process -Id $parentPid -ErrorAction Stop } catch { break }
            if (& $probe $lockFile $parentPid) {
                & $pwshExe -NoProfile -File $dbScript ready *> $null
                if ($LASTEXITCODE -ne 0) {
                    Write-Output "cmux web dev: local Postgres unavailable; restarting for CMUX_PORT=$cmuxPort"
                    & $pwshExe -NoProfile -File $dbScript up *> $null
                    if ($LASTEXITCODE -eq 0) {
                        bunx drizzle-kit migrate --config $drizzleConfig *> $null
                    }
                }
            }
            Start-Sleep -Seconds 2
        }
    } -ArgumentList $parent_pid, $script:dev_lock_file, $PwshExe, $DbLocalScript, $DrizzleConfig, $env:CMUX_PORT, $OwnsLockProbe.ToString()
}

function Stop-NextTree {
    if ($null -ne $script:next_proc -and -not $script:next_proc.HasExited) {
        # taskkill /T terminates the whole child tree (the bash pkill -P analogue).
        # Documented stopgap until the Job-Object teardown lands.
        & taskkill /T /F /PID $script:next_proc.Id *> $null
        try { $script:next_proc.WaitForExit(5000) | Out-Null } catch { }
    }
}

function Invoke-Cleanup {
    if ($script:cleanup_ran) { return }
    $script:cleanup_ran = $true

    Stop-NextTree

    if ($started_db -eq 1 -and $env:CMUX_DEV_STOP_DB_ON_EXIT -ne "0") {
        if ($null -ne $script:db_watchdog_job) {
            Stop-Job $script:db_watchdog_job -ErrorAction SilentlyContinue
            Remove-Job $script:db_watchdog_job -Force -ErrorAction SilentlyContinue
        }
        if ($null -ne $script:cleanup_watcher_job) {
            Stop-Job $script:cleanup_watcher_job -ErrorAction SilentlyContinue
            Remove-Job $script:cleanup_watcher_job -Force -ErrorAction SilentlyContinue
        }
        Stop-LocalServices
    }

    if (Test-OwnsDevLock) {
        Remove-Item -LiteralPath $script:dev_lock_file -Force -ErrorAction SilentlyContinue
    }
}
$script:cleanup_ran = $false

try {
    if ($env:CMUX_DEV_START_DB -ne "0") {
        $started_db = 1
        Set-DevLock
        Start-CleanupWatcher
        $code = Invoke-DbLocal -DbCommand "up" -Quiet
        if ($code -ne 0) { exit $code }
        bunx drizzle-kit migrate --config $DrizzleConfig
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Start-DbWatchdog
    }

    $redacted_database_url = "postgres://$($env:CMUX_DB_USER):<redacted>@localhost:$($env:CMUX_DB_PORT)/$($env:CMUX_DB_NAME)"
    @"
cmux web dev
  CMUX_PORT=$($env:CMUX_PORT)
  CMUX_VM_API_BASE_URL=$($env:CMUX_VM_API_BASE_URL)
  DATABASE_URL=$redacted_database_url
  CMUX_WEB_SECRET_ENV_FILE=$($env:CMUX_WEB_SECRET_ENV_FILE)
  CMUX_WEB_EXTRA_SECRET_ENV_FILE=$($env:CMUX_WEB_EXTRA_SECRET_ENV_FILE)
"@ | Write-Output

    $nextCmd = Get-Command next -ErrorAction Stop
    $script:next_proc = Start-Process -FilePath $nextCmd.Source -ArgumentList @("dev", "--port", $env:CMUX_PORT) -NoNewWindow -PassThru
    $script:next_proc.WaitForExit()
    $status = $script:next_proc.ExitCode
    $script:next_proc = $null
    Invoke-Cleanup
    exit $status
} finally {
    Invoke-Cleanup
}
