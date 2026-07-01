# Phase 6 — Polish, integration & ship   [L]

**Goal:** an installable, signed Windows cmux **beta**.

**Depends on:** Phases 2–5.

## Tasks

1. **Notifications** — wire the `cmux-core` notification store to **Windows toast**
   notifications (system integration).
2. **Session restore polish** — robust restore across restarts via
   `cmux-windowing` (edge cases: missing cwd, closed workspaces, monitor changes).
3. **Theming / appearance** — canonical theme options; light/dark; terminal theme.
4. **Performance pass** — typing latency and large-output throughput in the
   xterm/bridge path; no per-keystroke allocations; virtualize where needed (R5).
5. **Packaging** — Tauri bundler → MSI/NSIS installer; bundle the WebView2
   bootstrapper; ship the `cmux` CLI + `cmuxd` sidecar as configured.
6. **Code signing** — a **reputable certificate** so installs are clean under
   Smart App Control / SmartScreen (R4).
7. **Auto-update** — Tauri updater (or equivalent) with signed release artifacts.
8. **Docs & beta** — install/usage docs; changelog; ship a beta.

## Reuse
`cmux-core` notifications (done), `cmux-windowing` restore (done); Tauri bundler +
updater.

## Deliverable
A signed, auto-updating Windows cmux beta installer.

## Acceptance
- Installer runs on a clean machine; app launches without SAC/SmartScreen friction
  (signed).
- Notifications, restore, and theming work.
- Latency/throughput acceptable under load.
- Auto-update applies a signed release.

## Risks
- Signing cert & reputation (R4) — procure early.
- Auto-update correctness with signing.
- Performance under load (R5).

## Touchpoints
`crates/cmux-core` (notifications, restore), `apps/desktop/src-tauri`
(`tauri.conf.json` bundle/updater, toast), signing/CI config, docs.

## Cross-cutting to close out here
- **CI hardening** (old M13) — Windows CI across the workspace + web tests +
  golden parity.
- **Performance & parity pass** (old M14).
- Decide whether the **Go daemon** and **CLI per-command socket layer** ship in
  the beta or stay deferred (see `90` optional tracks).
