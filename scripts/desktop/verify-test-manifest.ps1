param(
    [string]$TestExecutable,
    [switch]$RunList
)

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
    Write-Host "Skipping desktop test manifest verification on non-Windows."
    exit 0
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $TestExecutable) {
    $targetRoot = if ($env:CARGO_TARGET_DIR) {
        $env:CARGO_TARGET_DIR
    } else {
        Join-Path $repoRoot "target"
    }

    $testExecutable = Get-ChildItem -Path (Join-Path $targetRoot "debug\deps") `
        -Filter "cmux_desktop_lib-*.exe" -File |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}

if (-not $TestExecutable -or -not (Test-Path -LiteralPath $TestExecutable -PathType Leaf)) {
    throw "cmux-desktop lib test executable not found; run 'cargo test -p cmux-desktop --lib --no-run' first"
}

function Find-NativeTool([string]$Name, [scriptblock]$Fallback) {
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    $path = & $Fallback
    if (-not $path) {
        throw "$Name was not found"
    }
    return $path
}

$mt = Find-NativeTool "mt.exe" {
    Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Filter mt.exe -Recurse `
        -ErrorAction SilentlyContinue |
        Where-Object FullName -Match '\\x64\\mt\.exe$' |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
$dumpbin = Find-NativeTool "dumpbin.exe" {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
        $visualStudio = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath
        Get-ChildItem "$visualStudio\VC\Tools\MSVC" -Filter dumpbin.exe -Recurse `
            -ErrorAction SilentlyContinue |
            Where-Object FullName -Match '\\Hostx64\\x64\\dumpbin\.exe$' |
            Sort-Object FullName -Descending |
            Select-Object -First 1 -ExpandProperty FullName
    }
}
$manifestPath = Join-Path ([System.IO.Path]::GetTempPath()) "cmux-desktop-test-$PID.manifest"

try {
    & $mt "-inputresource:$TestExecutable;#1" "-out:$manifestPath"
    if ($LASTEXITCODE -ne 0) {
        throw "mt.exe could not extract manifest resource #1 from $TestExecutable (exit $LASTEXITCODE)"
    }

    $manifest = Get-Content -LiteralPath $manifestPath -Raw
    if ($manifest -notmatch 'name=["'']Microsoft\.Windows\.Common-Controls["'']') {
        throw "manifest does not request Microsoft.Windows.Common-Controls"
    }
    if ($manifest -notmatch 'version=["'']6\.0\.0\.0["'']') {
        throw "manifest does not request Common-Controls version 6.0.0.0"
    }

    $imports = (& $dumpbin /imports $TestExecutable | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin could not inspect imports for $TestExecutable (exit $LASTEXITCODE)"
    }
    if ($imports -notmatch '(?m)^\s+.*\bTaskDialogIndirect\s*$') {
        throw "test executable does not import TaskDialogIndirect"
    }

    if ($RunList) {
        & $TestExecutable --list | Out-Host
        if ($LASTEXITCODE -ne 0) {
            $exitCodeHex = '{0:x8}' -f ($LASTEXITCODE -band 0xffffffffL)
            throw "test executable --list failed with exit $LASTEXITCODE (0x$exitCodeHex)"
        }
    }

    Write-Host "PASS: test executable imports TaskDialogIndirect and requests Common-Controls v6."
    if ($RunList) {
        Write-Host "PASS: test executable --list exited 0."
    }
} finally {
    Remove-Item -LiteralPath $manifestPath -Force -ErrorAction SilentlyContinue
}
