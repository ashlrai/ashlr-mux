[CmdletBinding()]
param(
    [string]$TargetTriple = "",
    [switch]$SkipGo,
    [switch]$AllowPlaceholderDaemon = $true
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$BinariesDir = Join-Path $RepoRoot "apps\desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $BinariesDir | Out-Null

function Resolve-TargetTriple {
    param([string]$ExplicitTriple)

    if ($ExplicitTriple) {
        return $ExplicitTriple
    }

    $hostLine = rustc -Vv | Select-String "host:"
    if (-not $hostLine) {
        throw "Unable to resolve Rust host triple."
    }

    return ($hostLine.Line -split " ")[1].Trim()
}

function Resolve-GoTarget {
    param([string]$Triple)

    switch -Regex ($Triple) {
        "^x86_64-pc-windows-msvc$" { return @{ GOOS = "windows"; GOARCH = "amd64"; Extension = ".exe" } }
        "^aarch64-pc-windows-msvc$" { return @{ GOOS = "windows"; GOARCH = "arm64"; Extension = ".exe" } }
        "^aarch64-apple-darwin$" { return @{ GOOS = "darwin"; GOARCH = "arm64"; Extension = "" } }
        "^x86_64-apple-darwin$" { return @{ GOOS = "darwin"; GOARCH = "amd64"; Extension = "" } }
        "^x86_64-unknown-linux-gnu$" { return @{ GOOS = "linux"; GOARCH = "amd64"; Extension = "" } }
        "^aarch64-unknown-linux-gnu$" { return @{ GOOS = "linux"; GOARCH = "arm64"; Extension = "" } }
        default { throw "Unsupported target triple for Go sidecar staging: $Triple" }
    }
}

$ResolvedTriple = Resolve-TargetTriple -ExplicitTriple $TargetTriple
$CliExtension = if ($ResolvedTriple -match "windows") { ".exe" } else { "" }

Write-Host "Staging desktop sidecars for $ResolvedTriple"

$CliTargetDir = Join-Path $RepoRoot "target\$ResolvedTriple\release"
cargo build --manifest-path (Join-Path $RepoRoot "crates\cmux-cli\Cargo.toml") --release --target $ResolvedTriple | Out-Host

$CliSource = Join-Path $CliTargetDir "cmux$CliExtension"
$CliDest = Join-Path $BinariesDir "cmux-$ResolvedTriple$CliExtension"
Copy-Item -Force $CliSource $CliDest

if ($SkipGo) {
    Write-Warning "Skipping Go sidecar staging by request."
    exit 0
}

$GoCommand = Get-Command go -ErrorAction SilentlyContinue
$DaemonDest = Join-Path $BinariesDir "cmuxd-remote-$ResolvedTriple$CliExtension"
if (-not $GoCommand) {
    if ($AllowPlaceholderDaemon) {
        Write-Warning "Go is not installed; copying the CLI stub as a placeholder cmuxd-remote sidecar for local M0 validation."
        Copy-Item -Force $CliDest $DaemonDest
        exit 0
    }

    throw "Go is required to stage the real cmuxd-remote sidecar."
}

$GoTarget = Resolve-GoTarget -Triple $ResolvedTriple
$GoOutput = Join-Path $BinariesDir "cmuxd-remote-$ResolvedTriple$($GoTarget.Extension)"
$GoEnv = @{
    GOOS = $GoTarget.GOOS
    GOARCH = $GoTarget.GOARCH
}

Push-Location (Join-Path $RepoRoot "daemon\remote")
try {
    $env:GOOS = $GoEnv.GOOS
    $env:GOARCH = $GoEnv.GOARCH
    go build -trimpath -buildvcs=false -o $GoOutput .\cmd\cmuxd-remote | Out-Host
}
finally {
    Pop-Location
    Remove-Item Env:\GOOS -ErrorAction SilentlyContinue
    Remove-Item Env:\GOARCH -ErrorAction SilentlyContinue
}
