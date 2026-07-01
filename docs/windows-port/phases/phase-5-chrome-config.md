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

---

## Research-derived plan (2026-07-01, ultracode workflow `w5kvotmvq`)

Rebuild the app chrome (Vault/sessions sidebar, command palette, Settings incl.
keyboard shortcuts, `cmux.json` editor, i18n) in the Tauri+React shell under
`apps/desktop/web`, mirroring the canonical SwiftUI sources: sidebar =
`Sources/SessionIndexView.swift` + `SessionIndexStore.swift` +
`SessionIndexModels.swift`; palette = `Sources/CommandPalette/CommandPaletteOverlay.swift`
(+ `ContentView+*CommandPalette.swift`); settings = `Sources/SettingsNavigation.swift`,
`Sources/KeyboardShortcutSettings.swift`, `Sources/Settings/ConfigSettingsView.swift`.

**CRITICAL i18n finding — the localization source of truth is NOT `web/messages`.**
`web/messages/en.json`+`ja.json` (and the 18 other locales) are the **marketing
website** catalog — top-level keys are `ios`/`meta`/`home`/`blog`/`docs`/
`community`/`download`/`landing` — with **zero app-shell strings**. The actual
shell strings (`sessionIndex.*`, `settings.*`, `shortcut.*.label`, `menu.*`,
`command.*`; ~62 keys verified, EN+JA only) live in
`Resources/Localizable.xcstrings`. The React shell must therefore localize from a
**flattened `Localizable.xcstrings`**, not from `web/messages`. This overrides the
resume-doc assumption and must be resolved before any i18n coding.

**Already ported / already landed.** `crates/cmux-core/src/shortcuts_action.rs`
`Action` (109 variants; `Action::ALL`/`raw_value`/`from_raw`; serde renames ==
on-disk `cmux.json` keys) is ported — build the shortcuts UI directly on it, do
not re-enumerate. The `cmux-config` crate (landed 2026-07-01) is the `cmux.json`
backing store; React never writes the file directly, only via `settings_write` /
`config_write_raw`.

### Build order (concrete)
1. **Resolve i18n source-of-truth (blocking):** confirm `web/messages` is
   marketing-only and shell strings live in `Localizable.xcstrings` (both
   verified), then pick the extraction path (recommendation below).
2. **i18n substrate:** `apps/desktop/web/src/i18n/index.ts` exposing flat
   `t(key, vars?)` + `useT()` + locale switch; `scripts/gen-shell-messages.ts`
   flattens `Localizable.xcstrings` → `src/i18n/messages/{en,ja}.json` (mirroring
   the `String(localized: KEY, defaultValue:)` contract); bind locale from
   `cmux.json` `app.language` via the host bridge.
3. **Extend `@cmux/core-types`:** the generated barrel exports only
   `Session*Snapshot` today. Add ts-rs `#[ts(export)]` Rust models in `cmux-core`
   for sidebar/palette/settings payloads (see Data model) and re-export from the
   barrel; add the `ActionId` export per the in-file WS3 comment.
4. **Host bridge surface:** add the Tauri commands + `host.on` event streams (see
   below) to the `host.ts` channel map; keep `NativeReply` unwrap via `host.invoke`.
5. **Sidebar external store:** `useVaultStore.ts` — a `useSyncExternalStore`-backed
   store (entries/grouping/scope/currentDirectory/isLoading) **plus a SEPARATE drag
   store** (mirrors `SessionDragCoordinator` isolation); memoized
   `sectionsForCurrentGrouping()` selector (mirror the revision-cache).
6. **Sidebar components** (mirror the `SessionIndexView` tree): `VaultSidebar.tsx`
   (control bar: grouping toggle, "This folder only" scope, reload; loading/empty/
   list states) → `SectionReorderGap.tsx` + `IndexSection.tsx` (collapse header,
   drag-reorder, first-5 rows + Show more) → `SessionRow.tsx` (icon, displayTitle,
   relative time, hover, double-click preview, context menu) → `ShowMorePopover.tsx`
   (paginated search) → `TranscriptPreview.tsx` (virtualized). All of
   `IndexSection`/`SessionRow`/`SectionReorderGap` in `React.memo` with comparators
   that **exclude callbacks**; callbacks passed as one stable bundle (mirror
   `IndexSectionActions`/`SectionGapActions`).
7. **Palette model:** `useCommandPaletteModel.ts` porting
   `CommandPaletteOverlayRenderModel` — scheduled updates with monotonic sequence +
   `resultsVersion` guards to drop stale renders; state =
   `CommandPaletteCommandListRenderState`.
8. **Palette components:** `CommandPalette.tsx` (input + mode routing:
   switcher/goToWorkspace, view, rightSidebar, auth, canvas) + `CommandPaletteList.tsx`
   (virtualized rows, `matchedIndices` highlight, trailing shortcut/kind label,
   keyboard nav + scroll-follow). Catalog/exec via `command_palette_catalog`/`_run`.
9. **Settings scaffolding:** `SettingsWindow.tsx` (nav + detail + search),
   `SettingsNav.tsx` (16 `SettingsNavigationTarget` sections, localized titles +
   icons), `SettingsSearch.ts` (port `SettingsSearchIndex`: entries, tokenized
   scoring, `settingsPathAnchorIDs` key→anchor map, deep-link via `settings.navigate`).
10. **Settings sections:** one component per target under `settings/sections/*.tsx`,
    each control reading/writing exactly the `cmux.json` dotted path the Swift row
    uses, via `settings_read`/`settings_write`.
11. **Keyboard shortcuts UI:** `KeyboardShortcutsSection.tsx` driven by
    `shortcuts_action::Action` (iterate `Action::ALL`/`raw_value` + a NEW localized
    label map); `ShortcutRecorder.tsx` captures chords and delegates
    conflict/normalization to a Rust `shortcuts_validate`/`shortcuts_write`; persist
    to `shortcuts.bindings`; support reset-to-defaults.
12. **`cmux.json` editor:** `ConfigJsonEditor.tsx` mirroring `ConfigSettingsView`
    (source picker, path display, dirty tracking, save→reload) over
    `config_read_raw`/`config_write_raw` from `cmux-config`.
13. **Localization audit:** enumerate every user-facing string; ensure EN+JA in the
    generated catalog; add a drift test that fails on any `t()` key missing from a
    locale (mirrors the CLAUDE.md audit rule).
14. **Tests:** port the perf-boundary invariants — a render-count regression test
    asserting rows do not re-render on orthogonal store changes (issue #2586
    guardrails), plus core-types drift and i18n key-coverage checks.

### Host-bridge channels
Extend the `host.ts` channel map (do not invent a new bridge):
- **vault** — methods `vault_snapshot`, `vault_search_sessions`,
  `vault_load_directory_snapshot`, `vault_load_transcript`, `vault_reorder_sections`,
  `session_resume`; event `vault.changed`.
- **commandPalette** — methods `command_palette_catalog`, `command_palette_run`;
  no events.
- **settings** — methods `settings_read`, `settings_write`; events
  `settings.changed`, `settings.navigate`.
- **shortcuts** — methods `shortcuts_read`, `shortcuts_write`, `shortcuts_reset`,
  `shortcuts_validate`; event `shortcuts.changed`.
- **config** — methods `config_read_raw`, `config_write_raw`, `config_reload`;
  no events.

### New Rust commands
- `vault_snapshot` — enumerate + group vault sessions (replaces
  `sectionsForCurrentGrouping`); returns immutable snapshot for the list.
- `vault_search_sessions` — paginated Show-more search (`SessionSearchFn` parity).
- `vault_load_directory_snapshot` — full merged directory snapshot for in-memory
  pagination on empty query (`DirectorySnapshotFn` parity).
- `vault_load_transcript` — transcript turns for the preview popover (ports
  per-agent JSONL/SQLite parsing).
- `session_resume` — resume a session in a new tab/pane; builds the resume command
  native-side (`SessionEntryResumeCoordinator` parity).
- `vault_reorder_sections` — persist section drag-reorder (agentOrder/directoryOrder).
- `command_palette_catalog` — fuzzy-matched palette rows incl. `matchedIndices` +
  trailing shortcut/kind labels.
- `command_palette_run` — execute the selected row (routes to the shared action path).
- `settings_read` — read typed `cmux.json` values by dotted key for a section.
- `settings_write` — write one setting via `cmux-config`; fires `settings.changed`.
- `shortcuts_read` — effective bindings keyed by `Action` raw value.
- `shortcuts_write` — validate + normalize + persist a binding (conflict/
  numbered-digit/chord rules from `KeyboardShortcutSettings`); returns normalized
  value or error.
- `shortcuts_reset` — reset one or all bindings to defaults.
- `config_read_raw` — raw `cmux.json`/effective-config text + display paths for the
  editor (`ConfigSettingsView` parity).
- `config_write_raw` — save raw `cmux.json` and reload configuration.

### Data model
New ts-rs types to add to `@cmux/core-types` (today the barrel has only
`AppSessionSnapshot` + `Session*Snapshot`):
- **Sidebar:** `SessionEntry` `{ id, agent: SessionAgent (claude|codex|grok|
  opencode|rovodev|hermesAgent|registered{id,name,iconAssetName}), sessionId, title,
  cwd?, gitBranch?, pullRequest?{number,url,repository?}, modified, fileURL?,
  specifics }` — derived `displayTitle`/`cwdLabel`/`cwdBasename`/`resumeCommand`
  are computed native-side and shipped in the DTO (resume-command building is shell
  logic → port to Rust / precompute). `IndexSection` `{ key: SectionKey
  ('agent:<id>'|'dir:<path>'), title, icon: SectionIcon(.agent|.folder), entries }`;
  `shouldOfferShowMore = key.isDirectory || entries.count>5`. `SessionGrouping`
  (directory|agent). Search: `SearchScope` (agent|directory), `SearchOutcome`
  `{ entries, errors }`, `DirectorySnapshot` `{ cwd, entries, errors }`.
- **Palette:** `CommandPaletteRenderResultRow` `{ id, title, matchedIndices,
  trailingLabel?{text, style:'shortcut'|'kind'} }`;
  `CommandPaletteCommandListRenderState` `{ resultsVersion, emptyStateText,
  listIdentity, rows, selectedIndex, shouldShowEmptyState, scrollTargetID?,
  scrollTargetAnchor? }`.
- **Shortcuts:** reuse `shortcuts_action.rs` `Action`. **On-disk
  `shortcuts.bindings` keys are the serde-rename values** (e.g. `toggleRightSidebar`
  serializes as `toggleFileExplorer`) — the UI must round-trip via `Action` raw
  values, never re-derive keys. Add a shared `StoredShortcut` `{ key, command,
  shift, option, control, optional chord second stroke }`; port the per-action
  metadata (`defaultShortcut`, `usesNumberedDigitMatching`, `allowsBareFirstStroke`,
  `allowsChordShortcut`, `isBrowserContentShortcut`, `isPublicShortcutAction`,
  `label`) — currently Swift-only in `KeyboardShortcutSettings` — server-side, keyed
  by `Action`.
- **Settings:** `SettingsNavigationTarget` (16 sections), `SettingsSearchEntry`
  `{ id, kind:section|setting, target, title, subtitle?, symbolName, searchText }`,
  and `settingsPathAnchorIDs` (dotted-key → anchor). `cmux.json` is the single
  persistence backing store for all rows (dotted keys e.g. `app.language`,
  `sidebar.showPorts`, `terminal.copyOnSelect`, `shortcuts.bindings`), owned by
  `cmux-config`.

### Open decisions
- **i18n source of truth →** extract `Localizable.xcstrings` (canonical shell
  catalog, EN+JA) → per-locale flat JSON at build via `scripts/gen-shell-messages.ts`,
  shaped like `web/messages` so tooling is shared; xcstrings is source of truth,
  JSON generated. `web/messages` stays only for reused marketing/webview surfaces.
- **Session-list/grouping/search/transcript logic →** port `SessionIndexStore` +
  `SessionTranscriptLoader` to **Rust** behind `vault_*` commands (heavy native
  parsing — Claude/Codex/OpenCode JSONL+SQLite, ripgrep search, resume-command
  building — with existing golden patterns); the webview renders immutable DTOs,
  keeping the snapshot boundary clean.
- **Shortcut metadata + labels →** extend `cmux-core` with a shortcut-metadata
  module (defaults, numbered-digit/chord/bare-stroke flags, conflict logic) as the
  single source; expose labels as i18n keys (`shortcut.*.label` already in xcstrings)
  so the audit stays enforceable.
- **`cmux-config` API →** require typed `settings_read`/`settings_write` by dotted
  key (matching `settingsPathAnchorIDs`) **and** raw text for the editor; typed
  access avoids each control re-implementing JSONC patching and preserves
  comments/formatting.
- **DnD + virtualization libs →** `@tanstack/react-virtual` for lists + a minimal
  DnD (dnd-kit) with memo boundaries; whichever is chosen must NOT force context
  subscriptions into rows (would violate the snapshot boundary).

### Key risks
- **SwiftUI list-perf boundary (CLAUDE.md "Snapshot boundary for list subtrees",
  issue #2586) → React memo + external-store discipline.** No view below a
  virtualized list may hold a store reference; in React, rows/gaps must NOT consume
  the vault store via context/hook — pass immutable snapshots + one stable callback
  bundle (mirror `IndexSectionActions`/`SectionGapActions`/`SessionSearchFn`). A
  stray `useContext` in `SessionRow` reintroduces the 100% CPU re-render storm.
- **Drag-state isolation:** keep a SEPARATE drag store in React (mirrors
  `SessionDragCoordinator`); do not fold dragging into the vault store, or drag
  transitions invalidate data rows.
- **Equatable-skip parity:** `React.memo` comparators must compare only value props
  and ignore callbacks; callbacks must be referentially stable (`useCallback`/stable
  bundle) or memo is defeated.
- **No state mutation inside render:** palette/settings selectors must not schedule
  store writes during render (the `resultsVersion`/sequence guard exists precisely to
  avoid stale/feedback renders); do updates in effects/handlers only.
- **i18n coverage:** every user-facing string needs EN+JA; the marketing catalog is
  the wrong source and missing shell keys silently fall back to English. Add a
  key-coverage/drift test.
- **`shortcuts.bindings` on-disk keys are the serde-rename values** — round-trip via
  `Action` raw values, never re-derive keys, or you corrupt `cmux.json` compat.
- **Transcript/resume logic is macOS-shell-specific** (POSIX `shellQuote`,
  `NSWorkspace` open, Finder reveal) — re-derive resume commands and file-manager
  actions for Windows; do not copy the macOS shell strings verbatim.
- **Command palette has multiple modes** (view/rightSidebar/auth/canvas/switcher
  across `ContentView+*CommandPalette.swift`) — enumerate all before building; a
  naive single-list port will miss modes.
