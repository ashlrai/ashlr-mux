# Phase 5 — Sidebar, command palette, settings, config, shortcuts, i18n   [L]

**Goal:** the surrounding app chrome and configuration, canonical.

**Depends on:** Phase 2 (workspace shell / component base).

## Tasks

1. **Sidebar** — workspaces list + file explorer (React); data from Rust (fs +
   the workspace model). Mirror `CmuxSidebar` / `FileExplorer*` behavior.
2. **Command palette** (React) with an **action registry** mirroring cmux
   commands (the same actions bound to shortcuts and menus).
3. **Settings** UI (React) + **`cmux.json`** config (canonical schema) read/write
   via Rust. Include **keyboard-shortcut settings**: every cmux-owned shortcut is
   editable, persisted to `cmux.json`, and documented (shortcut policy in
   `CLAUDE.md`).
4. **i18n** — en/ja message catalogs. Localization is a **canonical requirement**;
   run the localization audit on every user-facing string (see `90`).

## Reuse
The React component base (Phase 1); `@cmux/core-types`; the `cmux.json` schema
(canonical).

## New code
Sidebar / palette / settings React; the config Rust layer (read/write/validate
`cmux.json`); i18n wiring + catalogs.

## Deliverable
Full app chrome; configurable via Settings and `cmux.json`; localized (en/ja).

## Acceptance
- Settings persist to `cmux.json` and reload correctly.
- Shortcuts are editable and take effect; palette runs the same actions.
- en + ja both present; localization audit clean.

## Risks
- Scope (this phase is broad) — sequence sidebar → palette → settings → i18n.
- Keeping the localization audit disciplined across many strings.

## Touchpoints
`crates/cmux-core` (config/model), `apps/desktop/src-tauri` (config commands),
`apps/desktop/web` (sidebar/palette/settings), `Packages/macOS/CmuxSidebar`/
`CmuxCommandPalette`/`CmuxSettingsUI` + `web/messages/*.json` (behavior +
localization reference).
