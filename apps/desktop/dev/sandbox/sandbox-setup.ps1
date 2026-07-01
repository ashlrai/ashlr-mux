<#
  Runs INSIDE Windows Sandbox on logon (mapped in at C:\host by run-sandbox.ps1).
  A fresh sandbox lacks the WebView2 runtime, so install it, then launch the
  app. The VC++ runtime DLLs are already staged next to the exe on the host, so
  they need no handling here.
#>
$ErrorActionPreference = 'Continue'
Write-Host '=== cmux sandbox setup ===' -ForegroundColor Cyan

$exe = 'C:\cmux\cmux-desktop.exe'
Write-Host ("exe present: {0}" -f (Test-Path $exe))

# --- WebView2 Evergreen runtime (fixed client GUID) ---
$guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$keys = @(
  "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$guid",
  "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$guid"
)
$wv2 = $null
foreach ($k in $keys) {
  $v = (Get-ItemProperty -Path $k -ErrorAction SilentlyContinue).pv
  if ($v) { $wv2 = $v }
}

if ($wv2) {
  Write-Host "WebView2 runtime present: $wv2" -ForegroundColor Green
} else {
  Write-Host 'Installing WebView2 runtime...' -ForegroundColor Yellow
  $inst = Join-Path $env:TEMP 'wv2.exe'
  # Prefer a host-cached installer (C:\host\cache) if present; else download the
  # Evergreen bootstrapper. The bootstrapper host (go.microsoft.com) is reliable
  # from inside the sandbox in practice.
  $cached = 'C:\host\cache\MicrosoftEdgeWebView2Setup.exe'
  try {
    if (Test-Path $cached) {
      Copy-Item $cached $inst -Force
      Write-Host 'using cached WebView2 installer' -ForegroundColor DarkGray
    } else {
      Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $inst -UseBasicParsing
    }
    Start-Process -FilePath $inst -ArgumentList '/silent', '/install' -Wait
    Write-Host 'WebView2 install finished.' -ForegroundColor Green
  } catch {
    Write-Host ("WebView2 install FAILED: {0}" -f $_.Exception.Message) -ForegroundColor Red
  }
}

Write-Host 'Launching cmux-desktop.exe ...' -ForegroundColor Green
try {
  Start-Process -FilePath $exe
} catch {
  Write-Host ("launch threw: {0}" -f $_.Exception.Message) -ForegroundColor Red
}

Start-Sleep -Seconds 4
if (Get-Process cmux-desktop -ErrorAction SilentlyContinue) {
  Write-Host 'cmux-desktop is RUNNING - the window should be visible.' -ForegroundColor Green
} else {
  Write-Host 'cmux-desktop EXITED immediately - see errors above.' -ForegroundColor Red
}
Write-Host ''
Write-Host 'This setup window stays open for diagnostics.' -ForegroundColor Cyan
