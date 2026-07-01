<#
.SYNOPSIS
  Build cmux-desktop and launch it inside Windows Sandbox (a SAC-free VM), so a
  locally-compiled unsigned binary can actually run on a machine where Smart App
  Control blocks it on the host.

.DESCRIPTION
  Host side of the turnkey sandbox harness:
    1. Builds the web frontend + the cmux-desktop debug exe (unless -SkipBuild).
    2. Stages the VC++ 2015-2022 runtime DLLs next to the exe - a fresh Windows
       Sandbox has no VC++ redistributable, and the exe's own directory is first
       in the DLL search order, so this needs no in-VM download.
    3. Generates a .wsb with absolute mapped-folder paths and launches it.
  The in-VM setup (WebView2 install + app launch) is handled by
  sandbox-setup.ps1, mapped in and run on logon.

.NOTES
  Requires the "Containers-DisposableClientVM" (Windows Sandbox) optional feature.
  Enable once (elevated): Enable-WindowsOptionalFeature -Online -FeatureName
  "Containers-DisposableClientVM" -All   (then reboot).
#>
[CmdletBinding()]
param(
  # Skip the web + cargo build and just relaunch the existing exe.
  [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$ScriptDir = $PSScriptRoot
# dev/sandbox -> dev -> desktop -> apps -> <repo root (cmux)>
$RepoRoot  = (Resolve-Path (Join-Path $ScriptDir '..\..\..\..')).Path
$TargetDir = Join-Path $RepoRoot 'target\debug'
$Exe       = Join-Path $TargetDir 'cmux-desktop.exe'

Write-Host "cmux repo root: $RepoRoot" -ForegroundColor DarkGray

if (-not $SkipBuild) {
  Write-Host 'Building web frontend (bun)...' -ForegroundColor Cyan
  Push-Location $RepoRoot
  try {
    & bun run desktop:web:build
    if ($LASTEXITCODE -ne 0) { throw "web build failed (exit $LASTEXITCODE)" }
    Write-Host 'Building cmux-desktop (cargo, debug)...' -ForegroundColor Cyan
    & cargo build -p cmux-desktop
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
  } finally {
    Pop-Location
  }
}

if (-not (Test-Path $Exe)) {
  throw "cmux-desktop.exe not found at $Exe - run without -SkipBuild first."
}

# Stage the VC++ runtime DLLs next to the exe (fresh sandbox has no redist).
$dlls = 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll', 'MSVCP140.dll'
foreach ($d in $dlls) {
  $dest = Join-Path $TargetDir $d
  if (-not (Test-Path $dest)) {
    $src = Join-Path $env:WINDIR "System32\$d"
    if (Test-Path $src) {
      Copy-Item $src $dest -Force
      Write-Host "staged $d" -ForegroundColor DarkGray
    } else {
      Write-Warning "host is missing $d - the app may fail to start in the sandbox"
    }
  }
}

# Generate the .wsb with absolute paths (Windows Sandbox requires absolute
# HostFolder values, so this can't be a committed static file).
$wsb = @"
<Configuration>
  <VGpu>Enable</VGpu>
  <Networking>Enable</Networking>
  <MemoryInMB>4096</MemoryInMB>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$TargetDir</HostFolder>
      <SandboxFolder>C:\cmux</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$ScriptDir</HostFolder>
      <SandboxFolder>C:\host</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -ExecutionPolicy Bypass -NoExit -File C:\host\sandbox-setup.ps1</Command>
  </LogonCommand>
</Configuration>
"@

$out = Join-Path $env:TEMP 'cmux-sandbox.wsb'
# Write without a BOM so the sandbox host parses the XML cleanly.
[System.IO.File]::WriteAllText($out, $wsb, (New-Object System.Text.UTF8Encoding($false)))

Write-Host "Launching Windows Sandbox ($out)..." -ForegroundColor Green
Start-Process $out
Write-Host 'The sandbox will install WebView2 and launch cmux-desktop on logon.' -ForegroundColor Cyan
