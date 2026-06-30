# Dot-source this file from dev scripts (`. ./scripts/load-dev-env.ps1`). It is the
# PowerShell analogue of `source scripts/load-dev-env.sh`: it intentionally keeps
# local dev database URLs derived from CMUX_PORT so parallel worktrees cannot hit
# the same Postgres instance by accident.
#
# IMPORTANT: this script must be DOT-SOURCED so its $env: assignments land in the
# caller's scope. On error it uses `return` (never `exit`) so dot-sourcing an
# interactive shell does not kill that shell.

$cmux_web_dir = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path

# Capture pre-existing env presence (the bash ${VAR+x} set-vs-unset distinction)
# so caller-provided values are restored AFTER sourcing the secret files. Note:
# PowerShell cannot represent a set-but-empty env var (assigning '' removes it),
# so "set" here means "present with a value", matching how callers use these.
$cmux_existing_cmux_port_set = Test-Path Env:CMUX_PORT
$cmux_existing_cmux_port = $env:CMUX_PORT
$cmux_existing_port_set = Test-Path Env:PORT
$cmux_existing_port = $env:PORT
$cmux_existing_db_port_offset_set = Test-Path Env:CMUX_DB_PORT_OFFSET
$cmux_existing_db_port_offset = $env:CMUX_DB_PORT_OFFSET
$cmux_existing_db_port_set = Test-Path Env:CMUX_DB_PORT
$cmux_existing_db_port = $env:CMUX_DB_PORT
$cmux_existing_db_user_set = Test-Path Env:CMUX_DB_USER
$cmux_existing_db_user = $env:CMUX_DB_USER
$cmux_existing_db_password_set = Test-Path Env:CMUX_DB_PASSWORD
$cmux_existing_db_password = $env:CMUX_DB_PASSWORD
$cmux_existing_db_name_set = Test-Path Env:CMUX_DB_NAME
$cmux_existing_db_name = $env:CMUX_DB_NAME

# HOME -> USERPROFILE for the default ~/.secrets locations on Windows.
$cmux_home = $env:USERPROFILE

$cmux_extra_secret_file = if (-not [string]::IsNullOrEmpty($env:CMUXTERM_EXTRA_ENV_FILE)) {
    $env:CMUXTERM_EXTRA_ENV_FILE
} elseif (-not [string]::IsNullOrEmpty($env:CMUX_WEB_EXTRA_ENV_FILE)) {
    $env:CMUX_WEB_EXTRA_ENV_FILE
} else {
    ""
}
if ([string]::IsNullOrEmpty($cmux_extra_secret_file) -and (Test-Path -LiteralPath (Join-Path $cmux_home ".secrets\cmux.env"))) {
    $cmux_extra_secret_file = (Join-Path $cmux_home ".secrets\cmux.env")
}

$cmux_secret_file = if (-not [string]::IsNullOrEmpty($env:CMUXTERM_ENV_FILE)) {
    $env:CMUXTERM_ENV_FILE
} elseif (-not [string]::IsNullOrEmpty($env:CMUX_WEB_ENV_FILE)) {
    $env:CMUX_WEB_ENV_FILE
} else {
    ""
}
if ([string]::IsNullOrEmpty($cmux_secret_file)) {
    if (Test-Path -LiteralPath (Join-Path $cmux_home ".secrets\cmuxterm-dev.env")) {
        $cmux_secret_file = (Join-Path $cmux_home ".secrets\cmuxterm-dev.env")
    } elseif (Test-Path -LiteralPath (Join-Path $cmux_home ".secret\cmuxterm.env")) {
        $cmux_secret_file = (Join-Path $cmux_home ".secret\cmuxterm.env")
    } elseif (Test-Path -LiteralPath (Join-Path $cmux_home ".secrets\cmuxterm.env")) {
        $cmux_secret_file = (Join-Path $cmux_home ".secrets\cmuxterm.env")
    } else {
        [Console]::Error.WriteLine("Missing cmux web secrets. Expected ~/.secrets/cmuxterm-dev.env.")
        return
    }
}

# Parse KEY=VALUE lines into $env:. This replaces bash `set -a; source FILE`.
#
# DEVIATION (documented): bash `source` executes the file as a shell script, so a
# secret file may use ${...} expansion, command substitution, multi-line quoting,
# or `export` statements. This static parser only understands literal
# `KEY=VALUE` (optionally surrounded-quote-stripped) lines and SKIPS anything it
# cannot parse. Any secret file relying on shell expansion/quoting is NOT fully
# emulated and must be flagged.
function Import-CmuxSecretFile {
    param([string]$Path)

    foreach ($line in (Get-Content -LiteralPath $Path)) {
        if ($line -match '^\s*$') { continue }
        if ($line -match '^\s*#') { continue }
        $idx = $line.IndexOf('=')
        if ($idx -lt 0) { continue }
        $key = $line.Substring(0, $idx).Trim()
        # Tolerate an optional leading `export ` like a sourced shell file would.
        if ($key -match '^export\s+(.+)$') { $key = $Matches[1] }
        if ([string]::IsNullOrEmpty($key)) { continue }
        $val = $line.Substring($idx + 1)
        # Strip a single pair of matching surrounding quotes.
        if ($val.Length -ge 2 -and (
                ($val[0] -eq '"' -and $val[-1] -eq '"') -or
                ($val[0] -eq "'" -and $val[-1] -eq "'"))) {
            $val = $val.Substring(1, $val.Length - 2)
        }
        Set-Item -Path ("Env:" + $key) -Value $val
    }
}

if (-not [string]::IsNullOrEmpty($cmux_extra_secret_file)) {
    if (Test-Path -LiteralPath $cmux_extra_secret_file) {
        Import-CmuxSecretFile -Path $cmux_extra_secret_file
    } else {
        [Console]::Error.WriteLine("cmux web dev: extra secret file not found: $cmux_extra_secret_file")
    }
}
Import-CmuxSecretFile -Path $cmux_secret_file

# If the primary secret file does not define STACK_SUPER_SECRET_ADMIN_KEY, ensure
# it is not inherited from the ambient environment.
if (-not (Select-String -LiteralPath $cmux_secret_file -Pattern '^STACK_SUPER_SECRET_ADMIN_KEY=' -Quiet)) {
    Remove-Item -Path Env:STACK_SUPER_SECRET_ADMIN_KEY -ErrorAction SilentlyContinue
}

# Restore caller-set values that the secret files may have overwritten.
if ($cmux_existing_cmux_port_set) { $env:CMUX_PORT = $cmux_existing_cmux_port }
if ($cmux_existing_port_set) { $env:PORT = $cmux_existing_port }
if ($cmux_existing_db_port_offset_set) { $env:CMUX_DB_PORT_OFFSET = $cmux_existing_db_port_offset }
if ($cmux_existing_db_port_set) { $env:CMUX_DB_PORT = $cmux_existing_db_port }
if ($cmux_existing_db_user_set) { $env:CMUX_DB_USER = $cmux_existing_db_user }
if ($cmux_existing_db_password_set) { $env:CMUX_DB_PASSWORD = $cmux_existing_db_password }
if ($cmux_existing_db_name_set) { $env:CMUX_DB_NAME = $cmux_existing_db_name }

$cmux_port = if (-not [string]::IsNullOrEmpty($env:CMUX_PORT)) {
    $env:CMUX_PORT
} elseif (-not [string]::IsNullOrEmpty($env:PORT)) {
    $env:PORT
} else {
    "3777"
}
if ($cmux_port -notmatch '^[0-9]+$') {
    [Console]::Error.WriteLine("CMUX_PORT must be numeric, got: $cmux_port")
    return
}
$env:CMUX_PORT = $cmux_port

$cmux_db_offset = if (-not [string]::IsNullOrEmpty($env:CMUX_DB_PORT_OFFSET)) {
    $env:CMUX_DB_PORT_OFFSET
} else {
    "10000"
}
if ($cmux_db_offset -notmatch '^[0-9]+$') {
    [Console]::Error.WriteLine("CMUX_DB_PORT_OFFSET must be numeric, got: $cmux_db_offset")
    return
}
$env:CMUX_DB_PORT_OFFSET = $cmux_db_offset

if ([string]::IsNullOrEmpty($env:CMUX_DB_USER)) { $env:CMUX_DB_USER = "cmux" }
if ([string]::IsNullOrEmpty($env:CMUX_DB_PASSWORD)) { $env:CMUX_DB_PASSWORD = "cmux" }
if ([string]::IsNullOrEmpty($env:CMUX_DB_NAME)) { $env:CMUX_DB_NAME = "cmux" }
if ([string]::IsNullOrEmpty($env:CMUX_DB_PORT)) {
    $env:CMUX_DB_PORT = [string]([int]$cmux_port + [int]$cmux_db_offset)
}

if ($env:CMUX_DEV_USE_EXTERNAL_DATABASE_URL -ne "1") {
    $env:DATABASE_URL = "postgres://$($env:CMUX_DB_USER):$($env:CMUX_DB_PASSWORD)@localhost:$($env:CMUX_DB_PORT)/$($env:CMUX_DB_NAME)"
    $env:DIRECT_DATABASE_URL = $env:DATABASE_URL
} elseif ([string]::IsNullOrEmpty($env:DIRECT_DATABASE_URL) -and -not [string]::IsNullOrEmpty($env:DATABASE_URL)) {
    $env:DIRECT_DATABASE_URL = $env:DATABASE_URL
}

if ($env:CMUX_DEV_USE_EXTERNAL_VM_API_BASE_URL -ne "1") {
    $env:CMUX_VM_API_BASE_URL = "http://localhost:$($env:CMUX_PORT)"
}

# Local dev should not require a checked-in or per-worktree .env.local just to pass
# startup validation for routes the developer is not exercising.
if ([string]::IsNullOrEmpty($env:RESEND_API_KEY)) { $env:RESEND_API_KEY = "cmux-local-dev" }
if ([string]::IsNullOrEmpty($env:CMUX_FEEDBACK_FROM_EMAIL)) { $env:CMUX_FEEDBACK_FROM_EMAIL = "dev@example.invalid" }
if ([string]::IsNullOrEmpty($env:CMUX_FEEDBACK_RATE_LIMIT_ID)) { $env:CMUX_FEEDBACK_RATE_LIMIT_ID = "cmux-feedback-local" }
if ([string]::IsNullOrEmpty($env:CMUX_PUSH_RATE_LIMIT_ID)) { $env:CMUX_PUSH_RATE_LIMIT_ID = "cmux-push-local" }

$env:CMUX_WEB_SECRET_ENV_FILE = $cmux_secret_file
$env:CMUX_WEB_EXTRA_SECRET_ENV_FILE = $cmux_extra_secret_file
$env:PATH = "$cmux_web_dir\node_modules\.bin;$($env:PATH)"
