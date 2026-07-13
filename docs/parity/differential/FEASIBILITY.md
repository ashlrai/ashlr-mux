# Frozen-Canonical Live Differential — Feasibility Report

Status date: 2026-07-13. Scope: making the `differential_live_runner_plan`
(docs/parity/contracts/workspace_lifecycle.json) and `differential_live_gate`
(docs/parity/contracts/pane_surface_lifecycle.json) executable. Pinned
canonical commit: `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`.

## Verdict

- **Canonical capture on GitHub-hosted macOS: FEASIBLE**, on the `macos-15`
  image, using the repo's own proven GitHub-hosted lane
  (`.github/workflows/ci-macos-compat.yml`). Implemented in
  `.github/workflows/canonical-capture.yml`.
- **Windows capture: FEASIBLE with one code-level blocker** — the Tauri
  backend's control pipe name is hard-coded, so per-fixture pipe isolation
  requires a small backend change (details below). Captures can run today
  serially against the fixed pipe.

## Canonical side — evidence

### GhosttyKit acquisition: prebuilt download, no zig-from-source build

CI does **not** build GhosttyKit from zig source in regular jobs. It downloads
a prebuilt, checksum-pinned `GhosttyKit.xcframework.tar.gz`:

- `.github/workflows/ci.yml:562-572` — `Cache GhosttyKit.xcframework` keyed by
  the ghostty submodule SHA, then `./scripts/download-prebuilt-ghosttykit.sh`
  on cache miss. `.github/workflows/ci-macos-compat.yml:65-75` does the same
  on generic macOS images.
- `scripts/download-prebuilt-ghosttykit.sh` fetches
  `https://github.com/manaflow-ai/ghostty/releases/download/xcframework-<ghostty_sha>-crashsubdir-cmux-crash-v1/GhosttyKit.xcframework.tar.gz`
  and verifies it against `scripts/ghosttykit-checksums.txt`.
- Building from source happens only in the dispatch-only
  `.github/workflows/build-ghosttykit.yml` (zig, ~20 min), which publishes the
  release the download script consumes.

**Pin verified for the frozen commit:** at `e1825d40` the ghostty submodule is
`dd726a9a6050abd67f0dee7bba65136557567994`
(`git ls-tree e1825d40 ghostty`), and `scripts/ghosttykit-checksums.txt` *at
that commit* contains the pin
`dd726a9a6050abd67f0dee7bba65136557567994 dafc9e3db622dcfa5e8c2581b89afdb55a97ba754c38f0712a60555a6a917bab`
(its last line). The prebuilt artifact download therefore works for the frozen
build with zero GhosttyKit compilation.

### macOS image / Xcode choice: `macos-15`

- The compat workflow's Xcode policy (`ci-macos-compat.yml:30-58`) is "latest
  Xcode installed on the runner", with the inline note that the project needs
  **Xcode 16+** (Swift tools 6.0 required by sentry-cocoa). GitHub-hosted
  `macos-15` images ship Xcode 16.x — sufficient.
- `CMUX_CI_XCODE_APP` + `CMUX_CI_REQUIRED_MACOS_SDK_MAJOR: "26"`
  (`ci.yml:523-524`, 923-925) apply only to the **self-hosted app-host test
  jobs** (runner label `vars.MACOS_RUNNER_15 || 'warp-macos-15-arm64-6x'`,
  `ci.yml:516`) which the fork does not have. A plain `xcodebuild -scheme cmux
  -configuration Debug build` has the looser compat-lane requirements — that
  exact build is what `ci-macos-compat.yml:183-193` runs on macOS 15 images
  (`blacksmith-6vcpu-macos-15` default, same image family as GH-hosted
  `macos-15`).
- zig: needed by an in-build helper step; `scripts/install-zig-ci.sh` works on
  macOS 15 (`skip_zig: false` in the compat matrix). On macOS 26 zig 0.15.2's
  MachO linker cannot resolve libSystem (`ci-macos-compat.yml:21`), so the
  `macos-26` fallback lane must set `skip_zig=true` → `CMUX_SKIP_ZIG_BUILD=1`.
- Headless GUI: GitHub-hosted macOS runners have no display;
  `scripts/create-virtual-display.m` (compiled + run in
  `ci-macos-compat.yml:167-181`) provides one, after which
  `scripts/smoke-test-ci.sh` proves the app launches, binds its control
  socket, and answers v1 `ping` with `PONG` on CI.

### Isolated tagged socket

Local dev uses `scripts/reload.sh --tag <t>` + `scripts/cmux-debug-cli.sh`
(socket `/tmp/cmux-debug-<slug>.sock`, per-tag DerivedData, bundled CLI at
`<app>/Contents/Resources/bin/cmux`). On an ephemeral CI VM the equivalent
isolation is achieved without the tagged product rename:

- dedicated DerivedData (`-derivedDataPath`), and
- an explicit socket via `CMUX_SOCKET_PATH=/tmp/cmux-capture.sock` — debug
  builds honor the override
  (`Packages/macOS/CmuxSettings/Sources/CmuxSettings/SocketControl/SocketControlSettings.swift`
  `socketPath`/`shouldHonorSocketPathOverride`; `CMUX_ALLOW_SOCKET_OVERRIDE=1`
  is the explicit force key, line 16) — verified at the pinned commit.
- `CMUX_SOCKET_MODE=allowAll CMUX_UI_TEST_MODE=1` mirror
  `scripts/smoke-test-ci.sh:30`.

The bundled CLI (`cmux DEV.app/Contents/Resources/bin/cmux`) is used for CLI
lanes, matching `scripts/cmux-debug-cli.sh:62`.

### Expected build minutes

The compat job (package resolve + full unit tests + app build + smoke) fits a
30-minute budget on `blacksmith-6vcpu-macos-15`. The capture workflow skips
the test suite; on GH-hosted `macos-15` (fewer vCPUs) expect **~25-45 min cold**
(package resolution + app build dominate), and materially less on re-runs via
the SPM/DerivedData/GhosttyKit caches. Workflow timeout is bounded at 90 min.

### Canonical-side risks (none hard-blocking)

1. **Pinned-commit-on-GH-image build unproven until first run.** The compat
   lane proves the recipe for repo HEAD on macOS-15-class images; `e1825d40`
   is a recent main commit so drift risk is low. Mitigation already wired: the
   `runner_image=macos-26` + `skip_zig=true` fallback inputs if the commit
   turns out to need SDK 26.
2. **Fork commit reachability.** `e1825d40` is an upstream-main commit; the
   workflow fetches the SHA from `origin` and falls back to
   `https://github.com/manaflow-ai/cmux.git` explicitly, so dispatch works
   even if the fork remote lacks the object.
3. **Events lane**: the driver holds one `events.stream` NDJSON connection per
   events-enabled case (`Sources/CmuxEventStream.swift` at the pinned commit);
   heartbeats are disabled via `include_heartbeats: false`. Event *timing*
   noise is bounded by the driver's settle delays; if flaky, per-case
   `approved_differences` must NOT be used to paper over ordering — extend the
   settle instead.

## Windows side — launch plan

### How e2e reaches a live backend today

- `scripts/desktop/run_desktop_test_matrix.py` `e2e` lane →
  `tests/test_desktop_e2e.py::test_native_desktop_smoke_launch`: builds
  `cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml`, then drives
  `scripts/desktop/smoke-launch-windows.ps1 -AppPath target/debug/cmux-desktop.exe`
  and asserts a visible window. There is no dedicated headless/test
  entrypoint; the control-socket listener starts with the app
  (`apps/desktop/src-tauri/src/control_socket.rs::start_control_socket_listener`).

### Plan

1. Build backend + CLI: `cargo build -p cmux-desktop -p cmux-cli`.
2. Launch `target/debug/cmux-desktop.exe` in an interactive desktop session
   (this dev machine, or a GH `windows-latest` runner — its agent runs in an
   interactive session, the same environment the smoke-launch e2e already
   tolerates; `smoke-launch-windows.ps1` classifies denied-window environments
   and the driver run should be gated on that same signal).
3. Wait for the pipe, then run the same driver:

   ```
   python scripts/parity/capture_driver.py ^
     --manifest docs/parity/differential/pane_surface_lifecycle.manifest.json ^
     --socket \\.\pipe\cmux ^
     --cli target\debug\cmux.exe ^
     --platform windows ^
     --output windows.ndjson
   ```

   The driver injects `CMUX_SOCKET_PATH=<pipe>` for CLI ops, which the Rust
   CLI honors (`crates/cmux-cli/src/socket.rs` precedence
   `--socket > CMUX_SOCKET_PATH > CMUX_SOCKET > default`), and speaks the same
   v1/v2 NDJSON frames (`crates/cmux-ipc/src/client.rs`).
4. Restart phases: pass `--restart-cmd` invoking a PowerShell relaunch script
   (kill `cmux-desktop.exe`, relaunch, poll the pipe) — the analogue of the
   canonical `/tmp/launch-canonical.sh`.

### Windows-side blockers

1. **[code change required] Fixed pipe name — no isolation.**
   `apps/desktop/src-tauri/src/control_socket.rs:78` hard-codes
   `CONTROL_PIPE_BASE_NAME = "cmux"` and `control_pipe_path()` (line 849) has
   no env/flag override, unlike canonical's `CMUX_SOCKET_PATH` policy. An
   "isolated named pipe" fixture therefore cannot exist until a
   `CMUX_CONTROL_PIPE_NAME` (or `CMUX_TAG`-suffix) override is added —
   a ~10-line change plumbed through `cmux_ipc::control_pipe_path`. Until
   then: run captures serially and ensure no other `cmux-desktop` instance
   owns `\\.\pipe\cmux`.
2. **Session-0 / windowless CI caveat.** Tauri needs a desktop session to
   create its window; GH `windows-latest` generally provides one, but this is
   exactly the environmental failure the smoke-launch script already detects —
   the Windows capture lane should reuse its detection before invoking the
   driver.
3. **Restore/persistence phases** need the Windows app to actually persist
   sessions under the same profile dir across the relaunch; verify the
   backend's snapshot path is stable per user before trusting the
   `persistence` lane.

## Comparison step (root integrator)

`scripts/parity/differential_harness.py::compare_observations` is directly
importable and takes two observation dicts plus the case's
`approved_differences` — both of which the capture NDJSON records provide
(`{"type":"case","id",...,"observation",...,"approved_differences":[...]}`).
Joining `canonical.ndjson` × `windows.ndjson` on case id and calling
`compare_observations` per pair is a ~30-line follow-up script; no harness
change is needed.

## Open blockers, ranked

1. Windows pipe-name override (code change; blocks *isolated* Windows fixture,
   not first serial captures).
2. First real dispatch of `canonical-capture.yml` (proves pinned-commit build
   on the GH `macos-15` image; fallback inputs exist).
3. `workspace_lifecycle` manifest not yet authored (the workflow input exists;
   the manifest-resolution step fails fast with a clear message).
4. Remote-tmux / Dock-rich fixtures from the contract's canonical_fixture
   (two windows, remote arrival, event-subscription restart matrix) are not in
   the first manifest — extend `pane_surface_lifecycle.manifest.json` after
   the first capture round-trips.
5. Join/compare entrypoint over the two NDJSON files (small script, see
   above).

## How to execute the first capture

```
gh workflow run canonical-capture.yml \
  --repo <fork-owner>/<fork-repo> \
  --ref parity/diff-lane \
  -f family=pane_surface_lifecycle
```

Then download the `canonical-capture-pane_surface_lifecycle` artifact
(`canonical.ndjson` + app/socket logs).
