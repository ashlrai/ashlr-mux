# cmux-desktop Windows Sandbox harness

Run the locally-built (unsigned) `cmux-desktop` app on a Windows machine where
**Smart App Control (SAC)** blocks unsigned binaries — without disabling SAC.

Windows Sandbox is a disposable VM that boots SAC-free, and because it's real
Windows it exercises the actual ConPTY path (not a Linux PTY stand-in).

## One-time setup

Enable the Windows Sandbox feature in an **elevated** PowerShell, then reboot:

```powershell
Enable-WindowsOptionalFeature -Online -FeatureName "Containers-DisposableClientVM" -All
```

Windows Sandbox requires Windows 10/11 **Pro** or Enterprise + hardware
virtualization.

## Usage

Double-click **`launch.cmd`** (or run it from a terminal). It will:

1. Build the web frontend and the `cmux-desktop` debug exe.
2. Stage the VC++ 2015-2022 runtime DLLs (`VCRUNTIME140.dll`,
   `VCRUNTIME140_1.dll`, `MSVCP140.dll`) next to the exe — a fresh sandbox has no
   VC++ redistributable, and the exe's own directory is first in the DLL search
   order, so no in-VM download is needed.
3. Generate a `.wsb` (in `%TEMP%`) with absolute mapped-folder paths and launch
   the sandbox.

Inside the sandbox, `sandbox-setup.ps1` runs on logon: it installs the WebView2
runtime (missing from a fresh sandbox) and launches the app. A live PowerShell
terminal should appear in the cmux window.

To relaunch without rebuilding:

```cmd
launch.cmd -SkipBuild
```

## Why each piece exists (discovered by first real launch)

- **`withGlobalTauri`** (in `tauri.conf.json`, not here): Tauri 2 doesn't inject
  `window.__TAURI__` unless opted in; without it the webview can't reach the Rust
  commands and the terminal shows "Tauri bridge is unavailable".
- **VC++ DLLs**: staged host-side because the sandbox's DNS flaked on the
  `vc_redist.x64.exe` CDN; copying the DLLs sidesteps any download.
- **WebView2**: installed in-VM each run because the sandbox is disposable. Drop a
  cached `MicrosoftEdgeWebView2Setup.exe` in `dev/sandbox/cache/` to reuse it and
  skip the re-download.

## Notes

- Every launch is a fresh VM, so WebView2 reinstalls each time (~1-2 min). This
  is inherent to a disposable sandbox.
- The generated `.wsb` lives in `%TEMP%`; the mapped `target\debug` folder is
  read-only inside the sandbox.
