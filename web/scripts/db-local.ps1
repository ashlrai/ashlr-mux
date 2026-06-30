# PowerShell port of scripts/db-local.sh. Manages the per-worktree local Postgres
# container whose project/container/volume names + port are derived from CMUX_PORT
# and the branch slug, so parallel worktrees never collide. The derived names MUST
# stay byte-identical to db-local.sh or a mac-started and a Windows-started
# worktree would target different containers.
[CmdletBinding()]
param(
    [string]$Command = "status"
)

# Emulate `set -euo pipefail`: terminating errors stop the script. Native command
# (docker/git/bunx) exit codes are checked explicitly via $LASTEXITCODE below.
$ErrorActionPreference = "Stop"

$ROOT_DIR = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path

$REPO_DIR = git -C $ROOT_DIR rev-parse --show-toplevel 2>$null
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrEmpty($REPO_DIR)) {
    $REPO_DIR = (Resolve-Path -LiteralPath (Join-Path $ROOT_DIR "..")).Path
} else {
    $REPO_DIR = $REPO_DIR.Trim()
}

$cmux_port = if (-not [string]::IsNullOrEmpty($env:CMUX_PORT)) {
    $env:CMUX_PORT
} elseif (-not [string]::IsNullOrEmpty($env:PORT)) {
    $env:PORT
} else {
    "3777"
}
if ($cmux_port -notmatch '^[0-9]+$') {
    [Console]::Error.WriteLine("CMUX_PORT must be numeric, got: $cmux_port")
    exit 2
}

$db_kind = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_KIND)) { $env:CMUX_DB_KIND } else { "dev" }
$db_offset = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_PORT_OFFSET)) { $env:CMUX_DB_PORT_OFFSET } else { "10000" }
if ($db_offset -notmatch '^[0-9]+$') {
    [Console]::Error.WriteLine("CMUX_DB_PORT_OFFSET must be numeric, got: $db_offset")
    exit 2
}

$db_port = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_PORT)) { $env:CMUX_DB_PORT } else { [string]([int]$cmux_port + [int]$db_offset) }
$db_user = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_USER)) { $env:CMUX_DB_USER } else { "cmux" }
$db_password = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_PASSWORD)) { $env:CMUX_DB_PASSWORD } else { "cmux" }
$db_name = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_NAME)) { $env:CMUX_DB_NAME } else { "cmux" }

$branch = git -C $REPO_DIR branch --show-current 2>$null
if ($LASTEXITCODE -ne 0) { $branch = "" }
$branch = "$branch".Trim()
if ([string]::IsNullOrEmpty($branch)) {
    $branch = Split-Path -Leaf $REPO_DIR
}

# Branch slug: lowercase, non-alnum -> '-', strip leading/trailing '-', collapse
# repeated '-', cut to 48 chars. Mirrors the db-local.sh tr/sed/cut pipeline.
# `-creplace` is case-sensitive to match sed's [a-z0-9] exactly after lowercasing.
$slug = $branch.ToLowerInvariant()
$slug = $slug -creplace '[^a-z0-9]+', '-'
$slug = $slug -creplace '^-+', ''
$slug = $slug -creplace '-+$', ''
$slug = $slug -creplace '-+', '-'
if ($slug.Length -gt 48) { $slug = $slug.Substring(0, 48) }
if ([string]::IsNullOrEmpty($slug)) { $slug = "worktree" }

# All names default-if-unset so callers (test mode) can override them.
if ([string]::IsNullOrEmpty($env:COMPOSE_PROJECT_NAME)) { $env:COMPOSE_PROJECT_NAME = "cmux-db-$slug-$db_kind-$cmux_port" }
if ([string]::IsNullOrEmpty($env:CMUX_DB_CONTAINER_NAME)) { $env:CMUX_DB_CONTAINER_NAME = "cmux-postgres-$slug-$db_kind-$cmux_port" }
if ([string]::IsNullOrEmpty($env:CMUX_DB_VOLUME_NAME)) { $env:CMUX_DB_VOLUME_NAME = "cmux-postgres-$slug-$db_kind-$cmux_port" }
$env:CMUX_DB_PORT = $db_port
$env:CMUX_DB_USER = $db_user
$env:CMUX_DB_PASSWORD = $db_password
$env:CMUX_DB_NAME = $db_name
if ([string]::IsNullOrEmpty($env:DATABASE_URL)) { $env:DATABASE_URL = "postgres://$($db_user):$($db_password)@localhost:$($db_port)/$($db_name)" }
if ([string]::IsNullOrEmpty($env:DIRECT_DATABASE_URL)) { $env:DIRECT_DATABASE_URL = $env:DATABASE_URL }

$ComposeFile = Join-Path $ROOT_DIR "docker-compose.db.yml"

function Invoke-Compose {
    # docker compose reads COMPOSE_PROJECT_NAME / CMUX_DB_* from the environment.
    docker compose -f $ComposeFile @args
}

function Wait-ForPostgres {
    foreach ($attempt in 1..60) {
        Invoke-Compose exec -T postgres pg_isready -U $db_user -d $db_name *> $null
        if ($LASTEXITCODE -eq 0) {
            return
        }
        Start-Sleep -Seconds 1
    }
    [Console]::Error.WriteLine("Timed out waiting for Postgres on localhost:$db_port")
    (Invoke-Compose ps 2>&1) | ForEach-Object { [Console]::Error.WriteLine($_) }
    exit 1
}

function Write-Status {
    $redacted_url = "postgres://$($db_user):<redacted>@localhost:$($db_port)/$($db_name)"
    @"
CMUX_PORT=$cmux_port
CMUX_DB_KIND=$db_kind
CMUX_DB_PORT=$db_port
COMPOSE_PROJECT_NAME=$($env:COMPOSE_PROJECT_NAME)
CMUX_DB_CONTAINER_NAME=$($env:CMUX_DB_CONTAINER_NAME)
CMUX_DB_VOLUME_NAME=$($env:CMUX_DB_VOLUME_NAME)
DATABASE_URL=$redacted_url
"@ | Write-Output
}

# Re-invoke this script as a child process (the analogue of bash `"$0" ...`,
# optionally under the `env -u .../KEY=VAL` scrub used by the `test` command).
# Removing/overriding entries on a copied environment block isolates the child so
# the test container/volume/project names re-derive cleanly without leaking the
# overrides back into the current scope.
function Invoke-SelfDbLocal {
    param(
        [Parameter(Mandatory = $true)][string]$ChildCommand,
        [string[]]$RemoveVars = @(),
        [hashtable]$SetVars = @{},
        [switch]$Quiet
    )

    $pwshExe = (Get-Process -Id $PID).Path
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $pwshExe
    $psi.UseShellExecute = $false
    foreach ($a in @("-NoProfile", "-File", $PSCommandPath, $ChildCommand)) {
        [void]$psi.ArgumentList.Add($a)
    }
    foreach ($entry in [System.Environment]::GetEnvironmentVariables().GetEnumerator()) {
        $psi.Environment[[string]$entry.Key] = [string]$entry.Value
    }
    foreach ($name in $RemoveVars) {
        if ($psi.Environment.ContainsKey($name)) { [void]$psi.Environment.Remove($name) }
    }
    foreach ($name in $SetVars.Keys) {
        $psi.Environment[[string]$name] = [string]$SetVars[$name]
    }
    if ($Quiet) {
        $psi.RedirectStandardOutput = $true
    }
    $proc = [System.Diagnostics.Process]::Start($psi)
    if ($Quiet) { [void]$proc.StandardOutput.ReadToEnd() }
    $proc.WaitForExit()
    return $proc.ExitCode
}

switch ($Command) {
    "up" {
        Invoke-Compose up -d
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Wait-ForPostgres
        Write-Status
    }
    "down" {
        Invoke-Compose down
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    "reset" {
        Invoke-Compose down -v
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Invoke-Compose up -d
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Wait-ForPostgres
        Write-Status
    }
    "status" {
        Write-Status
        Invoke-Compose ps
    }
    "migrate" {
        $code = Invoke-SelfDbLocal -ChildCommand "up" -Quiet
        if ($code -ne 0) { exit $code }
        bunx drizzle-kit migrate --config (Join-Path $ROOT_DIR "drizzle.config.ts")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    "ready" {
        Invoke-Compose exec -T postgres pg_isready -U $db_user -d $db_name *> $null
        exit $LASTEXITCODE
    }
    "test" {
        # env -u analogue: scrub the worktree-specific names + URLs so the child
        # `up` re-derives a clean cmux_test container/volume/project.
        $test_offset = if (-not [string]::IsNullOrEmpty($env:CMUX_TEST_DB_PORT_OFFSET)) { $env:CMUX_TEST_DB_PORT_OFFSET } else { "30000" }
        $test_name = if (-not [string]::IsNullOrEmpty($env:CMUX_TEST_DB_NAME)) { $env:CMUX_TEST_DB_NAME } else { "cmux_test" }
        $code = Invoke-SelfDbLocal -ChildCommand "up" -Quiet `
            -RemoveVars @("COMPOSE_PROJECT_NAME", "CMUX_DB_CONTAINER_NAME", "CMUX_DB_VOLUME_NAME", "CMUX_DB_PORT", "DATABASE_URL", "DIRECT_DATABASE_URL") `
            -SetVars @{ CMUX_DB_KIND = "test"; CMUX_DB_PORT_OFFSET = $test_offset; CMUX_DB_NAME = $test_name }
        if ($code -ne 0) { exit $code }

        $env:CMUX_DB_TEST = "1"
        $env:CMUX_DB_KIND = "test"
        $env:CMUX_DB_PORT_OFFSET = $test_offset
        $env:CMUX_DB_NAME = $test_name
        $env:CMUX_DB_PORT = [string]([int]$cmux_port + [int]$test_offset)
        $env:DATABASE_URL = "postgres://$($db_user):$($db_password)@localhost:$($env:CMUX_DB_PORT)/$($env:CMUX_DB_NAME)"
        $env:DIRECT_DATABASE_URL = $env:DATABASE_URL
        bunx drizzle-kit migrate --config (Join-Path $ROOT_DIR "drizzle.config.ts")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        bunx drizzle-kit migrate --config (Join-Path $ROOT_DIR "drizzle.config.ts")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        # DEFERRED: run-db-behavior-tests.ps1 does not exist yet; the bash version
        # relies on find/mktemp/PIPESTATUS and stays the source of truth. On
        # Windows this requires Git Bash on PATH (db:test parity is deferred).
        bash (Join-Path $ROOT_DIR "scripts/run-db-behavior-tests.sh")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    "url" {
        Write-Output $env:DATABASE_URL
    }
    default {
        [Console]::Error.WriteLine("Usage: bun db:{up,down,reset,status,migrate,ready,test}")
        exit 2
    }
}
