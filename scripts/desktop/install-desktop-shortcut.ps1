param(
  [string]$ExePath = "",
  [string]$ShortcutName = "cmux"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..\..")

if ([string]::IsNullOrWhiteSpace($ExePath)) {
  $ReleaseExe = Join-Path $RepoRoot "target\release\cmux-desktop.exe"
  $DebugExe = Join-Path $RepoRoot "target\debug\cmux-desktop.exe"

  if (Test-Path $ReleaseExe) {
    $ExePath = $ReleaseExe
  } elseif (Test-Path $DebugExe) {
    $ExePath = $DebugExe
  } else {
    throw "cmux-desktop.exe was not found. Build it first with: cargo build -p cmux-desktop"
  }
}

$ResolvedExe = Resolve-Path $ExePath
$Desktop = [Environment]::GetFolderPath("DesktopDirectory")
$ShortcutPath = Join-Path $Desktop "$ShortcutName.lnk"

$Shell = New-Object -ComObject WScript.Shell
$Shortcut = $Shell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = $ResolvedExe.Path
$Shortcut.WorkingDirectory = $RepoRoot.Path
$Shortcut.IconLocation = $ResolvedExe.Path
$Shortcut.Description = "Launch cmux for Windows"
$Shortcut.Save()

Write-Host "Created desktop shortcut: $ShortcutPath"
Write-Host "Target: $($ResolvedExe.Path)"
