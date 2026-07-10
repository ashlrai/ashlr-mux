# Compact Resume Card

Read this section first after compaction. The active goal is **not complete** until a broader parity audit confirms no required UI/backend work remains.

Urgent compaction checkpoint, 2026-07-09:

- `cmux capabilities` parity slice is implemented and ready to push: the previously unmapped user-facing command now calls the already-supported `system.capabilities` backend method and has concrete no-socket help. The focused red/green test, simplified system-command test grouping, complete `cmux-cli` suite (99 library + 3 binary tests), help smoke probe, formatting, and whitespace checks pass.
- Terminal text capture parity was pushed as `79efa6cdc`: Windows/Tauri now advertises and handles `surface.read_text`, mirrors each ConPTY output chunk into the existing Alacritty VT grid, keeps that grid synchronized on resize, and returns canonical-shaped plain text/base64 plus workspace/surface/window identities. `cmux read-screen` and `cmux capture-pane` now route through the method, support workspace/surface/window selectors, `--scrollback`, and positive `--lines`, print plain text by default, and expose concrete help. Capture trims never-used viewport rows while retaining an empty cursor row and applies line tails after meaningful-row selection.
- Terminal-capture verification passed after the test/simplify/retest loop: focused capture/CLI/format tests, complete `cmux-cli` and `cmux-terminal` suites, all non-desktop workspace targets, desktop all-target check, desktop test-target linking, desktop build, CLI help/error smoke probes, CI change-area guard, generator drift check, `cargo fmt`, and whitespace checks. Executing a desktop unit test again hit the known native harness/loader hang, so the final desktop gate remains compile/link/build rather than runtime.
- The broad workspace gate exposed three canonical shortcut actions missing from the generated Rust catalog after the upstream merge: `saveLayoutTemplate`, `newWorkspaceGroup`, and `cycleTextBoxSubmitAction`. The catalog was regenerated to 112 entries, exact default shortcuts and the no-chord policy were ported, and the generator now emits rustfmt-stable output so regeneration, drift checking, and formatting all pass together.
- Windows WebSocket PTY parity was pushed as `c6e3702f8`: the canonical authentication, lease, WebSocket/RPC, persistent-session, replay, resize, backpressure, input-sequence acknowledgement, and shutdown implementation is shared across platforms; small Unix and Windows launch/resize adapters supply `creack/pty` and native ConPTY respectively. Windows defaults to PowerShell, supports `cmd.exe` and Unix-like explicit shells, and writes oversized startup commands to platform-appropriate temporary scripts. The previous Windows `not implemented in M0` runtime stub is gone.
- WebSocket PTY verification passed after a simplification/retest loop: real Windows ConPTY attachment/ready/output/input/resize behavior, real TCP server startup/health/shutdown, focused tests five consecutive times, the complete remote Go suite twice, `go vet ./...`, and a Linux test-binary cross-compile. The race build could not run because this Go environment has CGO disabled (`-race requires cgo`). Cross-compilation also exposed a stale port-only `cli_unix_test.go` that duplicated canonical's now-cross-platform v2 CLI suite and retained three deleted v1 tests; the redundant file was removed.
- Canonical sync slice: merged upstream `origin/main` at `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452` (6,316 commits after the prior merge base). The merge had nine textual conflicts; resolutions preserve the Windows desktop CI route, add canonical Linux/TUI routes, adopt canonical's v2-only remote CLI, and remove four upstream-deleted v1 socket tests.
- Remote-daemon sync: canonical v2 relay/PTy code now owns the shared Go files while the port retains named-pipe, WinSock-refused, detached-process, and Windows lock seams. Windows adapters were updated for canonical admin-auth/input-sequence interfaces; CLI address detection now requires a valid numeric TCP port so `C:\...` control paths are not misclassified; named-pipe dials use a two-second startup budget. Canonical's expanded Go suite passes twice on Windows.
- Web/CI simplification: terminal focus activation now uses the pure `terminalPaneIsActive` predicate, removing a cross-file Bun mock race while preserving store tests and pointer/focus handler tests. CI routing tests now probe whether `bash` is actually executable before running shell-backed checks, avoiding false positives from Windows app aliases.
- Canonical-sync verification passed: full desktop web suite twice (1,232 tests), web typecheck/build, all non-desktop Rust workspace targets, desktop all-target check/test linking, Go suite twice, CI change-area guard, workspace package grouping, Package.resolved policy, and whitespace checks. `cmux-tui` validation reached its Ghostty build script but could not continue because Zig is not installed on this host.
- The goal remains incomplete. After pushing the Windows WebSocket PTY slice, run the next canonical-vs-Windows surface/backend audit and take the highest-confidence concrete parity gap through the same test/simplify/retest/push loop.
- The accumulated Windows/Tauri parity worktree was stabilized as one coherent baseline before the next upstream audit. The latest searchable state-write path was simplified to carry a narrowed binding key without a non-null assertion, and the core-types generator now strips trailing whitespace before committing generated bindings.
- Baseline verification passed: focused custom-sidebar tests (92), the full desktop-web test suite, web typecheck and production build, all non-desktop Rust workspace targets, `cargo check -p cmux-desktop --all-targets`, desktop test-target linking with `cargo test -p cmux-desktop --lib --no-run`, `cargo build -p cmux-desktop`, core-types generation/drift checks, desktop contract checks, shared agent-session tests (87), and `git diff --check`.
- Known local validation limitation: executing `cargo test -p cmux-desktop --lib` exits before the Rust harness with Windows `STATUS_ENTRYPOINT_NOT_FOUND` (`0xc0000139`). The same target compiles and links successfully, and the failure has no Rust assertion output or Application event-log crash record. Treat this as a native loader/environment follow-up, not a passing runtime test.
- Working repo is `C:\Users\User\coding\work\ashlr-mux\cmux`, not the outer `ashlr-mux` folder.
- User goal remains active: reach feature/UI/backend parity between ashlr-mux and cmux; do not mark complete yet.
- The recent high-confidence momentum is the Windows/Tauri custom-sidebar Swift renderer parity track.
- Latest completed verified slice is constrained SwiftUI `.searchable(text:prompt:placement:)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Searchable implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: constrained `.searchable(text: $localQuery, placement: .sidebar, prompt: "...")` now parses as a child-bearing modifier, renders sidebar-local search chrome above the modified content with stable `cmux-custom-sidebar-swift-searchable*` classes, `data-swift-search-*` breadcrumbs, prompt/placement metadata, and direct local `@State String` write-back through the existing sidebar state engine. This intentionally does **not** claim native search suggestions, tokens, scopes, platform search-field placement, or broad `SearchFieldPlacement` semantics.
- Searchable tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: child-bearing modifier parser coverage asserts value/prompt/placement/state-binding metadata, and SSR coverage asserts the search input chrome, prompt/value attributes, placement breadcrumbs, and local state binding metadata.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `.searchable(text:prompt:placement:)` support while keeping native suggestions/tokens/scopes/platform placement behavior as follow-up.
- Verification for the searchable slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (92 passed, 1156 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (142 passed, 1358 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before searchable support was constrained SwiftUI `ContentUnavailableView` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- ContentUnavailableView implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: constrained `ContentUnavailableView("Title", systemImage: ..., description: Text(...))` plus label/description/actions builder forms now parse and render as sidebar-local empty-state panels with stable `cmux-custom-sidebar-swift-content-unavailable*` classes, `data-swift-content-unavailable` breadcrumbs, optional SF Symbol fallback icon breadcrumbs, rich label/description children, and action children. The shared labeled trailing-closure reader now accepts `description:` and `actions:` labels. This intentionally does **not** claim native platform styling, search-specific convenience initializers, localization, or full symbol/style semantics.
- ContentUnavailableView tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage asserts simple title/system-image/description forms and builder label/description/actions forms; SSR coverage asserts empty-state classes, symbol breadcrumbs, description text, and action rendering.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained ContentUnavailableView support while keeping native platform/search/localization behavior as follow-up.
- Verification for the ContentUnavailableView slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (92 passed, 1148 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (142 passed, 1350 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest fully verified slice before ContentUnavailableView support was constrained SwiftUI `Link` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Link implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `Link("Title", destination: URL(string: "https://...")!)` and constrained `Link(destination: URL(string: "https://...")!) { ... }` label-builder forms now parse and render as sidebar-local external anchors for safe `http`/`https` destinations, with stable `cmux-custom-sidebar-swift-link` classes, `data-swift-link-*` breadcrumbs, rich label children, `target="_blank"`, and `rel="noreferrer"`. Unsafe or unsupported schemes render as inert disabled link-shaped rows with `data-swift-link-blocked` instead of clickable hrefs. `URL(string:)!` / `URL(string:)?` force/optional suffixes are normalized by the shared URL initializer unwrap path. This intentionally does **not** claim native `OpenURLAction`, environment override behavior, custom scheme dispatch, platform URL policy, or broad URL type semantics.
- Link tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage asserts title-form safe links, rich-label safe links, and blocked unsafe links; SSR coverage asserts safe hrefs, target/rel attributes, data breadcrumbs, rich label SF Symbol output, disabled blocked-link chrome, and absence of unsafe `file:` href leakage.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained Link support while keeping native OpenURL/custom-scheme semantics as follow-up.
- Verification for the Link slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (90 passed, 1130 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (140 passed, 1332 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Suggested next slice: continue the Swift renderer parity track by scanning for another common missing sidebar authoring primitive or stale `○` matrix item, then keep the parser/render/tests/docs/gates loop.
- Latest completed verified slice before Link support was constrained `DisclosureGroup` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- DisclosureGroup implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: static `DisclosureGroup("Title") { ... }`, `DisclosureGroup { ... }`, and constrained `DisclosureGroup(isExpanded: $localBool) { ... } label: { ... }` forms now parse and render as sidebar-local collapsible sections backed by `<details>/<summary>` chrome, stable `cmux-custom-sidebar-swift-disclosure-*` classes, `data-swift-disclosure-*` breadcrumbs, optional rich label children, and direct local `@State Bool` expansion write-back through the existing sidebar state engine. The shared trailing-closure reader now accepts `label:` closures for child-bearing SwiftUI views that use the same labeled trailing syntax. This intentionally does **not** claim native `DisclosureGroupStyle`, inherited style environments, outline/list integration, animation parity, or broad binding semantics.
- DisclosureGroup tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage asserts title-form and binding-backed rich-label disclosure groups, and SSR coverage asserts rendered details/summary chrome, expanded-state breadcrumbs, rich label children, state-binding metadata, and content output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained DisclosureGroup support while keeping native style/list/outline semantics as follow-up.
- Verification for the DisclosureGroup slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (88 passed, 1110 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (138 passed, 1312 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before DisclosureGroup support was constrained `GroupBox` / `.groupBoxStyle(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- GroupBox implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: static `GroupBox("Title") { ... }`, `GroupBox { ... }`, and constrained `GroupBox(label: { ... }) { ... }` forms now parse and render as sidebar-local labeled card containers with `cmux-custom-sidebar-swift-group-box` chrome, optional rich label children, stable `data-swift-group-box` / `data-swift-group-box-label` breadcrumbs, and `.groupBoxStyle(...)` token preservation through `cmux-custom-sidebar-swift-group-box-style-*` classes plus `data-swift-group-box-style`. This intentionally does **not** claim custom `GroupBoxStyle` structs, native platform group-box chrome, inherited style environments, or full initializer/format-style coverage.
- GroupBox tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage now asserts title-form and `label:`-closure group boxes plus `.groupBoxStyle(.card)` metadata, and SSR coverage asserts rendered group-box classes/data, rich label children, style breadcrumbs, and content output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained GroupBox support while keeping native/custom style semantics as follow-up.
- Verification for the GroupBox slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1094 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1296 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before GroupBox support was constrained `.toolbarBackground(...)` / `.toolbarColorScheme(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Toolbar-background/color-scheme implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: static navigation/toolbar chrome can now preserve `.toolbarBackground(...)` and `.toolbarColorScheme(...)` tokens, optional `for:` bar placements, stable `cmux-custom-sidebar-swift-toolbar-background-*` / `cmux-custom-sidebar-swift-toolbar-color-scheme-*` classes, `data-swift-toolbar-*` breadcrumbs, and safe sidebar-local nav/toolbar CSS background/color-scheme hints. This intentionally does **not** claim native platform toolbar slotting, collapsing, automatic placement resolution, color propagation, visibility resolution, or dynamic customization semantics.
- Toolbar-background/color-scheme tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the static navigation/toolbar parser and SSR fixtures now cover `.toolbarBackground(.blue, for: .navigationBar)` and `.toolbarColorScheme(.dark, for: .navigationBar)`, asserting parsed metadata plus rendered classes/data/style hints.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained toolbar chrome support while keeping native toolbar/platform semantics as follow-up.
- Verification for the toolbar-background/color-scheme slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1083 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1285 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before toolbar chrome support was constrained `.presentationBackground(...)` / `.presentationCornerRadius(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Presentation-background/corner implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: presented content can now carry `.presentationBackground(...)` and `.presentationCornerRadius(...)` metadata that is hoisted onto the sidebar-local sheet/popover/alert panel as stable `cmux-custom-sidebar-swift-presentation-background-*` / `cmux-custom-sidebar-swift-presentation-corner-radius` classes, `data-swift-presentation-background` / `data-swift-presentation-corner-radius` breadcrumbs, and safe panel CSS background/radius hints. This intentionally does **not** claim native material sampling, adaptive presentation behavior, platform sheet geometry, or exact SwiftUI modal/window semantics.
- Presentation-background/corner tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the local presentation parser and SSR fixtures now cover `.presentationBackground(.regularMaterial)` and `.presentationCornerRadius(22)` alongside detents/drag indicator, asserting parsed metadata plus rendered classes/data/style hints.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained presentation background/corner-radius support while keeping native material/adaptive/platform geometry semantics as follow-up.
- Verification for the presentation-background/corner slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1073 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1275 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before presentation background/corner support was constrained `.badge(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Badge implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.badge(...)` now preserves scalar/text badge values as stable `cmux-custom-sidebar-swift-badge` class and `data-swift-badge` metadata, and renders a sidebar-local pill on the authored node via CSS. This intentionally does **not** claim native list-row, tab, toolbar, menu, accessibility badge placement, platform badge aggregation, or exact SwiftUI placement semantics.
- Badge tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the rich leaf parser and SSR fixtures now cover expression-backed `.badge(unreadTotal)` and constant `.badge(3)`, asserting parsed metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained badge support while keeping native placement/aggregation semantics as follow-up.
- Verification for the badge slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1068 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1270 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before badge support was constrained `.hoverEffect(...)` / `.defaultHoverEffect(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Hover-effect implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.hoverEffect(...)` and `.defaultHoverEffect(...)` now preserve authored pointer-hover style tokens as stable `cmux-custom-sidebar-swift-hover-effect-*` / `cmux-custom-sidebar-swift-default-hover-effect-*` classes and `data-swift-hover-effect` / `data-swift-hover-effect-enabled` / `data-swift-default-hover-effect` metadata, with sidebar-local hover chrome hints for automatic/highlight/lift-style effects. Disabled hover effects remain metadata-only. This intentionally does **not** claim native pointer-region behavior, inherited default hover-effect environments, exact platform hover animations, or gesture/focus propagation semantics.
- Hover-effect tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the rich leaf parser and SSR fixtures now cover enabled `.hoverEffect(.lift, isEnabled: true)`, disabled `.hoverEffect(.highlight, isEnabled: false)`, and `.defaultHoverEffect(.highlight)`, asserting parsed metadata plus rendered classes/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained hover-effect support while keeping native pointer/default-hover/platform animation semantics as follow-up.
- Verification for the hover-effect slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1064 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1266 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before hover-effect support was constrained `.hidden()` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Hidden implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.hidden()` now parses as an explicit Swift modifier, preserves authored hidden-view intent as stable `cmux-custom-sidebar-swift-hidden` class and `data-swift-hidden="true"` metadata, and maps to sidebar-local `visibility:hidden` so layout space is retained while visible chrome is suppressed. This intentionally does **not** claim full SwiftUI identity/replacement behavior, transition/lifecycle behavior, native layout negotiation, or broader state/diff semantics.
- Hidden tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the rich leaf parser and SSR fixtures now cover `.hidden()`, asserting parsed metadata plus rendered class, data breadcrumb, and serialized visibility hint.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained hidden-view support while keeping native SwiftUI identity/transition/lifecycle/layout semantics as follow-up.
- Verification for the hidden slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1052 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1254 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before hidden support was constrained `.controlGroupStyle(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Control-group-style implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.controlGroupStyle(...)` now preserves built-in style tokens as stable `cmux-custom-sidebar-swift-control-group-style-*` classes and `data-swift-control-group-style`, and adds sidebar-local chrome hints for compact/menu/palette/navigation variants. This intentionally does **not** claim custom `ControlGroupStyle` structs, inherited environment propagation, native platform menu/palette behavior, or exact platform grouped-control semantics.
- Control-group-style tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the ControlGroup parser and SSR fixtures now cover `.controlGroupStyle(.compactMenu)`, asserting parsed metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained control-group-style support while keeping native/custom style semantics as follow-up.
- Verification for the control-group-style slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1049 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1251 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before control-group-style support was constrained `.labelsHidden()` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Labels-hidden implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.labelsHidden()` now preserves authored label-hiding intent as stable `cmux-custom-sidebar-swift-labels-hidden` classes and `data-swift-labels-hidden`, and applies sidebar-local visually-hidden label chrome for common labels/controls while keeping control UI and accessible labels intact. This intentionally does **not** claim inherited SwiftUI label-style environment propagation, exact platform label-layout semantics, or arbitrary custom style behavior.
- Labels-hidden tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the built-in control-style parser and SSR fixtures now cover `.labelsHidden()` on a text field, asserting parsed metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained labels-hidden support while keeping native/inherited label semantics as follow-up.
- Verification for the labels-hidden slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1047 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1249 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before labels-hidden support was constrained `.allowsHitTesting(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Allows-hit-testing implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.allowsHitTesting(...)` now parses no-arg/default true and explicit bool forms, preserves authored hit-testing intent as stable `cmux-custom-sidebar-swift-allows-hit-testing` / `cmux-custom-sidebar-swift-allows-hit-testing-off` classes and `data-swift-allows-hit-testing`, and maps `false` to a safe sidebar-local `pointer-events:none` hint. This intentionally does **not** claim native SwiftUI hit-test tree behavior, gesture priority/composition, keyboard-vs-pointer focus nuance, or full native event routing.
- Allows-hit-testing tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf parser and SSR fixtures now cover `.allowsHitTesting(false)`, asserting parsed bool metadata plus rendered class, data breadcrumb, and CSS pointer-events hint.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained allows-hit-testing support while keeping native hit-test/gesture semantics as follow-up.
- Verification for the allows-hit-testing slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1045 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1247 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before allows-hit-testing support was constrained `.safeAreaPadding(...)` / `.contentMargins(..., for:)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Safe-area/content-margin implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.safeAreaPadding(...)` and `.contentMargins(..., for:)` now parse constrained edge-set, length, `EdgeInsets`, and content-margin placement forms, render safe sidebar-local padding hints, emit stable `cmux-custom-sidebar-swift-safe-area-padding-*` / `cmux-custom-sidebar-swift-content-margins-*` classes, and preserve `data-swift-safe-area-padding-*` / `data-swift-content-margins-*` breadcrumbs. This intentionally does **not** claim native safe-area environment reads, scroll-container margin negotiation, platform inset behavior, or RTL-aware leading/trailing mirroring.
- Safe-area/content-margin tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover `.safeAreaPadding(.horizontal, 12)`, `.contentMargins(.bottom, 5, for: .scrollContent)`, and `.safeAreaPadding(EdgeInsets(top:leading:bottom:trailing:))`, asserting parsed metadata plus rendered classes/data breadcrumbs/CSS hints.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained safe-area/content-margin support while keeping native safe-area/scroll-margin semantics as follow-up.
- Verification for the safe-area/content-margin slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1042 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1244 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before safe-area/content-margin support was constrained `.clipShape(_:style:)` FillStyle metadata coverage for the Windows/Tauri custom-sidebar Swift renderer.
- ClipShape FillStyle implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.clipShape(shape, style: FillStyle(eoFill:antialiased:))` now preserves constrained `eoFill` and `antialiased` metadata, renders stable `cmux-custom-sidebar-swift-clip-style-eoFill` / `cmux-custom-sidebar-swift-clip-antialiased-off` classes, and emits `data-swift-clip-style` / `data-swift-clip-antialiased` breadcrumbs. This intentionally does **not** claim exact native clipping paths, fill-rule geometry, or rasterization/antialias behavior.
- ClipShape FillStyle tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover `.clipShape(Capsule(), style: FillStyle(eoFill: true, antialiased: false))`, asserting parsed metadata plus rendered classes/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained clip-shape FillStyle metadata support while keeping exact native clipping/fill-rule semantics as follow-up.
- Verification for the clip-shape FillStyle slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1020 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1222 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before clip-shape FillStyle metadata was constrained `RoundedRectangle(..., style:)` / `Capsule(style:)` corner-style metadata coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Shape corner-style implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: shape nodes now preserve constrained `style:` tokens for `RoundedRectangle` and `Capsule`, render stable `cmux-custom-sidebar-swift-shape-style-continuous` / `cmux-custom-sidebar-swift-shape-style-circular` classes, and emit `data-swift-shape-style` breadcrumbs. This intentionally does **not** claim exact native continuous/circular corner geometry, true container-radius reads, custom Shape, Canvas, or imperative Path builder parity.
- Shape corner-style tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover `RoundedRectangle(cornerRadius: 6, style: .continuous)` and `Capsule(style: .circular)`, asserting parsed metadata plus rendered classes/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained shape corner-style metadata support while keeping exact native geometry as follow-up.
- Verification for the shape corner-style slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1016 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1218 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before shape corner-style metadata was constrained `.padding(EdgeInsets(top:leading:bottom:trailing:))` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- EdgeInsets padding implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: padding modifiers now carry dedicated `paddingTop` / `paddingLeading` / `paddingBottom` / `paddingTrailing` metadata when authored as `.padding(EdgeInsets(...))`, and the renderer maps those values to side-specific CSS padding hints while preserving the existing scalar and edge-set padding paths. This intentionally does **not** claim RTL-aware leading/trailing mirroring, native SwiftUI layout negotiation, or exact platform padding wrapper semantics.
- EdgeInsets padding tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover `.padding(EdgeInsets(top: 1, leading: 2, bottom: 3, trailing: 4))`, asserting parsed side metadata and rendered side-specific padding CSS without regressing scalar or edge-set padding.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained EdgeInsets padding support while keeping RTL/native layout semantics as follow-up.
- Verification for the EdgeInsets padding slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1009 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1211 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before EdgeInsets padding was constrained redaction/privacy metadata coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Redaction/privacy implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.redacted(reason:)` now preserves normalized redaction reasons as stable classes and `data-swift-redaction-reason`, `.privacySensitive()` is tracked separately with `cmux-custom-sidebar-swift-privacy-sensitive` / `data-swift-privacy-sensitive`, and `.unredacted()` records an override breadcrumb with `cmux-custom-sidebar-swift-unredacted` / `data-swift-unredacted` while removing the sidebar-local placeholder mask. This intentionally does **not** claim inherited SwiftUI redaction environments, system privacy lock-state propagation, or exact native placeholder drawing.
- Redaction/privacy tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover `.redacted(reason: .placeholder)`, `.privacySensitive()`, and `.redacted(reason: [.placeholder, .privacy]).unredacted()`, asserting parsed modifier order plus rendered classes/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained redaction/privacy metadata support while keeping native redaction/privacy propagation as follow-up.
- Verification for the redaction/privacy slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 1003 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1205 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before redaction/privacy was sidebar-local `Form { ... }` container coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Form implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `Form { ... }` now parses as a form-marked list node, reuses the existing interpreted child/control path, renders stable `cmux-custom-sidebar-swift-form` and `data-swift-form="true"` breadcrumbs, and applies bounded sidebar-local form chrome hints. This intentionally does **not** claim native platform form row chrome, grouped/insetGrouped style fidelity, edit mode, section/list platform behavior, or broader form-specific SwiftUI semantics.
- Form tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover `Form { Section { TextField; Toggle } }`, asserting parsed `form: true`, editable control bindings inside the form, rendered form classes/data breadcrumbs, and retained control output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained sidebar-local Form support while keeping native form/platform semantics as follow-up.
- Verification for the Form slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (86 passed, 992 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (136 passed, 1194 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before Form was constrained `.flipsForRightToLeftLayoutDirection(...)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- RTL/image mirroring implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.flipsForRightToLeftLayoutDirection(...)` with no-arg/default true and explicit boolean forms, preserves the authored bool as render metadata, emits stable `cmux-custom-sidebar-swift-flips-for-rtl` / disabled classes, adds `data-swift-flips-for-rtl`, and applies a bounded CSS `scale: -1 1` mirror hint when active. This intentionally does **not** claim inherited native RTL image behavior, full SwiftUI leading/trailing layout negotiation, or platform image asset direction variants.
- RTL/image mirroring tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the image/presentation parser and SSR fixtures now cover `.flipsForRightToLeftLayoutDirection(true)`, asserting parsed metadata plus rendered class, data breadcrumb, and CSS mirror hint.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained RTL image-mirroring metadata while keeping native RTL layout/image semantics as follow-up.
- Verification for the RTL/image mirroring slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (84 passed, 983 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (134 passed, 1185 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before RTL/image mirroring was constrained read-only `@Environment(\.key)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Environment read implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift statement parser now accepts constrained `@Environment(\.colorScheme)`, `@Environment(\.layoutDirection)`, and `@Environment(\.locale)` property-wrapper declarations, seeding sidebar-stable read values (`light`, `leftToRight`, and a simple locale object with `identifier: "en-US"` / `languageCode: "en"`). Those values can be used in text interpolation, `if` branches, comparisons, member reads such as `locale.identifier`, and `cmux(...)` action params. This intentionally does **not** claim host/user preference propagation, inherited SwiftUI environment semantics, arbitrary environment keys, `@Environment(\.dismiss)`, `scenePhase`, or write-back-to-author behavior.
- Environment read tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser and SSR fixtures now cover color scheme, layout direction, and locale declarations, branch behavior, interpolation, locale member reads, and action params without `Unsupported @Environment` warnings.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained read-only environment declarations as partial support while preserving inherited/host-driven environment semantics as follow-up.
- Verification for the environment read slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (84 passed, 980 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (134 passed, 1182 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before environment reads was constrained `AsyncImage(url:) { image in ... } placeholder: { ... }` labeled trailing-closure coverage for the Windows/Tauri custom-sidebar Swift renderer.
- AsyncImage trailing-closure implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift view statement reader now consumes an `AsyncImage`-only trailing closure sequence with labeled `content:` / `placeholder:` closures before parsing following modifiers, and `parseAsyncImage` accepts those labeled trailing closures alongside the previously supported named `content:` / `placeholder:` initializer arguments. This keeps the parser narrow while allowing `.frame(...)` and other modifiers after the placeholder block. This intentionally does **not** claim `AsyncImagePhase` phase-closure semantics, true asynchronous load-state transitions, or arbitrary first-class `Image` value passing.
- AsyncImage trailing-closure tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the image/presentation parser and SSR fixtures now cover `AsyncImage(url:) { image in image.resizable() } placeholder: { Text(...) }.frame(width:height:)`, asserting parsed success/placeholder children, retained frame metadata after the placeholder label, rendered success-wrapper breadcrumbs, and the nested safe remote image.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark both named closure arguments and SwiftUI labeled trailing-closure spelling as supported while keeping phase/lifecycle semantics as follow-up.
- Verification for the AsyncImage trailing-closure slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (82 passed, 965 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (132 passed, 1167 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before AsyncImage trailing closures was constrained `AsyncImage(url:content:placeholder:)` named-closure coverage for the Windows/Tauri custom-sidebar Swift renderer.
- AsyncImage closure implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: async image nodes now preserve optional interpreted `successChildren` and `placeholderChildren`, collect event handlers from those closure children, and render named `content:` closures in a stable `cmux-custom-sidebar-swift-async-image-content` wrapper with `data-swift-async-image-phase`, `data-swift-async-image-url`, `data-swift-async-image-content-count`, and placeholder-count breadcrumbs. The common `content: { image in image.resizable().scaledToFill() }` pattern lowers the closure parameter to the existing async image leaf plus modifiers. The later trailing-closure slice adds SwiftUI's labeled `placeholder:` spelling; true async load lifecycle, SwiftUI `AsyncImagePhase`, arbitrary first-class `Image` value passing, and native placeholder-to-success timing remain follow-up.
- AsyncImage closure tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the image/presentation parser and SSR fixtures now cover named `AsyncImage(url:content:placeholder:)`, assert parsed success/placeholder children, and verify rendered success-wrapper breadcrumbs plus the nested safe remote image.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained named AsyncImage content/placeholder closure support while keeping phase/lifecycle/trailing-closure syntax as follow-up.
- Verification for the AsyncImage closure slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (82 passed, 960 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (132 passed, 1162 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before AsyncImage named closures was constrained Swift collection derivation coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Collection derivation implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: array transform handling now shares closure/key-path projection helpers and supports `.allSatisfy(...)`, `.min(by:)`, and `.max(by:)` alongside existing `filter`/`map`/`compactMap`/`flatMap`/`reduce`/`sorted`. `Dictionary(grouping:by:)` is now a bounded builtin that groups arrays into sidebar-local record values addressable through the existing dynamic subscript path. This intentionally does **not** claim first-class Swift `Dictionary<Key,Value>`, generic type checking, optional chaining, protocol-driven `Comparable`, full `Hashable` key semantics, or native Swift grouping behavior beyond JSON-like sidebar records.
- Collection derivation tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the collection transform parser/SSR fixtures now cover `Dictionary(grouping: workspaces, by: \.selected)`, closure-based grouping via `by: { ... }`, `.min(by: \.unreadCount)`, `.max(by: \.unreadCount)`, `.allSatisfy(\.selected)`, grouped boolean subscripts, and action-param propagation through `cmux(...)`.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained collection derivation support while keeping typed dictionaries/generics/native grouping semantics as follow-up.
- Verification for the collection derivation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (82 passed, 954 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (132 passed, 1156 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before collection derivations was constrained Swift key-path literal collection transform coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Key-path transform implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift evaluator now recognizes bounded `\.member` / `\.nested.member` / `\.self` key-path literals inside array transform arguments, projects them through the existing member resolver, and supports `.map(\.field)`, `.compactMap(\.field)`, `.flatMap(\.arrayField)`, and `.sorted(by: \.field)`. Sorting compares numbers, booleans, nil-like values, and strings conservatively. This intentionally does **not** claim first-class Swift `KeyPath` values, arbitrary key-path expressions, grouping, protocol-driven `Comparable`, optional chaining, or general typed `sorted(by:)` semantics.
- Key-path transform tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the collection transform parser/SSR fixtures now cover `ForEach(workspaces.map(\.title), id: \.self)`, `workspaces.sorted(by: \.title)`, `workspaces.compactMap(\.progress)`, and `workspaces.flatMap(\.ports)`, including action-param projection through `cmux(...)`.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained key-path transform support while keeping first-class key-path/grouping/type-system semantics as follow-up.
- Verification for the key-path transform slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (82 passed, 945 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (132 passed, 1147 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before key-path transforms was constrained SwiftUI `.bold(false)` / `.italic(false)` bool-argument coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Bold/italic bool implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now preserves explicit boolean arguments on `.bold(...)` and `.italic(...)`, while keeping no-arg `.bold()` / `.italic()` parsed in their previous stable shape. The renderer only applies bold/italic CSS when `boolValue !== false`, so `.bold(false)` and `.italic(false)` no longer force emphasis. This intentionally does **not** claim full inherited SwiftUI font resolution, attributed-run boundaries, or native font metric parity.
- Bold/italic bool tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf parser fixture now covers `.bold(false)`, the rich SSR fixture includes an explicit false row, and a focused SSR regression verifies `.bold(false).italic(false)` renders without `font-weight:760` or `font-style:italic`.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained bool-argument emphasis support while keeping full font semantics as follow-up.
- Verification for the bold/italic bool slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (82 passed, 935 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (132 passed, 1137 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before bold/italic bool arguments was constrained non-closure `Path(roundedRect:)` / `Path(ellipseIn:)` coverage for the Windows/Tauri custom-sidebar Swift renderer.
- Path convenience implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `Path(roundedRect: CGRect(...), cornerRadius:)` and `Path(ellipseIn: CGRect(...))` now parse as sidebar-local shape nodes using the structured geometry evaluator, preserve path rect metadata (`pathX`, `pathY`, `pathWidth`, `pathHeight`), render stable `cmux-custom-sidebar-swift-shape-pathRoundedRect` / `cmux-custom-sidebar-swift-shape-pathEllipse` classes, emit `data-swift-path`, `data-swift-path-x`, `data-swift-path-y`, `data-swift-path-width`, and `data-swift-path-height` breadcrumbs, and apply bounded CSS width/height hints. This intentionally does **not** claim imperative `Path { p in ... }`, Canvas, custom Shape protocol implementations, affine transforms, or exact native vector drawing.
- Path convenience tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the shapes parser/SSR fixtures now cover `Path(roundedRect: CGRect(x:y:width:height:), cornerRadius:)` and `Path(ellipseIn: CGRect(origin: CGPoint.zero, size: CGSize(...)))`, asserting parsed shape metadata plus rendered classes/data breadcrumbs and CSS size hints.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained Path convenience initializer support while keeping imperative Path/Canvas/custom Shape/native vector semantics as follow-up.
- Verification for the Path convenience slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 930 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1132 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before Path convenience initializers was constrained structured geometry literal coverage for the Windows/Tauri custom-sidebar Swift evaluator.
- Geometry literal implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift expression evaluator now carries constrained `Angle`, `UnitPoint`, `CGPoint`, `CGSize`, and `CGRect` values as structured internal metadata, supports `Angle(degrees:/radians:)`, `Angle.degrees(...)`, `Angle.radians(...)`, known `UnitPoint` tokens plus `UnitPoint(x:y:)`, `CGPoint(x:y:)`, `CGSize(width:height:)`, `CGRect(x:y:width:height:)`, `CGRect(origin:size:)`, `.zero` forms, and basic member access such as `.x`, `.y`, `.width`, `.height`, `.origin`, `.size`, `.degrees`, and `.radians`. Existing angle/point helpers now consume those values for hue/rotation/3D transform and accessibility activation point metadata. This intentionally does **not** claim full CoreGraphics APIs, path construction, affine transforms, arbitrary native UnitPoint placement, `GeometryProxy` values, or native SwiftUI layout semantics.
- Geometry literal tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the string-helper fixture now covers `CGSize(...).width` and `CGRect(...).height` interpolation, and layout/decorator parser/SSR fixtures now use `Angle.radians(...)`, `Angle(degrees:)`, `Angle.degrees(...)`, and `UnitPoint(x:y:)` while preserving the expected transform/filter metadata and rendered CSS output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained geometry literal support while keeping native CoreGraphics/path/GeometryProxy semantics as follow-up.
- Verification for the geometry literal slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 918 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1120 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before geometry literals was constrained `.accessibilityRepresentation { ... }` metadata/hidden-mirror coverage.
- AccessibilityRepresentation implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.accessibilityRepresentation { ... }`, preserves interpreted representation children as a child-bearing modifier, marks the modified wrapper with `cmux-custom-sidebar-swift-accessibility-representation`, emits `data-swift-accessibility-representation`, `data-swift-accessibility-representation-count`, and `data-swift-accessibility-representation-label` breadcrumbs, and renders the authored representation body as a hidden `aria-hidden` metadata mirror with `data-swift-accessibility-representation-content`. This intentionally does **not** claim native assistive-technology substitution, VoiceOver rotor integration, platform custom action menus, or full SwiftUI accessibility tree replacement semantics.
- AccessibilityRepresentation tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the child-bearing modifier parser/SSR fixtures now cover `.accessibilityRepresentation { Label("Voice row", systemImage: "speaker.wave.2") }`, asserting parsed child metadata plus rendered wrapper/content classes and data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained representation metadata/hidden-mirror support while keeping native representation substitution as follow-up.
- Verification for the accessibilityRepresentation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 917 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1119 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before accessibilityRepresentation was constrained `.accessibilityActivationPoint(...)` metadata coverage.
- AccessibilityActivationPoint implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts constrained `.accessibilityActivationPoint(CGPoint(x:y:))`, named `x:`/`y:` scalar metadata, and known `UnitPoint` tokens, lowers authored intent to stable `cmux-custom-sidebar-swift-accessibility-activation-point*` classes, and emits `data-swift-accessibility-activation-point`, `data-swift-accessibility-activation-point-x`, and `data-swift-accessibility-activation-point-y` breadcrumbs. This intentionally does **not** claim native VoiceOver activation geometry, browser hit-test relocation, arbitrary `CGPoint` expressions, or platform assistive-tech routing.
- AccessibilityActivationPoint tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf parser/SSR fixtures now cover `.accessibilityActivationPoint(CGPoint(x: 12, y: 24))`, asserting parsed x/y metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained activation-point metadata support while keeping native activation geometry as follow-up.
- Verification for the accessibilityActivationPoint slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 909 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1111 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Suggested next slice: inspect another stale matrix entry still marked missing/partial, likely a bounded `GeometryReader` placeholder/host seam, host-pushed `ScrollViewReader.scrollTo` intent support, richer native accessibility routing, or another SwiftUI modifier gap; keep each slice bounded and fully verified.
- Latest completed verified slice before accessibilityActivationPoint was constrained `.accessibilityAction { cmux(...) }` / `.accessibilityAction(named:) { cmux(...) }` support.
- AccessibilityAction implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.accessibilityAction { ... }`, `.accessibilityAction(.default) { ... }`, and `.accessibilityAction(named: Text("...")) { ... }`, preserves the normalized action name, captures safe `cmux(...)` actions, lowers the modifier to stable `cmux-custom-sidebar-swift-accessibility-action*` classes, emits `data-swift-accessibility-action` / `data-swift-accessibility-action-enabled` breadcrumbs, and makes constrained safe actions keyboard-invokable with Enter/Space by assigning `tabIndex=0` when needed. This intentionally does **not** claim native VoiceOver custom action menus, arbitrary Swift closure execution, custom activation points, or full platform assistive-tech routing.
- AccessibilityAction tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf parser/SSR fixtures now cover `.accessibilityAction(named: Text("Refresh")) { cmux("sidebar.reload", name: sourceName) }`, asserting parsed action metadata, rendered classes/data breadcrumbs, and `tabindex="0"`.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained safe accessibility action support while keeping native custom action menus/platform semantics as follow-up.
- Verification for the accessibilityAction slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 906 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1108 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before accessibilityAction was constrained `.onGeometryChange(for:) { ... }` metadata coverage.
- OnGeometryChange implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.onGeometryChange(for: SomeType.self) { proxy in ... }`, preserves the requested value type and transform-closure presence, raises the bounded modifier-chain parse cap from 25 to 40 so rich SwiftUI chains are not clipped, lowers the modifier to stable `cmux-custom-sidebar-swift-on-geometry-change*` classes, and emits `data-swift-on-geometry-change` / `data-swift-on-geometry-change-type` breadcrumbs. This intentionally does **not** claim host-supplied geometry, `GeometryProxy` values, transform evaluation, action execution, or state writes.
- OnGeometryChange tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf parser/SSR fixtures now cover `.onGeometryChange(for: CGSize.self) { proxy in proxy.size }`, asserting parsed `{ name: "onGeometryChange", value: "CGSize", boolValue: true }` metadata plus rendered classes/data breadcrumbs while preserving the existing `.id("copy-row")` chain coverage.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained onGeometryChange metadata support while keeping host geometry/action execution as follow-up.
- Verification for the onGeometryChange slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 901 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1103 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before onGeometryChange was constrained `.tabItem { Text/Label }` chrome coverage for non-selection `TabView`.
- TabItem implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now preserves `.tabItem { ... }` as a child modifier, extracts simple `Text(...)` / `Label(...)` labels for `TabView` children, renders a sidebar-local inert tab strip with `cmux-custom-sidebar-swift-tab-view-tabs` / `cmux-custom-sidebar-swift-tab-view-tab`, and emits `data-swift-tab-items`, `data-swift-tab-item-index`, and per-page `data-swift-tab-item` breadcrumbs. This intentionally does **not** claim `TabView(selection:)`, interactive tab switching, native page indicators, platform tab chrome, or full arbitrary tab item builders.
- TabItem tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the wrapper parser/SSR fixtures now cover `.tabItem { Text("Overview") }` and `.tabItem { Label("Details", systemImage: "list.bullet") }`, asserting parsed child-modifier metadata plus rendered tab-strip/page breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark simple tab item chrome support while keeping selection/native paging/platform tab behavior as follow-up.
- Verification for the tabItem slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 897 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1099 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before tabItem was constrained non-selection `TabView` / `.tabViewStyle(.page)` coverage.
- TabView implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift parser now accepts `TabView { ... }`, preserves interpreted child pages as a `tabView` node, accepts `.tabViewStyle(.page)` metadata, renders stable `cmux-custom-sidebar-swift-tab-view*` / `cmux-custom-sidebar-swift-tab-view-page` classes, emits `data-swift-tab-view`, `data-swift-tab-view-style`, and `data-swift-tab-view-page` breadcrumbs, and gives page style a sidebar-local horizontal scroll-snap hint. This intentionally does **not** claim `TabView(selection:)`, native page indicators, platform paging physics, or SwiftUI tab selection semantics.
- TabView tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the wrapper parser and SSR fixtures now cover `TabView { Text("First page"); Text("Second page") }.tabViewStyle(.page)`, asserting parsed node/style metadata plus rendered classes/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained TabView/page-style support while keeping selection/native paging as follow-up.
- Verification for the TabView slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 891 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1093 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before TabView was constrained `.visualEffect { content, proxy in ... }` metadata coverage.
- VisualEffect implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts authored `.visualEffect { ... }` trailing closures, preserves their presence as inert metadata, lowers them to the stable `cmux-custom-sidebar-swift-visual-effect` class, and emits the `data-swift-visual-effect="true"` breadcrumb. This intentionally does **not** claim real `GeometryProxy` values, closure execution, scroll-driven content transforms, host geometry borrowing, or native SwiftUI visual-effect layout behavior.
- VisualEffect tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the layout/decorator parser and SSR fixtures now cover `Text("Z").visualEffect { content, proxy in content }`, asserting parsed `{ name: "visualEffect", boolValue: true }` metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained visualEffect metadata support while keeping GeometryProxy-driven transform behavior as follow-up.
- Verification for the visualEffect slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 882 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1084 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before visualEffect was constrained `.scrollPosition(id:anchor:)` / `.defaultScrollAnchor(...)` metadata coverage.
- ScrollPosition implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts constrained `.scrollPosition(id:anchor:)`, `.scrollPosition(initialAnchor:)`, and `.defaultScrollAnchor(...)` metadata, preserves resolved ids/binding keys/anchors, lowers them to stable `cmux-custom-sidebar-swift-scroll-position*` / `cmux-custom-sidebar-swift-default-scroll-anchor-*` classes, and emits `data-swift-scroll-position-*` / `data-swift-default-scroll-anchor` breadcrumbs. This intentionally does **not** claim real binding-driven scroll-position sync, host-pushed scroll intents, `ScrollViewReader` proxy objects, or `scrollTo` behavior.
- ScrollPosition tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the text/list/scroll/symbol parser and SSR fixtures now cover `.scrollPosition(id: selectedId, anchor: .center)`, `.scrollPosition(id: "workspace-a", anchor: .center)`, and `.defaultScrollAnchor(.bottom)`, asserting parsed metadata plus rendered classes/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained scroll-position/default-anchor metadata support while keeping native binding/proxy scroll behavior as follow-up.
- Verification for the scrollPosition slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 880 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1082 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before scrollPosition was constrained `.alignmentGuide(...)` metadata/static-offset coverage.
- AlignmentGuide implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts constrained `.alignmentGuide(...) { ... }` modifiers, preserves guide tokens, extracts simple static numeric closure offsets, lowers them to stable `cmux-custom-sidebar-swift-alignment-guide*` classes, safe CSS margin hints, and `data-swift-alignment-guide` / `data-swift-alignment-guide-offset` breadcrumbs. This intentionally does **not** claim native `ViewDimensions` closure evaluation, SwiftUI alignment negotiation, custom alignment IDs, or host-borrowed layout callbacks.
- AlignmentGuide tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the layout/decorator parser and SSR fixtures now cover `Text("Z").alignmentGuide(.leading) { _ in 12 }`, asserting parsed modifier metadata plus rendered class, data breadcrumbs, and CSS margin output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained alignment-guide metadata/static-offset support while keeping real `ViewDimensions` semantics as follow-up.
- Verification for the alignmentGuide slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 874 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1076 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before alignmentGuide was constrained `.coordinateSpace(name:)` / `.coordinateSpace(.named(...))` leaf metadata coverage.
- CoordinateSpace implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts constrained `.coordinateSpace(name:)` and `.coordinateSpace(.named(...))` registrations, normalizes common `.local`/`.global`/named tokens, lowers them to stable `cmux-custom-sidebar-swift-coordinate-space*` classes, and emits `data-swift-coordinate-space` breadcrumbs. This intentionally does **not** claim host `GeometryProxy`, frame conversion, named-space lookups, or `GeometryReader`/`onGeometryChange` behavior.
- CoordinateSpace tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the layout/decorator parser and SSR fixtures now cover `ZStack { ... }.coordinateSpace(name: "board")`, asserting parsed modifier metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to split coordinate-space registration from host-borrowed geometry lookup semantics.
- Verification for the coordinateSpace slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 869 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1071 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Suggested next slice: inspect another stale matrix entry still marked missing/partial, likely `scrollPosition`/`ScrollViewReader` metadata/intents, `alignmentGuide` breadcrumbs, or a bounded `GeometryReader` placeholder/host seam; keep each slice bounded and fully verified.
- Latest completed verified slice before coordinateSpace was constrained scroll token modifier coverage for `.scrollTargetBehavior(...)`, `.scrollTargetLayout()`, `.scrollBounceBehavior(...)`, and `.scrollDisabled(...)`.
- Scroll token implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.scrollTargetBehavior(...)`, `.scrollTargetLayout(isEnabled:)`, `.scrollBounceBehavior(..., axes:)`, and `.scrollDisabled(...)`, lowers them to stable `cmux-custom-sidebar-swift-scroll-*` classes, safe CSS `overflow:hidden` for disabled scrolling, and `data-swift-scroll-target-*` / `data-swift-scroll-bounce-*` / `data-swift-scroll-disabled` breadcrumbs. This intentionally does **not** claim native paging/snap targets, bounce physics, scroll-position binding, or exact platform scrollbar behavior.
- Scroll token tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the text/list/scroll/symbol parser and SSR fixtures now cover `.scrollTargetBehavior(.paging)`, `.scrollTargetLayout()`, `.scrollBounceBehavior(.basedOnSize, axes: .vertical)`, and `.scrollDisabled(...)`, asserting parsed modifier metadata plus rendered class/data breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained scroll token support while keeping native scroll physics/position semantics as follow-up.
- Verification for the scroll token slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 865 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1067 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before scroll token modifiers was constrained `.containerRelativeFrame(...)` layout metadata/CSS-hint coverage.
- ContainerRelativeFrame implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.containerRelativeFrame(...)` with constrained axis, `count:`, `span:`, `spacing:`, and `alignment:` metadata, lowers it to stable `cmux-custom-sidebar-swift-container-relative-frame*` classes, safe CSS `width`/`flex-basis`/`min-height` hints, and `data-swift-container-relative-frame-*` breadcrumbs. This intentionally does **not** claim native SwiftUI container proposal math, exact placement, scroll-target sizing, or GeometryReader-style host geometry.
- ContainerRelativeFrame tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the layout/decorator parser and SSR fixtures now cover `Text("Z").containerRelativeFrame(.horizontal, count: 4, span: 2, spacing: 8, alignment: .center)`, asserting parsed modifier metadata plus rendered class, data breadcrumbs, and CSS sizing output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `.containerRelativeFrame(...)` support while keeping native container/scroll-target math as follow-up.
- Verification for the containerRelativeFrame slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 855 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1057 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before containerRelativeFrame was constrained `Text(_:style:)` / `Text(timerInterval:)` static date/timer coverage.
- Text style/timer implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: Swift text nodes now preserve optional `textStyle` and timer interval metadata, `Text(date, style: .date/.time)` renders through the existing deterministic UTC date/time formatter, and `Text(timerInterval: start...end, countsDown:)` renders a static duration snapshot with `data-swift-text-style`, `data-swift-timer-interval-start-ms`, `data-swift-timer-interval-end-ms`, and `data-swift-timer-counts-down` breadcrumbs. This intentionally does **not** claim native self-updating SwiftUI runloop text, pause-time support, locale/timezone negotiation, live relative ticking, or full date style semantics.
- Text style/timer tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the reduce/numeric-format parser fixture now covers `Text(launched, style: .date)`, `Text(launched, style: .time)`, and `Text(timerInterval: intervalStart...intervalEnd, countsDown: true)`, asserting parsed text metadata and static timer duration; the SSR formatting fixture asserts `data-swift-text-style` and timer interval breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained static text-style/timer support while keeping native live timer/date behavior as follow-up.
- Verification for the Text style/timer slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 845 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1047 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before Text style/timer was constrained `.strokeBorder(...)` shape breadcrumb/CSS-hint coverage.
- StrokeBorder implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts `.strokeBorder(color, lineWidth:)` / `.strokeBorder(color, width:)`, renders it through the existing safe CSS border hint path, and emits `cmux-custom-sidebar-swift-shape-stroke-border` plus `data-swift-shape-stroke`, `data-swift-shape-stroke-color`, and `data-swift-shape-stroke-width` breadcrumbs. This intentionally does **not** claim native insettable-shape stroke placement, true `StrokeStyle`, trim-before-stroke vector geometry, or full `ShapeStyle` payload semantics.
- StrokeBorder tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the layout/decorator parser and SSR fixtures now cover `RoundedRectangle(cornerRadius: 10).strokeBorder(.orange, lineWidth: 4)`, asserting parsed modifier metadata plus rendered class, data breadcrumbs, and CSS border output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `.strokeBorder(...)` support while keeping native shape/vector caveats.
- Verification for the strokeBorder slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 835 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1037 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before strokeBorder was constrained `Image(systemName:)` SF Symbol name/glyph breadcrumb coverage.
- SF Symbol implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: standalone `Image(systemName:)` nodes and system-image icons inside `Label(...)` now emit stable `data-swift-system-image` and `data-swift-system-image-glyph` breadcrumbs alongside the existing accessibility label and constrained text-glyph fallback. This intentionally does **not** claim native SF Symbol vector paths, weights, variable values, palette rendering, or exact symbol glyph semantics.
- SF Symbol tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the expanded Swift SSR fixture now asserts system-image breadcrumbs for `Label(..., systemImage:)`, closure-form `Label(title:icon:)`, and dynamic `Image(systemName:)` rows; the image presentation fixture asserts breadcrumbs for `Image(systemName: "bolt.fill")` alongside existing image-scale/symbol-rendering/symbol-variant classes.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to document constrained SF Symbol breadcrumbs while keeping native SF Symbol rendering as follow-up.
- Verification for the SF Symbol breadcrumb slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 829 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1031 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before SF Symbol breadcrumbs was constrained `AsyncImage(url:)` phase/safe-url breadcrumb coverage.
- AsyncImage implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: valid safe `http`/`https` `AsyncImage(url:)` leaves now render `data-swift-async-image-phase="success"` plus `data-swift-async-image-url` alongside the existing bounded lazy remote `<img>` output. Rejected unsafe URLs render the existing visible placeholder with `data-swift-async-image-phase="failure"` and still do not expose the unsafe URL. This intentionally does **not** claim native content/placeholder/phase closures, true async load lifecycle callbacks, retries, progress, cache state, or phase enum binding.
- AsyncImage tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the image SSR fixture now asserts success/failure phase breadcrumbs, the accepted safe URL breadcrumb, continued lazy/referrer-safe image output, and continued rejection of `file:` URLs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to document constrained AsyncImage breadcrumbs while keeping native closure/phase lifecycle behavior as follow-up.
- Verification for the AsyncImage breadcrumb slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 822 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1024 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before AsyncImage breadcrumbs was constrained `Image(decorative:)` asset accessibility coverage.
- Decorative image implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: loaded and placeholder `Image(decorative:)` asset nodes now consistently strip authored `aria-label` metadata from the rendered asset element and force decorative accessibility semantics (`alt=""` for loaded images plus `aria-hidden="true"`). This keeps normal `Image("name")` asset labels unchanged and still preserves safe `http`/`https` / `cmux-sidebar-asset://` URL gating.
- Decorative image tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the image SSR fixture now renders `Image(decorative: "badge.icon").accessibilityLabel("Decorative badge")`, verifies the safe asset URL, empty alt text, `aria-hidden="true"`, and asserts the authored accessibility label does not leak.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to document constrained decorative asset semantics while keeping native asset catalog lookup as follow-up.
- Verification for the decorative image slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 819 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1021 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before decorative image was constrained `ScrollView(axes, showsIndicators:)` breadcrumb/two-axis coverage.
- ScrollView implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the existing `scrollView` node already carried vertical/horizontal/both axis metadata and `showsIndicators`; render output now also emits stable `data-swift-scroll-axis` and `data-swift-scroll-shows-indicators` breadcrumbs alongside the existing axis/no-indicator classes. This intentionally does **not** claim native paging/snap targets, bounce behavior, scroll-position/reader APIs, scroll physics, or exact platform scrollbar semantics.
- ScrollView tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the scroll parser/SSR fixtures now cover `ScrollView(.horizontal, showsIndicators: false)` and `ScrollView([.horizontal, .vertical], showsIndicators: true)`, asserting parsed axis/indicator metadata plus rendered data breadcrumbs and classes.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to replace stale "promote passthrough" wording with constrained ScrollView support while keeping native scroll-behavior caveats.
- Verification for the ScrollView slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 815 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1017 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before ScrollView was constrained grid-cell modifier coverage for `.gridCellColumns(...)`, `.gridColumnAlignment(...)`, and `.gridCellAnchor(...)`.
- Grid-cell modifier implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift modifier parser now accepts the three grid-cell modifiers, lowers constrained values into stable classes, `data-swift-grid-cell-*` breadcrumbs, and CSS hints (`grid-column: span n`, `justify-self`, `place-self`) through the existing presentation path. This intentionally does **not** claim native SwiftUI grid measurement, exact spanning/alignment negotiation, or full cell placement semantics.
- Grid-cell tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the grid parser/SSR fixtures now apply the three modifiers to a rendered grid child and assert parsed modifier metadata plus rendered classes, data breadcrumbs, and CSS hints.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained grid-cell modifier support while keeping native grid-placement caveats.
- Verification for the grid-cell modifier slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 807 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 1009 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before grid-cell modifiers was constrained `LazyVGrid`/`LazyHGrid` + `GridItem` metadata/CSS-hint coverage.
- Grid implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `Grid` now preserves `gridKind: "grid"`, while `LazyVGrid` and `LazyHGrid` preserve distinct lazy-grid identity, constrained inline/local `[GridItem(.fixed/.flexible/.adaptive)]` summaries, `spacing:`, `pinnedViews:`, stable classes, `data-swift-grid-kind`, `data-swift-grid-items`, and sidebar-local CSS `grid-template-columns` / `grid-template-rows` hints. This intentionally does **not** claim native SwiftUI virtualization, exact `GridItem` sizing/adaptive packing, `.gridCellColumns`, `.gridColumnAlignment`, `.gridCellAnchor`, or full grid alignment/cell-span semantics.
- Grid tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the parser fixture now covers local `let columns = [GridItem(...)]` feeding `LazyVGrid(columns:spacing:)`, and the SSR fixture covers inline `LazyHGrid(rows: [GridItem(...)], spacing:)` with class, data breadcrumb, and CSS grid-template assertions.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained grid/lazy-grid support while keeping native measurement/virtualization/cell-span caveats.
- Verification for the grid/lazy-grid slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 797 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 999 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before grid/lazy-grid was constrained `Spacer(minLength:)` coverage.
- Spacer implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `Spacer(minLength: 24)` now preserves `minLength` on the parsed spacer node and renders a stable `data-swift-spacer-min-length` breadcrumb plus sidebar-local `min-width` / `min-height` CSS hints. This intentionally does **not** claim native SwiftUI layout proposal behavior, axis-specific expansion, or exact spacer negotiation across stack parents.
- Spacer tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the parser fixture now verifies a `Spacer(minLength: 24)` node inside the authored button row, and the expanded authored Swift SSR fixture asserts the spacer class, data breadcrumb, and CSS min-size hints.
- Docs were updated in `docs/custom-sidebars.md` and `docs/swiftui-interpreter-surface.md` to mark constrained `Spacer(minLength:)` support while keeping native axis/layout caveats. No CLI contract change was needed for this UI-renderer-only slice.
- Verification for the spacer slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 788 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 990 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before spacer was constrained static array `.formatted(.list(...))` coverage.
- List formatting implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: static arrays now support `.formatted(.list(type: ...))` and `Text(array, format: .list(type: ...))` through a sidebar-local `Intl.ListFormat("en-US")` bridge. It preserves constrained `.and`/conjunction, `.or`/disjunction, and `.unit` list types plus `.short`/`.narrow` width hints. This intentionally does **not** claim locale negotiation, rich item formatting, attributed list output, arbitrary `FormatStyle` composition, or full Foundation list formatting parity.
- List formatting tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the reduce/numeric-format parser fixture now covers `names.formatted(.list(type: .and))`, `Text(names, format: .list(type: .or))`, and formatted action params; the SSR formatting fixture asserts rendered list output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained list formatting support while keeping richer FormatStyle caveats.
- Verification for the list formatting slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 783 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 985 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before list formatting was constrained deterministic `Date(timeIntervalSince1970:)` / `.formatted(.dateTime...)` coverage.
- Date formatting implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift evaluator now supports a tagged `Date(timeIntervalSince1970:)` value, `Date(timeIntervalSinceReferenceDate:)`, and fallback `Date()` / `Date.now`, and formats it through `.formatted(.dateTime...)` or `Text(date, format: .dateTime...)`. Output is deterministic/sidebar-local UTC for static authored dates, with date fields (`year`/`month`/`day`/`weekday`) and time fields (`hour`/`minute`/`second`) deciding whether date, time, or both are shown. This intentionally does **not** claim live self-updating `Text(_:style:)`, `Text(timerInterval:)`, relative date styles, locale/timezone negotiation, calendars, attributed formatting, or full `FormatStyle` composition.
- Date formatting tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the reduce/numeric-format parser fixture now covers `Date(timeIntervalSince1970: 1704067200).formatted(.dateTime.year().month().day())`, `Text(date, format: .dateTime.hour().minute())`, and formatted action params; the SSR formatting fixture asserts rendered date output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained deterministic date formatting support while keeping live/relative/Foundation caveats.
- Verification for the date formatting slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 780 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 982 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before date formatting was constrained `Measurement(value:unit:)` / `.formatted(.measurement(...))` coverage.
- Measurement formatting implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the Swift evaluator now supports a tagged `Measurement(value:unit:)` value and formats it through `.formatted(.measurement(...))` or `Text(measurement, format: .measurement(...))`. The constrained unit map covers common storage (`bytes`, `kilobytes`, `megabytes`, `gigabytes`), duration (`seconds`, `minutes`, `hours`), length (`meters`, `kilometers`, `miles`, `feet`), mass (`grams`, `kilograms`), and temperature (`celsius`, `fahrenheit`) labels, with abbreviated/default and `.wide` width hints. This intentionally does **not** claim unit conversion, broad Foundation unit catalogs, locale negotiation, measurement arithmetic, attributed formatting, or full `FormatStyle` composition.
- Measurement tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the reduce/numeric-format parser fixture now covers `Measurement(value: 12.5, unit: UnitLength.kilometers).formatted(.measurement(width: .abbreviated))`, `Text(duration, format: .measurement(width: .wide))`, and formatted action params; the SSR formatting fixture asserts rendered measurement output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained measurement formatting support while keeping unit-conversion/Foundation caveats.
- Verification for the measurement formatting slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 777 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 979 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before measurement formatting was constrained `Text(_:format:)` / numeric `.formatted(...)` byte-count coverage.
- Byte-count formatting implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: numeric `.formatted(.byteCount(...))` and `Text(value, format: .byteCount(...))` now render sidebar-local static byte-count strings. File/default style uses decimal units (`KB`, `MB`, etc.); `.memory` / `.binary` hints use binary units (`KiB`, `MiB`, etc.). This intentionally does **not** claim full Swift `FormatStyle`, Foundation locale negotiation, Measurement/unit values, date/relative/list styles, or exact platform byte-count wording.
- Byte-count tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the reduce/numeric-format parser fixture now covers `.formatted(.byteCount(style: .file))`, `Text(portTotal, format: .byteCount(style: .file))`, and formatted action params; the SSR formatting fixture asserts the rendered `1.5 KB` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained byte-count formatting support while keeping broader `FormatStyle` caveats.
- Verification for the byte-count formatting slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 774 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 976 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before byte-count formatting was constrained collection-helper / conversion / `String(format:)` coverage.
- Collection/conversion implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: existing constrained support for `.enumerated()`, `.prefix`, `.suffix`, `.dropFirst`, `.dropLast`, `.reversed`, collection transforms, `min`/`max`/`abs`, `Int`/`Double`/`String` casts, and common numeric `.formatted(...)` styles was extended with a sidebar-local `String(format:)` subset. The formatter supports common `%d`/`%i`/`%u`/`%o`/`%x`/`%X`, `%f`/`%F`, `%e`/`%E`, `%g`/`%G`, `%@`/`%s`, `%%`, precision, width, left alignment, and zero padding. This intentionally does **not** claim full Foundation locale behavior, broad printf length modifiers/types, localized formatting, or every collection derivation.
- Collection/conversion tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the reduce/numeric-format parser fixture now covers `String(format: "%.1f%%", ratio * 100)`, `String(format: "%03d %@", arguments: [workspaceCount, selectedTitle])`, and formatted action params; the SSR formatting fixture asserts the rendered formatted string output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained collection/conversion/String-format support while keeping Foundation/printf caveats.
- Verification for the collection/conversion slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 771 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 973 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before collection/conversion was constrained `.contentShape(...)` / counted tap / long-press gesture breadcrumb coverage.
- Gesture/content-shape implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.contentShape(...)` now preserves a normalized `contentShape` presentation token and renders `data-swift-content-shape` alongside existing stable shape classes. `.onTapGesture(count:)` / `.onLongPressGesture` action wrappers now render Swift-style `data-swift-gesture`, `data-swift-tap-count`, and `data-swift-long-press-duration-ms` breadcrumbs alongside the existing button affordance classes. This intentionally does **not** claim native SwiftUI hit-test geometry, gesture priority/composition, gesture value payloads, or full native recognizer semantics.
- Gesture/content-shape tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the static navigation/toolbar SSR fixture now asserts `data-swift-content-shape="capsule"`, and the tap-count/long-press SSR fixture asserts the new gesture/count/duration breadcrumbs.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained content-shape/tap-count/long-press support while keeping native gesture/hit-test caveats.
- Verification for the gesture/content-shape slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 768 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 970 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before gesture/content-shape was constrained static toolbar/navigation chrome placement coverage.
- Toolbar/navigation implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `ToolbarItem(placement: .primaryAction)` now preserves a constrained `toolbarPlacement` token on the interpreted group node and renders stable `cmux-custom-sidebar-swift-toolbar-item-*` classes plus `data-swift-toolbar-placement`, while existing `.navigationTitle(...)`, `.navigationSubtitle(...)`, and `.navigationBarTitleDisplayMode(...)` chrome behavior remains intact. This intentionally does **not** claim native platform toolbar slotting, automatic placement/collapse, item ordering, customization, or platform toolbar semantics.
- Toolbar/navigation tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the static navigation/toolbar parser fixture now verifies `groupRole: "toolbarItem"` and `toolbarPlacement: "primaryAction"`, and the SSR fixture asserts `cmux-custom-sidebar-swift-toolbar-item-primaryAction` plus `data-swift-toolbar-placement="primaryAction"`.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained toolbar/navigation chrome support while keeping native toolbar semantics caveats.
- Verification for the toolbar/navigation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 763 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 965 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before toolbar/navigation was constrained static `.draggable(...)` payload breadcrumb coverage.
- Static draggable implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: constrained `.draggable(selectedId)` / static scalar payloads still render browser `draggable=true`, and now also expose the resolved payload as `data-swift-draggable` alongside the stable `cmux-custom-sidebar-swift-draggable` class. This intentionally does **not** claim native SwiftUI drag sessions, `Transferable`, item providers, preview/content closures, payload binding, or typed drag/drop semantics.
- Static draggable tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the static navigation/toolbar SSR fixture now asserts `data-swift-draggable="workspace-a"` in addition to the existing class and `draggable="true"` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained static draggable support while keeping native drag/session/Transferable caveats.
- Verification for the static draggable slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 761 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 963 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before static draggable was constrained `Label(title:icon:)` closure initializer coverage.
- Label closure implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `Label(title: { Text(...) }, icon: { Image(systemName:) })` now lowers through the existing label node by extracting a single constrained text title and SF Symbol image icon. Existing `Label("Title", systemImage:)` behavior remains unchanged. This intentionally does **not** claim arbitrary rich title/icon builders, asset-image icons in labels, or full platform label-style semantics.
- Label closure tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the list/section/label parser and SSR fixtures now include `Label(title: { Text("Route") }, icon: { Image(systemName: "arrow.right") })`, verify the parsed label node, and assert rendered title/icon accessibility output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained closure-form Label support while keeping rich-builder/platform semantics caveats.
- Verification for the Label closure slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 760 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 962 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before Label closure support was explicit `.truncationMode(...)` / `.multilineTextAlignment(...)` / `.textCase(...)` coverage.
- Text mode implementation was already present in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: constrained `.truncationMode(.head|.middle|.tail)`, `.multilineTextAlignment(.leading|.center|.trailing)`, and `.textCase(.uppercase|.lowercase)` lower to stable classes/CSS hints. This slice added explicit coverage for less-common tokens and corrected the stale SwiftUI surface matrix. This intentionally does **not** claim exact native truncation algorithms, full token coverage, or SwiftUI text layout parity.
- Text mode tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the text/list/scroll/symbol presentation parser/SSR fixture now covers `.truncationMode(.head)`, `.truncationMode(.middle)`, `.textCase(.lowercase)`, and `.multilineTextAlignment(.trailing)`, verifying metadata and rendered class/style output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained truncation/alignment/case support while keeping native truncation/text-layout caveats.
- Verification for the text-mode slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 758 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 960 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before text modes was constrained `.underline(..., pattern:color:)` / `.strikethrough(..., pattern:color:)` typography coverage.
- Text decoration implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.underline(active, pattern:, color:)` and `.strikethrough(active, pattern:, color:)` now preserve constrained active/pattern/color metadata, emit stable pattern classes such as `cmux-custom-sidebar-swift-underline-dash` / `cmux-custom-sidebar-swift-strikethrough-dot`, and lower color/pattern to CSS `text-decoration-color` / `text-decoration-style` hints. This intentionally does **not** claim per-decoration style/color preservation when underline and strikethrough are combined on one element, exact SwiftUI line styling, or full attributed-run semantics.
- Text decoration tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the text/list/scroll/symbol presentation parser/SSR fixture now covers `.underline(true, pattern: .dash, color: .mint)` and `.strikethrough(true, pattern: .dot, color: .red)`, verifies metadata, stable pattern classes, `text-decoration-style:dotted`, and `text-decoration-color:#fda4af`.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained decoration pattern/color support while keeping combined-decoration/native text-run caveats.
- Verification for the text-decoration slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 752 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 954 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before text decorations was constrained `.lineLimit(..., reservesSpace:)` typography coverage.
- Line-limit reserves-space implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: numeric `.lineLimit(n, reservesSpace: true)` now preserves `boolValue: true`, renders stable `cmux-custom-sidebar-swift-line-limit-reserves-space`, and applies a sidebar-local CSS `min-height: calc(n * 1.35em)` hint alongside the existing line clamp. This intentionally does **not** claim nil/range line-limit forms, exact SwiftUI text metrics, native line box reservation, or font-engine parity.
- Line-limit reserves-space tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf modifier parser/SSR fixture now uses `.lineLimit(2, reservesSpace: true)`, verifies `{ name: "lineLimit", value: "2", boolValue: true }`, and asserts the stable class plus `min-height:calc(2 * 1.35em)` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `reservesSpace:` support while keeping nil/range/native metrics caveats.
- Verification for the line-limit reserves-space slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 748 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 950 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before line-limit reserves-space was constrained `.monospacedDigit()` typography coverage.
- Monospaced-digit implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.monospacedDigit()` now preserves distinct modifier metadata instead of collapsing to `.monospaced()`, renders stable `cmux-custom-sidebar-swift-monospaced-digit`, and lowers to the CSS `font-variant-numeric: tabular-nums` hint. This intentionally does **not** claim native SwiftUI font feature availability, exact numeric glyph metrics, or full font-engine parity.
- Monospaced-digit tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf modifier parser/SSR fixture now includes `.monospacedDigit()`, verifies `{ name: "monospacedDigit" }`, and asserts the stable class plus `font-variant-numeric:tabular-nums` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `.monospacedDigit()` support while keeping font-feature caveats.
- Verification for the monospaced-digit slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 746 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 948 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before monospaced digit was explicit image `.scaledToFit()` / `.scaledToFill()` coverage.
- Image scale-fit/fill implementation was already present in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.scaledToFit()` and `.scaledToFill()` lower to `aspectRatio` presentation metadata and stable `cmux-custom-sidebar-swift-aspect-fit` / `cmux-custom-sidebar-swift-aspect-fill` classes. This slice added explicit parser/SSR coverage and corrected docs. This intentionally does **not** claim native SwiftUI image proposal sizing, exact object-fit behavior, or full image layout negotiation.
- Image scale-fit/fill tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the text/list/scroll/symbol presentation parser/SSR fixtures now cover `.scaledToFit()` on a system image and `.scaledToFill()` on `AsyncImage`, verifying `aspectRatio` metadata and stable fit/fill classes.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark `.scaledToFit()` / `.scaledToFill()` as constrained partial support instead of missing.
- Verification for the image scale-fit/fill slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 744 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 946 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before image scale-fit/fill was constrained `Text(...) + Text(...)` concatenation coverage.
- Text concatenation implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: the statement reader now carries top-level `+` chains between view calls, and the parser folds constrained `Text(...) + Text(...)` expressions into a single sidebar text node, including safe concatenation of existing markdown run metadata when present. This intentionally does **not** claim native attributed-run modifier boundary preservation, a richer text-run IR, arbitrary non-Text view concatenation, or full SwiftUI `Text` operator overload behavior.
- Text concatenation tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the text markdown/verbatim parser and SSR fixtures now include `Text("Open ") + Text(selectedTitle)`, verify the merged `Open Ops` text node, and assert rendered output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `Text + Text` support while keeping attributed-run caveats.
- Verification for the text-concatenation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 742 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 944 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before text concatenation was constrained `.dynamicTypeSize(...)` typography coverage.
- Dynamic type implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.dynamicTypeSize(.xSmall|.small|.medium|.large|.xLarge|.xxLarge|.xxxLarge|.accessibility1...5)` now parses as safe modifier metadata, emits stable `cmux-custom-sidebar-swift-dynamic-type-*` classes plus `data-swift-dynamic-type-size`, and maps common size-category tokens to CSS `font-size` hints. This intentionally does **not** claim range syntax, inherited host/user Dynamic Type propagation, exact SwiftUI scaling metrics, `@ScaledMetric`, or native environment behavior.
- Dynamic type tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the richer leaf modifier parser/SSR fixture now covers `.dynamicTypeSize(.accessibility2)`, verifies modifier metadata, stable class/data breadcrumbs, and the `font-size:1.78rem` CSS hint.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained dynamic type support while keeping native Dynamic Type caveats.
- Verification for the dynamic-type slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 740 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 942 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before dynamic type was constrained `Group` / `EmptyView()` semantic coverage.
- Group/EmptyView implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: authored `Group { ... }` now preserves `groupRole: "group"`, renders with stable `cmux-custom-sidebar-swift-group` and `data-swift-group="true"` breadcrumbs, and uses `display: contents` as a sidebar-local layout-transparent approximation. `EmptyView()` remains an intentional no-op node. This intentionally does **not** claim native SwiftUI Group modifier fan-out to each child or full layout/proposal semantics.
- Group/EmptyView tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the wrapper parser/SSR fixture now includes `Group { Text("Grouped") }` and `EmptyView()`, verifies parser metadata/no-op parsing, and asserts the stable semantic group breadcrumbs plus rendered grouped child.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to mark constrained `Group` / `EmptyView()` support while keeping modifier fan-out caveats.
- Verification for the Group/EmptyView slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 737 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 939 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before Group/EmptyView was constrained `ViewThatFits(in:)` semantic-container coverage.
- ViewThatFits implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `ViewThatFits(in:)` now preserves `groupRole: "viewThatFits"`, optional `fitAxis`, stable `cmux-custom-sidebar-swift-view-that-fits*` classes, `data-swift-view-that-fits`, and sidebar-local CSS layout hints. This intentionally does **not** claim native SwiftUI fit measurement, hiding of non-fitting alternatives, or exact proposed-layout negotiation.
- ViewThatFits tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the existing Gauge/AnyView/ViewThatFits parser and SSR fixtures now cover `ViewThatFits(in: .horizontal)`, verify `fitAxis: "horizontal"`, stable classes, and the data breadcrumb.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained `ViewThatFits(in:)` semantic support while keeping native fit-measurement caveats.
- Verification for the ViewThatFits slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 731 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 933 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before ViewThatFits was constrained `.compositingGroup()` modifier coverage.
- Compositing-group implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.compositingGroup()` now parses as a safe modifier, renders `cmux-custom-sidebar-swift-compositing-group`, and applies CSS `isolation:isolate` as a sidebar-local compositing boundary. This intentionally does **not** claim native SwiftUI offscreen rasterization, exact color-space behavior, drawing-group semantics, or full isolated blend-stack parity.
- Compositing-group tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the existing layout/decorator parser and SSR fixture now includes `.compositingGroup()`, verifies modifier metadata, and asserts the stable class plus `isolation:isolate` output alongside existing blend/filter/transform coverage.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained compositing-group support while keeping native rasterization/color-space caveats.
- Verification for the compositing-group slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 728 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 930 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before compositing-group was constrained `ContainerRelativeShape` shape coverage.
- ContainerRelativeShape implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `ContainerRelativeShape()` now parses as a safe shape node, renders through the existing shape element path, exposes `cmux-custom-sidebar-swift-shape-containerRelativeShape`, and uses a sidebar-local rounded-container CSS approximation. This intentionally does **not** claim native SwiftUI container-radius reads, inherited container geometry, `RoundedRectangle(style:)` / `Capsule(style:)` style metadata, or exact vector path semantics.
- ContainerRelativeShape tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the existing if-let/shapes/grids/zstacks/menus parser and SSR fixtures now include `ContainerRelativeShape().foregroundColor(.teal)`, verify the parsed `shape: "containerRelativeShape"` node, and assert the stable shape class renders.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained `ContainerRelativeShape` support while keeping native container-geometry caveats.
- Verification for the ContainerRelativeShape slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 726 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 928 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before ContainerRelativeShape was constrained `.fontWidth(...)` typography modifier coverage.
- Font width implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: constrained built-in `.fontWidth(.compressed|.condensed|.standard|.expanded)` tokens now parse as safe presentation metadata, lower to CSS `font-stretch` hints (`75%`, `87.5%`, `normal`, `112.5%`), and emit stable `cmux-custom-sidebar-swift-font-width-*` classes. This intentionally does **not** claim exact SwiftUI font metrics, native variable-font availability, custom font-width values, or full `Font.Width` behavior.
- Font width tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the existing richer leaf modifier parser/SSR fixture now verifies `.fontWidth(.condensed)` metadata, `cmux-custom-sidebar-swift-font-width-condensed`, and `font-stretch:87.5%` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained font-width support while keeping native font metrics/availability caveats.
- Verification for the font-width slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 724 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 926 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before font width was constrained text spacing modifier coverage for `.tracking(...)`, `.kerning(...)`, and `.baselineOffset(...)`.
- Text spacing implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: constrained scalar `.tracking(...)` and `.kerning(...)` modifiers now parse as safe text presentation metadata, lower to CSS `letter-spacing`, and emit stable `cmux-custom-sidebar-swift-tracking` / `cmux-custom-sidebar-swift-kerning` classes. `.baselineOffset(...)` lowers to a CSS `vertical-align` hint and emits `cmux-custom-sidebar-swift-baseline-offset`. This intentionally does **not** claim exact SwiftUI text-run layout, font-engine metrics, attributed-run composition, or native baseline layout behavior.
- Text spacing tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the existing common SwiftUI modifier parser/SSR fixture now verifies `.tracking(1.5)`, `.kerning(2)`, and `.baselineOffset(3)` metadata plus the stable classes and CSS `letter-spacing` / `vertical-align` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained typography spacing support while keeping native text-layout caveats.
- Verification for the text-spacing slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 722 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 924 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before text spacing was constrained `ControlGroup { ... }` grouped-control container coverage.
- ControlGroup implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `ControlGroup { ... }` now preserves `groupRole: "controlGroup"` on the existing group node, renders children in a compact horizontal sidebar-local grouped-control container, and exposes `cmux-custom-sidebar-swift-control-group` plus `data-swift-control-group="true"` breadcrumbs. This intentionally does **not** claim native segmented controls, toolbar grouping, platform control-group styling, rich menu controls, or full platform menu semantics.
- ControlGroup tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the existing if-let/shapes/grids/zstacks/menus parser and SSR fixtures now include a `ControlGroup` with safe `Button` actions, verify `groupRole: "controlGroup"`, rendered child labels, the stable class, and the `data-swift-control-group` breadcrumb.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained sidebar-local `ControlGroup` support while keeping native/platform caveats.
- Verification for the ControlGroup slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 713 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 915 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before ControlGroup was constrained `.keyboardShortcut(..., modifiers:)` metadata coverage.
- Keyboard shortcut modifier implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.keyboardShortcut("key", modifiers: [.command, .shift, .option, .control])` now preserves a normalized modifier list, renders `aria-keyshortcuts` using ARIA modifier names (`Meta`, `Shift`, `Alt`, `Control`), and emits stable `cmux-custom-sidebar-swift-keyboard-shortcut-*` modifier classes. Existing plain `.keyboardShortcut("key")` behavior remains available. This intentionally does **not** claim native/global accelerator registration, full `KeyEquivalent` members, full `EventModifiers` OptionSet semantics, or host command dispatch.
- Keyboard shortcut modifier tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: existing static navigation/toolbar parser and SSR coverage now includes `.keyboardShortcut("b", modifiers: [.command, .shift])`, verifies `secondaryValue: "command,shift"`, `aria-keyshortcuts="Meta+Shift+b"`, and the command/shift classes.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained keyboard shortcut modifier support while keeping native accelerator caveats.
- Verification for the keyboard-shortcut-modifiers slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 708 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 910 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before keyboard shortcut modifiers was constrained `.preferredColorScheme(...)` / `.environment(...)` render-preference metadata coverage.
- Environment-preference implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.preferredColorScheme(.dark|.light)`, `.environment(\.colorScheme, .dark|.light)`, and `.environment(\.layoutDirection, .rightToLeft|.leftToRight)` now parse as safe modifiers, emit stable `cmux-custom-sidebar-swift-preferred-color-scheme-*` / `cmux-custom-sidebar-swift-environment-*` classes, expose `data-swift-preferred-color-scheme`, `data-swift-environment-color-scheme`, and `data-swift-environment-layout-direction` breadcrumbs, and apply CSS `color-scheme` / `direction` hints on the rendered node. This intentionally does **not** claim inherited SwiftUI environment propagation, arbitrary environment keys, `@Environment` reads, locale/layout-direction mirroring semantics, or native platform color-scheme behavior.
- Environment-preference tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: existing richer leaf modifier parser/SSR coverage now includes `.preferredColorScheme(.dark)`, `.environment(\.colorScheme, .light)`, and `.environment(\.layoutDirection, .rightToLeft)` metadata plus class/data/style output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained environment-preference support while keeping full environment-propagation caveats.
- Verification for the environment-preference slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 706 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 908 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before environment preferences was constrained `.symbolEffectsRemoved()` metadata/reset coverage.
- Symbol-effects-removed implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.symbolEffectsRemoved()` / `.symbolEffectsRemoved(true)` now parses as a safe modifier, clears previous `symbolEffect` name/value/active metadata in the same modifier chain, filters stale `cmux-custom-sidebar-swift-symbol-effect-*` classes, and emits `cmux-custom-sidebar-swift-symbol-effects-removed` plus `data-swift-symbol-effects-removed="true"`. `.symbolEffectsRemoved(false)` is a no-op. This intentionally does **not** claim real SF Symbol animation runtime behavior, native symbol rendering, or state-driven animation diffing.
- Symbol-effects-removed tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: existing richer leaf modifier parser/SSR coverage now includes a `.symbolEffect(.bounce, isActive: true).symbolEffectsRemoved()` chain, verifies removal metadata, and asserts the stale bounce symbol-effect class/data breadcrumbs are absent while the separate pulse symbol-effect fixture still renders.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained `.symbolEffectsRemoved()` support while keeping animation-runtime caveats.
- Verification for the symbol-effects-removed slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 698 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 900 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before `.symbolEffectsRemoved()` was constrained image `.resizable(capInsets:resizingMode:)` metadata coverage.
- Image cap-inset implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.resizable(capInsets: EdgeInsets(top:leading:bottom:trailing:), resizingMode: .stretch|.tile)` now preserves numeric cap-inset metadata and resizing mode on the existing `resizable` modifier, renders stable `cmux-custom-sidebar-swift-image-resizing-*` / `cmux-custom-sidebar-swift-image-cap-insets` classes, and exposes safe CSS custom properties for each inset. This intentionally does **not** claim native SwiftUI nine-slice or tiled image drawing, `ImagePaint`, arbitrary `EdgeInsets` expressions, or full pre-AnyView `ImageConfig` semantics.
- Image cap-inset tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: existing text/list/scroll/symbol presentation parser and SSR coverage now include `EdgeInsets(top: 2, leading: 4, bottom: 6, trailing: 8)`, `.tile`, the resizing/cap-inset classes, and inset CSS variables.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained image cap-inset/resizing-mode support while keeping native image-slicing caveats.
- Verification for the image cap-inset slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 693 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 895 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before image cap insets was constrained `.rotation3DEffect(...)` transform modifier coverage.
- Rotation 3D implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.rotation3DEffect(.degrees(...), axis: (x:y:z:), anchor:, perspective:)` now preserves scalar angle/axis/anchor/perspective metadata and renders through the existing safe CSS transform pipeline as optional `perspective(...)`, `rotate3d(...)`, `transform-origin`, `cmux-custom-sidebar-swift-rotation3d`, and anchor classes. This intentionally does **not** claim full SwiftUI `UnitPoint`, `anchorZ`, structured geometry values, native perspective math, or native 3D layout semantics.
- Rotation 3D tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: existing layout/decorator parser and SSR coverage now include `.rotation3DEffect(.degrees(30), axis: (x: 0, y: 1, z: 0), anchor: .topLeading, perspective: 0.7)` metadata, class breadcrumbs, `transform-origin`, and combined transform output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list constrained `.rotation3DEffect` support while keeping broader geometry/native-layout caveats.
- Verification for the rotation-3D slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 687 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 889 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before `.rotation3DEffect` was constrained `.position(x:y:)` layout modifier coverage.
- Position implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.position(x:y:)` now preserves scalar x/y metadata and renders as a safe CSS placement hint with `position: relative`, `left`, `top`, and `cmux-custom-sidebar-swift-positioned`. This intentionally does **not** claim SwiftUI center-position layout, `CGPoint`/`UnitPoint` geometry parity, anchors, or native proposed-layout semantics.
- Position tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: existing layout/decorator parser and SSR coverage now include `.position(x: 12, y: 24)` metadata plus the stable positioned class and CSS `left`/`top` output.
- Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md` to list `.position` as supported while keeping broader geometry/native-layout caveats.
- Verification for the position slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 684 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 886 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed verified slice before `.position` was constrained scroll chrome modifier coverage.
- Scroll chrome implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.scrollIndicators(.hidden, axes:)` now preserves visibility and axis metadata as stable `cmux-custom-sidebar-swift-scroll-indicators-*` classes with scrollbar-hiding CSS hints, and `.scrollClipDisabled()` now preserves a boolean modifier and emits `cmux-custom-sidebar-swift-scroll-clip-disabled` with a safe overflow hint. Existing `.scrollContentBackground(.hidden)` and scroll-view `showsIndicators: false` behavior remain unchanged. This intentionally does **not** claim native paging/snap targets, bounce behavior, scroll physics, or exact platform scrollbar semantics.
- Scroll chrome tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies `.scrollIndicators(.hidden, axes: .vertical)` and `.scrollClipDisabled()` metadata on a list, and SSR coverage verifies the stable scroll-indicator axis and clip-disabled classes while keeping existing list style, scroll content background, image presentation, and symbol modifier behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the scroll-chrome slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 681 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 883 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the scroll-chrome slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, multi-arg `foregroundStyle`, broader `StyleValue` integration, `Group`/`EmptyView` explicit matrix cleanup, or a broader `CmuxSwiftRender` gap audit.
- Latest completed slice before this was constrained `ForEach(data:id:)` row identity breadcrumbs, fully verified.
- ForEach identity implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `ForEach(data, id:)` now resolves constrained scalar/key-path ids such as `\.offset`, `\.id`, and `\.self`, then stamps generated top-level rows with the existing `.id(...)` modifier metadata so rendered rows expose `data-swift-id` breadcrumbs. Existing flat-splice row generation remains unchanged. This intentionally does **not** claim SwiftUI row identity diffing, move animations, native reconciliation, or wrapper/list cell identity semantics.
- ForEach identity tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies generated row `.id` modifiers for `ForEach(Array(workspaces.enumerated()), id: \.offset)`, and SSR coverage verifies `data-swift-id="0"`, `"1"`, and `"2"` while keeping existing array helper, dropped/prefix, and for-loop behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the ForEach-id slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 678 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 880 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before ForEach-id was constrained `List(data,id:)` row expansion, fully verified.
- Data-driven List implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `List(data, id:) { item in ... }` now evaluates sequence expressions already supported by the Swift subset, expands rows through the trailing closure using the existing loop scope mechanics, and preserves the `id:` token as `dataId`, `cmux-custom-sidebar-swift-list-data`, and `data-swift-list-id` metadata. Plain trailing-closure `List { ... }` behavior is unchanged. This intentionally does **not** claim list selection, edit mode, SwiftUI identity/diffing semantics, or native platform list chrome.
- Data-driven List tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies `List(workspaces, id: \.id) { workspace in ... }` row expansion and `dataId`, and SSR coverage verifies `cmux-custom-sidebar-swift-list-data`, `data-swift-list-id`, and rendered rows while keeping existing List/Section/Label/ProgressView/ForEach behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the data-list slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 675 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 877 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before data-list was constrained `Section(header:footer:)` support, fully verified.
- Section header/footer implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `Section("Title") { ... }` keeps the existing title path, while constrained single-view-expression `Section(header: Text(...), footer: Text(...)) { ... }` now preserves header/footer child nodes, collects their event handlers, and renders dedicated header/body/footer slots through the safe Swift node path. This intentionally does **not** claim arbitrary `@ViewBuilder` header/footer closures, list selection/edit mode, or native platform list chrome.
- Section header/footer tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies header/footer child metadata and body content, and SSR coverage verifies section header/footer classes plus rendered header/footer text while keeping existing List/Section/Label/ProgressView/ForEach behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the section-header/footer slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 669 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 871 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before section header/footer was constrained `LazyVStack`/`LazyHStack(pinnedViews:)` metadata/classes, fully verified.
- Lazy stack implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `LazyVStack`/`LazyHStack` now preserve lazy intent, constrained `alignment`/`spacing`, and accepted `pinnedViews:` tokens (`sectionHeaders`, `sectionFooters`) as node metadata, stable `cmux-custom-sidebar-swift-stack-lazy` / `cmux-custom-sidebar-swift-pinned-*` classes, and `data-swift-pinned-views` breadcrumbs. This intentionally does **not** claim native virtualization, sticky section headers/footers, pinned scroll physics, or native lazy stack identity/layout semantics.
- Lazy stack tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies `LazyHStack(alignment: .top, spacing: 14, pinnedViews: [.sectionHeaders])` metadata inside `ScrollView(.horizontal, showsIndicators: false)`, and SSR coverage verifies lazy/pinned/alignment classes plus `data-swift-pinned-views` while keeping scroll, button role, and list-row modifier behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the lazy-stack slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 664 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 866 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before lazy stacks was constrained `VStack`/`HStack`/`ZStack(alignment:)` CSS alignment hints, fully verified.
- Stack alignment implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: eager/lazy `VStack`/`HStack` plus `ZStack` now preserve constrained SwiftUI alignment tokens (`leading`, `trailing`, `center`, `top`, `bottom`, and corner variants) as node `alignment`, emit stable `cmux-custom-sidebar-swift-stack-alignment-*` classes, and lower them to axis-aware CSS alignment hints (`align-items`/`justify-content` for stacks, `place-items` for ZStack). This intentionally does **not** claim full SwiftUI alignment guides, baseline alignment, RTL-aware leading/trailing mirroring, native layout negotiation, or exact wrapper semantics.
- Stack alignment tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies `VStack(alignment: .leading, spacing: 8)` and `HStack(alignment: .top)` metadata, and SSR coverage verifies stable stack-alignment classes, `gap:8px`, and `align-items:flex-start` output while keeping frame alignment/dimension/fill behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the stack-alignment slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 659 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 861 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before stack alignment was constrained `.frame(..., alignment:)` CSS placement hints, fully verified.
- Frame alignment implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.frame(..., alignment:)` now preserves constrained SwiftUI alignment tokens (`leading`, `trailing`, `center`, `top`, `bottom`, and corner variants) as `frameAlignment`, emits stable `cmux-custom-sidebar-swift-frame-*` classes, and lowers them to safe CSS `text-align` plus flex `justify-content` / `align-items` hints. This intentionally does **not** claim full SwiftUI proposed-size negotiation, wrapper placement, or native layout semantics.
- Frame alignment tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies `alignment: .trailing` metadata on fixed and fill frames, and SSR coverage verifies the stable frame class plus `text-align:right` / `justify-content:flex-end` output while keeping frame dimension and fill behavior intact. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the frame-alignment slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 652 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 854 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before frame alignment was constrained `.frame(...)` dimension coverage, fully verified.
- Frame dimension implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.frame(width:height:minWidth:minHeight:idealWidth:idealHeight:maxWidth:maxHeight:)` now preserves constrained numeric dimensions and lowers them to safe CSS sizing fields, while `maxWidth: .infinity` continues to render as the sidebar fill class. `idealWidth`/`idealHeight` act as preferred CSS dimensions only when fixed width/height are absent. This is not full SwiftUI proposed-size/ideal-size negotiation, wrapper placement, or native layout semantics.
- Frame dimension tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies width/height/min/ideal/max metadata, and SSR coverage verifies width/height/min-width/max-height output alongside existing fill behavior. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the frame-dimension slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 649 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 851 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Latest completed slice before frame dimensions was constrained edge-set `.padding(...)` layout coverage, fully verified.
- Edge-set padding implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.padding(8)` still applies all-side padding, while `.padding(.horizontal, 6)`, `.padding(.vertical, 6)`, `.padding([.top, .bottom], 4)`, and simple edge tokens now lower to side-specific safe CSS padding fields. This is not full `EdgeInsets(...)`, RTL-aware leading/trailing mirroring, or native SwiftUI layout negotiation.
- Edge-set padding tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies horizontal and top/bottom array edge metadata, and SSR coverage verifies `padding-left/right/top/bottom` output alongside existing all-side padding. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the edge-padding slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 644 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 846 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the edge-padding slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, `frame(width:height:)`/ideal dimensions, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest completed slice before this was constrained `.hueRotation` / `.blendMode` visual modifier coverage, fully verified.
- Hue/blend implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.hueRotation(.degrees(...))` now lowers through the safe filter pipeline as `hue-rotate(...)`, and `.blendMode(...)` maps known SwiftUI blend tokens through a fixed CSS `mix-blend-mode` allow-list with stable `cmux-custom-sidebar-swift-blend-*` classes. This is not native SwiftUI compositing groups, exact color-space behavior, isolated blend stacks, or full blend parity.
- Hue/blend tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: layout/decorator parser coverage verifies `.hueRotation(.degrees(90))` and `.blendMode(.screen)` metadata, and SSR coverage verifies `hue-rotate(90deg)`, `mix-blend-mode:screen`, and the blend class. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the hue/blend slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 640 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 842 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the hue/blend slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest completed slice before this was constrained hierarchical/named foreground palette coverage, fully verified.
- Hierarchical foreground palette implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.foregroundColor(...)`, `.foregroundStyle(...)`, `.fill(...)`, and `.tint(...)` now map constrained hierarchy tokens (`primary`, `secondary`, `tertiary`, `quaternary`, `quinary`), `accent`/`accentColor`, and named SwiftUI-adjacent colors (`mint`, `indigo`, `brown`) to safe sidebar colors. Hierarchical foreground tokens also expose stable `cmux-custom-sidebar-swift-foreground-*` classes. This is not full inherited SwiftUI `ShapeStyle`, multi-arg/layered `foregroundStyle`, environment propagation, or native style resolution.
- Hierarchical foreground tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies `.foregroundStyle(.tertiary)` and `.tint(.accent)` metadata, and SSR coverage verifies `tertiary` class/color plus accent color rendering. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the hierarchical foreground slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 638 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 840 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the hierarchical foreground slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest completed slice before this was constrained CSS-backed Material background coverage, fully verified.
- Material background implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.background(.ultraThinMaterial)`, `.background(.thinMaterial)`, `.background(.regularMaterial)`, `.background(.thickMaterial)`, `.background(.ultraThickMaterial)`, and `.background(.bar)` now render as safe translucent CSS backgrounds with blur/saturation and stable `cmux-custom-sidebar-swift-material-*` classes. This is not native SwiftUI vibrancy/material blending, host-window sampling, full `StyleValue` integration, foreground/material shape styles, or platform material semantics.
- Material tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: the Swift gradient/style SSR fixture verifies `.regularMaterial` and `.bar` background classes, CSS gradient output, and backdrop filtering. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the Material slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 633 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 835 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Material slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, broader `StyleValue` integration for hierarchical/material foreground styles, or a broader `CmuxSwiftRender` gap audit.
- `LabeledContent` implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `LabeledContent("Title", value: expr)` and `LabeledContent("Title") { <value views> }` parse as first-class `labeledContent` nodes and render as compact sidebar-local inspector rows with label/value slots, `cmux-custom-sidebar-swift-labeled-content*` classes, and `data-swift-labeled-content` breadcrumbs. This is not every SwiftUI initializer overload, named-label trailing closures, format-style semantics, or editable form behavior.
- `LabeledContent` tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies value and trailing-content forms, and SSR coverage verifies row classes/data breadcrumbs plus nested child rendering. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the `LabeledContent` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 628 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 830 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `LabeledContent` slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, named-label `LabeledContent` overloads, or a broader `CmuxSwiftRender` gap audit.
- `.dropDestination` implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.dropDestination(for:) { cmux(...) }` marks rendered nodes as browser drop targets, prevents default browser drop handling, invokes the authored precomputed safe action on drop, and exposes `data-swift-drop-destination` plus drop-destination classes keyed by the constrained type token. This is not typed `Transferable` payload binding, dropped item/location binding, native drop highlighting, or full drop result semantics.
- `.dropDestination` tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: static navigation/toolbar parser coverage verifies type-token and safe action capture, and SSR coverage verifies draggable/drop-target classes plus `data-swift-drop-destination`. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the drop-destination slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 617 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 819 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the drop-destination slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, or a broader `CmuxSwiftRender` gap audit.
- `.refreshable` / `.swipeActions` implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx` and `apps/desktop/web/src/styles.css`: `.refreshable { cmux(...) }` renders a sidebar-local Refresh button that invokes the authored safe action, while `.swipeActions(edge:allowsFullSwipe:) { ... }` renders a sidebar-local action tray from authored child views/buttons with `data-swift-swipe-*` breadcrumbs. This is not native pull-to-refresh physics, platform swipe gestures, edit actions, or full list-row integration.
- `.refreshable` / `.swipeActions` tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: child-modifier parser coverage verifies action capture, edge metadata, and destructive child buttons; SSR coverage verifies the refresh button, swipe tray classes, and data breadcrumbs. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the refresh/swipe slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 614 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 816 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the refresh/swipe slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, constrained drag/drop metadata, `.dropDestination` command affordances, or a broader `CmuxSwiftRender` gap audit.
- `.focusable` / `.focused` implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: `.focusable()` makes rendered nodes keyboard-focusable with stable metadata, `.focused($localBool)` mirrors browser focus/blur into direct local `@State` Bool bindings, and rendered nodes expose `data-swift-focusable`, `data-swift-focused`, `data-swift-focused-binding`, plus focusable/focused classes. This is not full `@FocusState`, programmatic focus ownership, focus scopes, or native focus propagation.
- `.focusable` / `.focused` tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies direct local focus bindings, and SSR coverage verifies focus breadcrumbs/classes. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the focus slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 606 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 808 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the focus slice: static swipe/refresh action affordances, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, constrained drag/drop metadata, or a broader `CmuxSwiftRender` gap audit.
- `.onHover` implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: it parses a single hover boolean closure parameter, precomputes enter/leave local handlers by binding that parameter to `true`/`false`, runs them from browser `onMouseEnter`/`onMouseLeave`, and exposes `data-swift-on-hover` plus `cmux-custom-sidebar-swift-hoverable` breadcrumbs. This is not full gesture composition, drag/drop payloads, focusable action support, or native pointer-region semantics.
- `.onHover` tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage verifies true/false assignment values from `hovering in`, and SSR coverage verifies the hover breadcrumb/class. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification for the `.onHover` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 601 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 803 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `.onHover` slice: focusable/focused metadata, static swipe/refresh action affordances, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- `.task` implementation landed in `apps/desktop/web/src/components/CustomSidebarSurface.tsx`: it reuses the safe local-handler subset from `.onAppear`, `.onDisappear`, `.onSubmit`, and `.onChange`; parses only trailing closure local-handler bodies; preserves optional `id:` as evaluated `value` plus raw `secondaryValue`; runs handlers from a separate `useEffect` keyed by `swiftTaskModifierSignature(...)`; and exposes `data-swift-task`, `data-swift-task-id`, and `data-swift-task-id-expression` breadcrumbs. This is not full Swift async/await, cancellation, priority, actor, or structured-concurrency behavior.
- `.task` tests landed in `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`: parser coverage lives in the existing local-handler fixture, and SSR breadcrumb coverage lives in the local `@State` controls fixture. Docs were updated in `docs/custom-sidebars.md`, `docs/cli-contract.md`, and `docs/swiftui-interpreter-surface.md`.
- Verification pattern after each UI slice: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`; then `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx`; then `bun run --cwd apps/desktop/web typecheck`; then `bun run --cwd apps/desktop/web build`. Build has had only the known Vite large-chunk warning.
- The worktree is intentionally very dirty with many tracked and untracked files from prior slices. Do not revert unrelated changes. `apps/desktop/web/src/components/CustomSidebarSurface.tsx`, `apps/desktop/web/src/components/CustomSidebarSurface.test.tsx`, `apps/desktop/web/src/styles.css`, and the docs are expected hot files.
- Windows desktop icon/shortcut work exists at `scripts/desktop/install-desktop-shortcut.ps1`; earlier user asked about putting cmux on the desktop and whether it runs cmux for Windows. Treat that as a separate packaging/shortcut thread unless they explicitly ask to resume it.

Current focus:

- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained scroll chrome modifier coverage added. `.scrollIndicators(.hidden, axes:)` now preserves visibility/axis metadata as stable classes with scrollbar-hiding CSS hints, and `.scrollClipDisabled()` now emits a safe overflow hint class. This does not claim native paging/snap targets, bounce behavior, scroll physics, or exact platform scrollbar semantics.
- Verification passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 681 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 883 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `ForEach(data,id:)` identity breadcrumb coverage added. ForEach now resolves simple scalar/key-path identities and stamps generated top-level rows with existing `.id(...)` metadata so rendered rows expose `data-swift-id` breadcrumbs. This does not claim SwiftUI row identity diffing, move animations, native reconciliation, or wrapper/list cell identity semantics.
- Verification for the ForEach-id slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 678 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 880 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `List(data,id:)` coverage added. Data-driven List now evaluates supported sequence expressions, expands rows through the trailing closure using existing loop scope mechanics, and preserves the `id:` token as list metadata/classes/data attributes while preserving plain `List { ... }` behavior. This does not claim list selection, edit mode, SwiftUI identity/diffing semantics, or native platform list chrome.
- Verification for the data-list slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 675 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 877 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `Section(header:footer:)` coverage added. Section nodes now preserve single-view-expression header/footer slots, collect event handlers from those slots, and render dedicated section title/header/body/footer regions through the safe Swift node path. This does not claim arbitrary `@ViewBuilder` header/footer closures, list selection/edit mode, or native platform list chrome.
- Verification for the section-header/footer slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 669 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 871 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained lazy stack metadata coverage added. `LazyVStack`/`LazyHStack` now preserve lazy intent, constrained alignment/spacing, and accepted `pinnedViews:` tokens as stable classes plus `data-swift-pinned-views` breadcrumbs while reusing the safe stack renderer. This does not claim native virtualization, sticky section headers/footers, pinned scroll physics, or native lazy stack identity/layout semantics.
- Verification for the lazy-stack slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 664 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 866 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained stack initializer alignment coverage added. `VStack(alignment:spacing:)`, `HStack(alignment:spacing:)`, `ZStack(alignment:)`, and lazy stack aliases now preserve safe alignment tokens, emit stable `cmux-custom-sidebar-swift-stack-alignment-*` classes, and lower them to axis-aware CSS alignment hints while preserving existing stack spacing and modifier behavior. This does not claim full SwiftUI alignment guides, baseline alignment, RTL-aware leading/trailing mirroring, native layout negotiation, or exact wrapper semantics.
- Verification for the stack-alignment slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 659 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 861 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.frame(..., alignment:)` coverage added. `.frame(..., alignment:)` now preserves safe alignment tokens, emits stable `cmux-custom-sidebar-swift-frame-*` classes, and lowers them to CSS text/flex alignment hints while preserving existing frame dimensions and `maxWidth: .infinity` fill behavior. This does not claim full SwiftUI proposed-size negotiation, wrapper placement, or native layout semantics.
- Verification for the frame-alignment slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 652 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 854 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).

- Previous UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.frame(...)` dimension coverage added. `.frame(width:height:minWidth:minHeight:idealWidth:idealHeight:maxWidth:maxHeight:)` now preserves constrained numeric dimensions and lowers them to safe CSS sizing fields, while `maxWidth: .infinity` continues to render as the sidebar fill class. `idealWidth`/`idealHeight` act as preferred CSS dimensions only when fixed width/height are absent. This does not claim full SwiftUI proposed-size/ideal-size negotiation, alignment placement, or native layout semantics.
- Verification for the frame-dimension slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 649 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 851 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the frame-dimension slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, frame alignment placement, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained edge-set `.padding(...)` coverage added. `.padding(8)` still applies all-side padding, while `.padding(.horizontal, 6)`, `.padding(.vertical, 6)`, `.padding([.top, .bottom], 4)`, and simple edge tokens now lower to side-specific safe CSS padding fields. This does not claim full `EdgeInsets(...)`, RTL-aware leading/trailing mirroring, or native SwiftUI layout negotiation.
- Verification for the edge-padding slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 644 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 846 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the edge-padding slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, `frame(width:height:)`/ideal dimensions, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.hueRotation` / `.blendMode` visual modifier coverage added. `.hueRotation(.degrees(...))` now lowers through the safe filter pipeline as `hue-rotate(...)`, and `.blendMode(...)` maps known SwiftUI blend tokens through a fixed CSS `mix-blend-mode` allow-list with stable `cmux-custom-sidebar-swift-blend-*` classes. This does not claim native SwiftUI compositing groups, exact color-space behavior, isolated blend stacks, or full blend parity.
- Verification for the hue/blend slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 640 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 842 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the hue/blend slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained hierarchical/named foreground palette coverage added. The renderer now maps hierarchy tokens (`primary`, `secondary`, `tertiary`, `quaternary`, `quinary`), `accent`/`accentColor`, and named SwiftUI-adjacent colors (`mint`, `indigo`, `brown`) to safe sidebar colors, with `cmux-custom-sidebar-swift-foreground-*` classes for hierarchical foreground tokens. This does not claim full inherited SwiftUI `ShapeStyle`, multi-arg/layered `foregroundStyle`, environment propagation, or native style resolution.
- Verification for the hierarchical foreground slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 638 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 840 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the hierarchical foreground slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, multi-arg `foregroundStyle`, broader `StyleValue` integration, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained CSS-backed Material background coverage added. The renderer now maps `.background(.ultraThinMaterial)`, `.background(.thinMaterial)`, `.background(.regularMaterial)`, `.background(.thickMaterial)`, `.background(.ultraThickMaterial)`, and `.background(.bar)` to safe translucent CSS backgrounds with blur/saturation plus stable `cmux-custom-sidebar-swift-material-*` classes. This does not claim native SwiftUI vibrancy/material blending, host-window sampling, full `StyleValue` integration, foreground/material shape styles, or platform material semantics.
- Verification for the Material slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 633 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 835 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Material slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, broader `StyleValue` integration for hierarchical/material foreground styles, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `LabeledContent` coverage added. The parser now accepts `LabeledContent("Title", value: expr)` and `LabeledContent("Title") { <value views> }` as first-class read-only row nodes. The React renderer displays compact inspector-style label/value rows, renders arbitrary interpreted value children in the value slot, and exposes `cmux-custom-sidebar-swift-labeled-content*` classes plus `data-swift-labeled-content` breadcrumbs. This does not claim named-label trailing-closure overloads, full format-style/initializer coverage, editable form behavior, or native SwiftUI row semantics.
- Verification for the `LabeledContent` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (81 passed, 628 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (131 passed, 830 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `LabeledContent` slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, named-label `LabeledContent` overloads, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.dropDestination(for:)` command-target coverage added. The parser now accepts `.dropDestination(for:) { cmux(...) }`, preserves the type token, and captures the safe action. The React renderer marks nodes as browser drop targets, prevents default browser drop handling, invokes the safe action on drop, and exposes `data-swift-drop-destination` plus drop-target classes. This does not claim typed `Transferable` payload binding, dropped item/location binding, native drop highlighting, or full drop result semantics.
- Verification for the drop-destination slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 617 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 819 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the drop-destination slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, typed drag/drop payload binding, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.refreshable` / `.swipeActions` action-affordance coverage added. The parser now accepts `.refreshable { cmux(...) }` and `.swipeActions(edge:allowsFullSwipe:) { ... }`, preserving safe actions, child button rows, edge metadata, and full-swipe breadcrumbs. The renderer displays a sidebar-local Refresh button and swipe action tray, and CSS gives them intentional sidebar styling. This does not claim native pull-to-refresh physics, platform swipe gestures, edit actions, or full list-row integration.
- Verification for the refresh/swipe slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 614 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 816 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the refresh/swipe slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, constrained drag/drop metadata, `.dropDestination` command affordances, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.focusable()` / `.focused($localBool)` coverage added. The parser now accepts `.focusable()`/`.focusable(false)` and `.focused($localBool)` for direct local `@State` Bool bindings. The React renderer makes focusable nodes keyboard-focusable, mirrors browser focus/blur into the local state bag for `.focused`, and exposes `data-swift-focusable`, `data-swift-focused`, `data-swift-focused-binding`, `cmux-custom-sidebar-swift-focusable`, and `cmux-custom-sidebar-swift-focused` breadcrumbs/classes. This does not claim full `@FocusState`, programmatic focus ownership, focus scopes, or native focus propagation.
- Verification for the focus slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 606 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 808 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the focus slice: static swipe/refresh action affordances, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, constrained drag/drop metadata, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.onHover` coverage added. The parser now accepts `.onHover { hovering in ... }`, evaluates the hover parameter for both enter and leave paths, supports the same safe local handler subset as submit/change/lifecycle hooks, and the React renderer fires handlers from browser mouse enter/leave events. Rendered nodes expose `data-swift-on-hover` and `cmux-custom-sidebar-swift-hoverable` breadcrumbs. This does not claim full gesture composition, drag/drop values, focusable action support, or native pointer-region semantics.
- Verification for the `.onHover` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 601 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 803 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `.onHover` slice: focusable/focused metadata, static swipe/refresh action affordances, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained `.task` coverage added. The parser now accepts `.task { ... }` and `.task(id:) { ... }` as local handlers using the same safe subset as submit/change/appear/disappear: simple local `@State` assignments plus safe-scoped `cmux(...)` actions. Rendered nodes expose `data-swift-task`, `data-swift-task-id`, and `data-swift-task-id-expression` breadcrumbs, and the React renderer fires task handlers on mount and when the stable task signature changes. This does not claim full Swift async/await, cancellation, priority, actor, structured-concurrency, exact SwiftUI lifecycle timing, or arbitrary closure execution.
- Verification for the `.task` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 598 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 800 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `.task` slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, constrained focus/hover/drag gesture metadata, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained lifecycle hook coverage added. The parser now accepts `.onAppear { ... }` and `.onDisappear { ... }` as local handlers using the same safe subset as submit/change: simple local `@State` assignments plus safe-scoped `cmux(...)` actions. Rendered nodes expose `data-swift-on-appear` / `data-swift-on-disappear` breadcrumbs, and the React renderer fires appear handlers on mount and disappear handlers on unmount using a stable handler signature so local state updates do not repeatedly re-fire appearance. At the time this did not claim exact SwiftUI lifecycle timing, arbitrary closure execution, true value diffing, `.task`, or `.transaction`; constrained `.task` coverage was added in the later slice above.
- Verification for the lifecycle hook slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 594 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 796 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the lifecycle hook slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, constrained `Task`/async lifecycle metadata, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift inert animation/transition metadata coverage added. The parser now accepts `.animation(...)`, `.transition(...)`, `.contentTransition(...)`, and `.symbolEffect(...)`, preserving constrained token/value/isActive metadata. The renderer exposes stable classes plus `data-swift-animation`, `data-swift-transition`, `data-swift-content-transition`, and `data-swift-symbol-effect*` breadcrumbs on rendered nodes. This intentionally does not claim `withAnimation`, watched-value animation firing, state-diff transition behavior, matched geometry, or real SF Symbol effect runtime behavior yet.
- Verification for the inert animation metadata slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 590 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 792 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the inert animation metadata slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `.onAppear`/`.onDisappear` constrained lifecycle hooks, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift accessibility metadata coverage expanded. The parser now accepts `.accessibilityValue(...)`, `.accessibilityHint(...)`, `.accessibilityAddTraits(...)`, `.accessibilityElement(children:)`, and `.accessibilitySortPriority(...)` alongside existing label/hidden support. The renderer maps direct web equivalents to `aria-label`, `aria-hidden`, and `aria-valuetext`, preserves richer SwiftUI-only semantics through `data-swift-accessibility-*` breadcrumbs, and adds stable classes for accessibility trait/element metadata. Later slices added constrained action, activation-point, and representation metadata; this still does not claim native VoiceOver rotor/sort behavior or native accessibility-tree substitution.
- Verification for the accessibility metadata slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 579 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 781 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the accessibility metadata slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, inert animation/transition metadata, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `.presentationDetents(...)` / `.presentationDragIndicator(...)` panel-chrome coverage added. The parser now normalizes constrained detent lists such as `[.medium, .large]`, the renderer walks presented content to hoist detent and drag-indicator metadata onto the sidebar-local presentation `<section>`, SSR breadcrumbs expose `data-swift-presentation-detents` and `data-swift-presentation-drag-indicator`, and CSS adds a visible drag handle plus bounded medium/large panel classes. This does not claim native SwiftUI sheet height negotiation, platform drag behavior, full detent selection bindings, or native modal/window semantics.
- Verification for the presentation chrome slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 571 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 773 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the presentation chrome slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, focused SwiftUI accessibility modifier coverage, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift item-based alert/confirmation dialog coverage added. The constrained presentation path now has parser and SSR render coverage for `.alert("Title", item:)` and `.confirmationDialog("Title", item:)` alongside the existing sheet/popover/fullScreenCover item forms, binds the item value into the closure scope, renders active alert-style item presentations with item breadcrumbs, preserves direct child `Button(role:)` action-row styling, and lets `dismiss()` close by clearing the local item state. This does not claim native modal/window semantics, full `@Environment(\.dismiss)` propagation, full Identifiable item semantics, native presentation detent behavior, or full lazy host presentation behavior; constrained detent/drag panel chrome was added in the later slice above.
- Verification for the item alert/confirmation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed, 567 expects), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed, 769 expects), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the item alert/confirmation slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, presentation detents/chrome metadata, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `.id(...)` metadata coverage added. The parser now preserves `.id(value)` as a Swift modifier, the shared renderer lowers it to a `cmux-custom-sidebar-swift-identified` class plus `data-swift-id` breadcrumb on rendered nodes, and docs mark it as author/debug identity metadata. This composes with the existing modifier path but does not claim full SwiftUI identity/replacement semantics, keyed diffing, transition reset behavior, or state-engine rewalk identity yet.
- Verification for the `.id(...)` metadata slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `.id(...)` slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `Picker` `.tag(...)` selection matching added. Picker options now preserve constrained tag metadata from text/label-like option rows, including rows produced by `ForEach`, display the visible label while comparing/writing the tag value, and render editable `<select>` options with encoded tag breadcrumbs. This composes with direct local `@State` bindings and the recent `ForEach($collection)` local element-binding support. This does not claim full SwiftUI `.tag` identity behavior, `.id(_:)`, rich arbitrary option view value extraction, custom `Binding`, menu/form picker semantics, or mutation of live cmux/session data.
- Verification for the `Picker` `.tag(...)` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (80 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (130 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the picker-tag slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `.id(_:)` identity metadata, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `ForEach($collection)` element-binding coverage added for local state arrays. The constrained renderer now preserves JSON-like local `@State` array/object values, parses `ForEach($items) { $item in ... }` closure parameters, emits nested writable binding keys such as `items[0].title`, `items[0].done`, and `items[0].count` for direct editable controls, and applies those writes immutably back into the local state bag. Direct `$name` controls and live/session read-only bindings continue to work. This does not claim full Swift `Binding`, `@Binding` params, custom collection identity, live cmux/session mutation, arbitrary nested statement mutation, or broad binding composition beyond constrained local array element fields/subscripts.
- Verification for the `ForEach($collection)` element-binding slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (78 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (128 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the element-binding slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `Picker` `.tag` selection matching, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift alert/confirmation dialog action-role coverage added. Alert-style sidebar-local presentations now split direct child `Button` nodes from message content, render those buttons in a dedicated `.cmux-custom-sidebar-swift-presentation-actions` action row, preserve existing `Button(role: .destructive/.cancel)` parsing and role styling, and allow action buttons to use the constrained local `dismiss()` presentation close path. At the time this did not claim native alert button ordering, keyboard-default/cancel behavior, platform-modal semantics, item-based alerts, or full SwiftUI `Alert`/`ConfirmationDialog` action modeling; item-based alert/confirmation coverage was added in the later slice above.
- Verification for the alert-action-role slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (76 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (126 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the alert-action slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `ForEach($collection)`/`@Binding` work, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift constrained presentation dismiss support added. The renderer now recognizes `dismiss()` inside `Button("Title") { dismiss() }` and `Button(action: { dismiss() }) { ... }` as a local presentation action, exposes a presentation-local dismiss context while rendering active sidebar-local presentation bodies, keeps dismiss buttons enabled without routing through `cmux(...)`, and closes the active presentation using the same state mutation as the host Close button (`false` for Bool `isPresented:` bindings, `null` for local item bindings). This does not claim full `@Environment(\.dismiss)` propagation, native modal/window semantics, alert button roles, detents, or route/navigation dismiss behavior.
- Verification for the constrained dismiss slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (76 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (126 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the dismiss slice: generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `ForEach($collection)`/`@Binding` work, alert button roles, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift item-based presentation coverage added. The constrained renderer now parses `.sheet(item:)`, `.popover(item:)`, and `.fullScreenCover(item:)` alongside the existing `isPresented:` forms, binds the item value into the content closure for optional/string-like values, renders active item presentations as sidebar-local panels with `data-swift-presentation-binding="item"` and item breadcrumbs, and closes local item presentations by clearing the bound local `@State` cell to `null`. Bool `isPresented:` presentations still close by flipping the binding to `false`. At the time this did not claim native modal/window semantics, environment `dismiss`, full `Identifiable` semantics, item-based alerts/confirmation dialogs, detents, or full lazy host presentation behavior; item-based alert/confirmation coverage was added in the later slice above.
- Verification for the item-presentation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (76 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (126 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the item-presentation slice: environment `dismiss`, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `ForEach($collection)`/`@Binding` work, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift value-route navigation coverage added. The constrained renderer now supports `NavigationLink(value:) { label }` inside `NavigationStack` when paired with `.navigationDestination(for:) { value in destination }`, resolves string-like route values at parse time, synthesizes concrete destination nodes with the route value bound into the destination closure, and renders value links as enabled sidebar-local navigation rows with `data-navigation-value` breadcrumbs. Static `NavigationLink("Title") { ... }` and `NavigationLink(destination:) { ... }` continue to work. This does not claim `NavigationStack(path:)`, persisted/path-bound route state, broad typed route matching, multiple destination overload dispatch beyond the constrained subset, `NavigationSplitView`, `dismiss`, or `TabView(selection:)`.
- Verification for the value-route navigation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (76 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (126 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the route-navigation slice: item-based presentation and `dismiss`, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `ForEach($collection)`/`@Binding` work, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `DatePicker`/`ColorPicker` local-control coverage added. The constrained renderer now parses `DatePicker("Title", selection:, displayedComponents:)` and `ColorPicker("Title", selection:)`, preserves direct local `@State` binding keys, renders native HTML date/time/datetime-local and color inputs with sidebar styling, and fires the same constrained `.onChange` local handler path as the other editable controls. Live cmux/session and constant bindings remain read-only. This does not claim native Swift `Date`/`Color` values, calendar/range/format-style semantics, labels beyond the constrained title/children subset, or mutation of live cmux/session data.
- Verification for the `DatePicker`/`ColorPicker` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (76 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (126 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the date/color slice: item-based presentation and `dismiss`, route/path binding and `navigationDestination`, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `ForEach($collection)`/`@Binding` work, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift local submit/change handler coverage added. The constrained renderer now parses `.onSubmit { ... }` and `.onChange(of: value) { ... }` modifiers, preserves their local handler payloads on render nodes, and executes only simple local `@State` assignments plus safe-scoped `cmux(...)` actions. Direct local editable controls now fire `.onChange` when their bound local state changes; single-line text fields fire `.onSubmit` on Enter, and multiline text editors fire it on Ctrl/Cmd+Enter. SSR breadcrumbs expose `data-swift-on-submit` and `data-swift-on-change`. This does not claim arbitrary Swift closure execution, compound mutation, custom `Binding`, mutation of live cmux/session data, or true `.onChange` value-diffing across re-walks.
- Verification for the submit/change slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (76 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (126 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the submit/change slice: item-based presentation and `dismiss`, route/path binding and `navigationDestination`, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, `DatePicker`/`ColorPicker`, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift local-state control coverage expanded. The constrained renderer now parses and renders `SecureField("...", text:)`, `TextEditor(text:)`, and `Stepper("...", value:in:step:)` alongside the existing `TextField`/`Toggle`/`Slider`/`Picker` controls. Direct local `@State` bindings are editable, live cmux/session and constant bindings remain read-only, `SecureField` renders as password input, `TextEditor` renders as multiline text, and `Stepper` renders an accessible spinbutton with increment/decrement controls plus `data-swift-state-binding` breadcrumbs. This does not claim full custom `Binding`, `@Binding` params, focus/submit semantics, `Stepper(onIncrement:onDecrement:)`, or live cmux-data mutation.
- Verification for the expanded local-control slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (75 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (125 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the expanded local-control slice: `.onSubmit`/`.onChange(of:)`, item-based presentation and `dismiss`, route/path binding and `navigationDestination`, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift sidebar-local presentation coverage added. The constrained renderer now parses `.sheet(isPresented:)`, `.popover(isPresented:)`, `.fullScreenCover(isPresented:)`, `.alert("Title", isPresented:)`, and `.confirmationDialog("Title", isPresented:)` modifiers for simple local `@State Bool` bindings, renders active presentations as sidebar-local dialog/panel surfaces, and closes them by flipping the local state binding to `false`. This does not claim item-based presentations, native window/modal semantics, `dismiss`, alert button roles, presentation detents, or full lazy host presentation behavior.
- Verification for the presentation slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (75 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (125 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the presentation slice: item-based presentation and `dismiss`, route/path binding and `navigationDestination`, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift sidebar-local navigation stack coverage added. The constrained renderer now parses `NavigationStack { ... }`, `NavigationLink("Title") { ... }`, and `NavigationLink(destination: <view>) { <label> }`, renders links as intentional sidebar rows, and maintains an in-pane React navigation stack with Back navigation and nested-link support. Destination children are preserved for event/action collection and rendered lazily when pushed. This does not claim `NavigationStack(path:)`, `NavigationLink(value:)` route matching, `navigationDestination`, `NavigationSplitView`, `dismiss`, sheets/popovers, or persisted host navigation state.
- Verification for the navigation-stack slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (73 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (123 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the navigation-stack slice: route/path binding and `navigationDestination`, sheets/popovers/alerts, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift `Image("asset")` / `Image(decorative:)` host-asset coverage added. Positional `Image("name")` now correctly becomes an asset lookup instead of being treated as an SF Symbol, while `Image(systemName:)` remains the symbol path. `extension.sidebar.snapshot` now accepts the pane `source_path`, discovers image files in a sibling `<sidebar-name>.assets/` directory, mints jailed `cmux-sidebar-asset://` URLs keyed by relative path and extensionless relative path, and the Tauri `cmux-sidebar-asset` scheme revalidates every request before serving bytes. The renderer also accepts safe `http`/`https` asset URLs, rejects `file:`/`data:`/unsafe entries to visible placeholders, and renders valid assets as bounded lazy `<img>` nodes that compose with existing image modifiers. Full native asset-catalog semantics remain follow-up work.
- Verification for the `Image("asset")` slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_assets --lib --no-run`, `cargo check -p cmux-desktop`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (71 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (121 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the asset-image slice: richer navigation/presentation semantics, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, richer native asset-catalog semantics, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `AsyncImage(url:)` leaf coverage added. The constrained renderer now parses `AsyncImage(url: URL(string: "https://..."))` and direct string URL forms, accepts only `http`/`https` URLs, rejects `file:`, `data:`, and app-internal schemes to a visible placeholder, and renders valid remote images as bounded lazy `<img>` nodes that compose with existing image modifiers such as `.resizable()`, `.interpolation(...)`, and accessibility labels. This does not claim `AsyncImage` content/placeholder/phase closures, asset catalogs, or full native image loading semantics.
- Verification for the `AsyncImage(url:)` slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (70 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (120 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the `AsyncImage` slice: richer navigation/presentation semantics, `Image("asset")` host asset lookup, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift gradient style coverage expanded. The constrained renderer now safely lowers `LinearGradient(colors:startPoint:endPoint:)`, `RadialGradient(colors:...)`, `AngularGradient(colors:...)`, and `Color.<token>.gradient` into CSS-backed styles for `.background`, `.foregroundStyle`, `.fill`, and `.tint`. Colors are mapped through the existing safe Swift color-token palette; this does not claim full SwiftUI `StyleValue`, custom gradient stops/locations, materials, arbitrary color spaces, or exact native blending.
- Verification for the gradient style slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (70 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (120 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the gradient slice: `AsyncImage(url:)` leaf support, richer navigation/presentation semantics, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, or a broader `CmuxSwiftRender` gap audit.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift author event hooks now have a safe first implementation. The parser recognizes `.onEvent("event.name") { ... }` and `.onEvent(category: "workspace") { ... }` modifiers anywhere in the rendered tree, collects document-level handlers, matches subsequent live `cmux://events-changed` bridge events by name/category, skips the retained bootstrap event, applies handler-local `@State` assignments evaluated against `events.latest`, and dispatches optional handler `cmux(...)` calls through the existing safe-scoped `custom_sidebar_action_invoke` path. This is intentionally not arbitrary Swift closure execution, custom `Binding`, persisted state identity, or the full remaining macOS event-name catalog.
- Verification for the `.onEvent` author-hook slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (69 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (119 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the event-hook slice: richer SwiftUI state lifecycle/binding support, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, remaining macOS event-catalog coverage, gradients/AsyncImage/navigation gaps, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift local state/editable control coverage expanded. The constrained renderer now parses simple `@State var name = ...` declarations, feeds local state values into Swift expression evaluation, preserves direct `$name` binding keys on `TextField`, `Toggle`, `Slider`, and simple text-option `Picker`, and renders those local-state controls as editable React controls with `data-swift-state-binding` breadcrumbs. Live cmux/session data bindings and `.constant(...)` remain read-only; this does not claim full SwiftUI state identity/lifecycle, persisted state, custom `Binding`, or backend/live-data mutation.
- Verification for the local `@State`/editable-controls slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (68 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (118 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the local state slice: author-defined `on(event)` handler/state semantics, richer SwiftUI state lifecycle/binding support, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift shape coverage expanded to match the documented subset. `CustomSidebarSurface` now parses/renders `Ellipse`, `UnevenRoundedRectangle`, and `.trim(from:to:)` metadata, composes `.stroke` through the existing safe border lowering, and styles trim via CSS custom properties. `.trim(from:to:)` is intentionally a CSS partial-fill approximation, not full SwiftUI vector path geometry.
- Verification for the Swift shape slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (66 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (116 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the shape slice: author-defined `on(event)` handler/state semantics, stateful two-way controls (`@State`/editable `TextField`/`Toggle`/`Slider`/`Picker`), generated full-dispatcher action schema/coercion, manifest-granted privilege UX, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift split-view coverage expanded. The constrained renderer now parses and renders `HSplitView { ... }` and `VSplitView { ... }` as safe sidebar-local split containers with independently scrollable panes and horizontal/vertical divider styling. This closes a docs-to-UI mismatch for common two-column authored sidebars, while deliberately not claiming full native/persisted SwiftUI split divider behavior.
- Verification for the Swift split-view slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (66 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (116 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the split-view slice: author-defined `on(event)` handler/state semantics, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, stateful two-way controls, or a broader `CmuxSwiftRender` gap audit.
- Latest UI/backend parity slice completed: Windows/Tauri authored custom sidebars now have an in-process event bridge. The backend emits `cmux://events-changed` whenever the control event log records a cmux event, while keeping the existing `events.stream`/durable log behavior. `CustomSidebarSurface` seeds `events.*` from `extension.sidebar.snapshot`, listens for live `cmux://events-changed` payloads, keeps a bounded recent tail/counts, exposes Swift paths such as `events.latest.name`, `events.recent.count`, and `events.name_counts["workspace.selected"]`, and adds JSON-friendly aliases `latestEventName`, `latestEventCategory`, and `latestEventSeq`. This closes the basic in-process authored-sidebar EventBridge refresh gap but does not implement author-defined `on(event)` handlers or the full remaining macOS event-name catalog.
- Verification for the in-process authored-sidebar EventBridge slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop derived_session_events --lib --no-run`, `cargo check -p cmux-desktop`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (64 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (114 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the EventBridge slice: author-defined `on(event)` handler/state semantics, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, or a broader `CmuxSwiftRender` gap audit.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar authored actions now have a method-specific safe-default schema gate at `custom_sidebar_action_invoke`. Allowed action params are validated before dispatch for common sidebar methods (`workspace.select`, `surface.focus` selectors when provided, `sidebar.select/open`, progress/status/meta/log methods, etc.); failures return `custom_sidebar_action_schema_invalid` with `field`, `expected`, `accepted_keys`, and schema version data. `capabilities` now advertises the current authored-action catalog at `custom_sidebar_actions.schema`, and the React custom sidebar surface renders schema errors as direct inline authoring hints instead of generic action failures. This is not full generated dispatcher-wide schema/coercion yet.
- Verification for the authored-action schema slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_action --lib --no-run`, `cargo check -p cmux-desktop`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (62 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (112 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the authored-action schema slice: in-process EventBridge/runtime polish, generated full-dispatcher action schema/coercion, manifest-granted privilege UX, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift text rendering coverage expanded. `Text(verbatim:)` now renders literal strings, static literal `Text("...")` with simple inline markdown renders safe React text runs for `**bold**`, `*italic*`, `` `code` ``, and `http(s)` links, and interpolated text remains verbatim to avoid pretending to support full LocalizedStringKey/string-catalog behavior. Unsafe markdown hrefs are sanitized to `#`.
- Verification for the Text markdown/verbatim slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (62 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (112 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Text markdown/verbatim slice: method-specific action schema UX, EventBridge/runtime polish, or a broader `CmuxSwiftRender` gap audit.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift built-in control style coverage expanded. The constrained renderer now parses/renders token forms of `.controlSize(...)`, `.buttonStyle(.plain|.bordered|.borderedProminent)`, `.buttonBorderShape(...)`, `.labelStyle(...)`, `.menuStyle(...)`, `.pickerStyle(...)`, `.toggleStyle(...)`, and `.textFieldStyle(...)` as safe CSS classes across the existing Button/Label/Menu/Picker/Toggle/TextField nodes. This intentionally does not claim support for custom Swift `ButtonStyle`/`ToggleStyle` structs or style configuration children.
- Verification for the built-in control style slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (60 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (110 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the control-style slice: method-specific action schema UX, `Text` markdown/verbatim discriminator polish, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift gesture coverage expanded. The constrained renderer now supports `.onTapGesture(count:) { cmux(...) }` as a double-click/tap affordance and `.onLongPressGesture { cmux(...) }` as a press-and-hold affordance, both using the same safe-scoped authored action dispatch path as buttons and existing `.onTapGesture`. Gesture wrappers carry visible affordance classes/metadata (`data-tap-count`, long-press/multi-tap classes) without claiming deferred gesture-value payloads, hover/drag gesture state, or arbitrary gesture composition.
- Verification for the tap-count/long-press gesture slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (58 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (108 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the gesture slice: method-specific action schema UX, built-in control style modifiers (`controlSize`/`buttonBorderShape`/`pickerStyle`/`toggleStyle`/`textFieldStyle`), EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift static navigation/toolbar modifier coverage expanded. The constrained renderer now supports sidebar-local `.navigationTitle(...)`, `.navigationSubtitle(...)`, `.navigationBarTitleDisplayMode(...)`, `.toolbar { ToolbarItem { ... } }`, `.keyboardShortcut(...)`, `.contentShape(...)`, and static string `.draggable(...)`. `ToolbarItem` and `ControlGroup` parse as safe group containers, toolbar buttons reuse the existing safe-scoped authored action path, and this does not claim full `NavigationStack`, sheets, or system keyboard handling.
- Verification for the static navigation/toolbar slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (56 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (106 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the static navigation/toolbar slice: method-specific action schema UX, long-press/tap-count gesture polish, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift image modifier coverage expanded. The constrained renderer now parses/renders generic image presentation modifiers `.resizable()`, `.renderingMode(.template|.original)`, `.interpolation(...)`, and `.antialiased(...)` as safe CSS/classes, composing with the existing `.imageScale`, `.symbolRenderingMode`, and `.symbolVariant` SF Symbol support.
- Verification for the image modifier slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (54 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (104 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the image modifier slice: method-specific action schema UX, static navigation/toolbar modifier polish, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift leaf modifier coverage expanded again. The constrained renderer now parses/renders `.fontWeight(...)`, `.fontDesign(...)`, `.accessibilityLabel(...)`, `.accessibilityHidden(...)`, `.redacted(reason:)`, `.privacySensitive()`, and `.unredacted()` as safe presentation metadata. Font weights/designs lower to vetted CSS, redaction/privacy use placeholder styling, and accessibility metadata is carried onto common rendered Swift nodes.
- Verification for the leaf modifier/accessibility/redaction slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (54 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (104 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the leaf modifier/accessibility/redaction slice: method-specific action schema UX, richer image modifiers (`resizable`/`renderingMode`), EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift child-bearing modifier coverage expanded. The constrained renderer now preserves and renders trailing-closure modifier content for `.background { ... }`, `.overlay(alignment:) { ... }`, `.mask { ... }`, `.safeAreaInset(edge:) { ... }`, and `.contextMenu { ... }` through a safe wrapper node. Nested content uses the existing Swift renderer/action path, so context-menu buttons and overlay buttons still route through safe-scoped authored actions. This closes a documented no-op gap without adding arbitrary CSS or arbitrary Swift execution.
- Verification for the child-bearing modifier slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (54 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (104 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the child-bearing modifier slice: method-specific action schema UX, accessibility/redaction/font modifier polish, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift wrapper/control coverage expanded. The constrained renderer now supports `Gauge(value:in:)` as a read-only progress-style view, unwraps `AnyView(<view>)`, and renders `ViewThatFits { ... }` through the existing grouped child renderer. This closes another docs-to-UI mismatch for common SwiftUI sidebar wrappers without claiming full layout negotiation.
- Verification for the Gauge/AnyView/ViewThatFits slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (52 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (102 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Gauge/wrapper slice: method-specific action schema UX, arbitrary-child modifier rendering (`overlay`/`background`/`contextMenu`/`safeAreaInset`), accessibility/redaction/font modifier polish, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift reducer/formatting coverage expanded. The constrained evaluator now supports array `.reduce(initial) { ... }` with `$0`/`$1` shorthand and named accumulator/item closure params, including multi-line `let` bindings that hold reducer expressions. Numeric formatting now supports `.formatted(.currency(code:))`, `.formatted(.percent)`, `.formatted(.notation(.compactName))`, and `Text(value, format: ...)` for static sidebar display/action params.
- Verification for the reducer/formatting slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (50 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (100 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the reducer/formatting slice: method-specific action schema UX, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift collection-transform coverage expanded. The constrained evaluator now supports array `.filter { ... }`, `.map { ... }`, `.flatMap { ... }`, and `.sorted { ... }` with both `$0`/`$1` shorthand closures and named closure params. These transforms chain through existing member resolution, work in `ForEach(...)` collections, and preserve mapped arrays in typed `cmux(...)` action params.
- Verification for the collection-transform slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (48 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (98 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the collection-transform slice: method-specific action schema UX, reduce/formatted helper coverage, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift data-literal coverage expanded for dictionary/object literals and keyed subscripts. The constrained evaluator now distinguishes array literals from dictionary literals with top-level `key: value` entries, supports dynamic keys such as `[selectedId: selectedTitle]`, resolves object/dictionary subscripts like `labels[workspace.id]` and `counts["ports"]`, preserves dictionary values in typed `cmux(...)` action params, and treats missing path/subscript values as `nil` for optional-style comparisons like `lookup["missing"] == nil`.
- Verification for the dictionary/subscript slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (46 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (96 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the dictionary/subscript slice: method-specific action schema UX, richer Swift collection transforms (`filter`/`map`/`sorted`), EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift expression parity expanded for arithmetic, comparisons, and boolean logic. The constrained evaluator now handles parenthesized expressions, string `+` concatenation, numeric `+ - * / %` with safe `/ 0`/`% 0`, comparisons `== != > >= < <=`, short-circuit `&&`/`||`, and unary `!` with proper precedence. These expressions work in `if` conditions, interpolated `Text`, local/user helper functions, and typed `cmux(...)` action params.
- Verification for the arithmetic/logical expression slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (44 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (94 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the arithmetic/logical slice: method-specific action schema UX, dictionary/subscript literal support, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift value-helper coverage expanded again. The constrained evaluator now supports string `.count`, `.hasPrefix(...)`, `.hasSuffix(...)`, `.contains(...)`, `.uppercased()`, `.lowercased()`, `.split(separator:)`, common builtins `min`, `max`, `abs`, `Int`, `Double`, `String`, and balanced/nested Swift string interpolation such as `"\(String(max(workspaceCount, 3)))"`. These helpers work in `Text(...)`, `if` conditions, `ForEach` sequences, local/user helper functions, and typed `cmux(...)` action params.
- Verification for the string/builtin helper slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (42 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (92 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the string/builtin helper slice: arithmetic/comparison/logical expression parity, method-specific action schema UX, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift local `let` bindings and simple user helper functions are now implemented in the constrained parser. Top-level and nested `let name = expr` declarations feed subsequent views/actions; simple `func` declarations can return value expressions for `Text(...)`/`cmux(...)` params or return `some View` row helpers that parse through the existing Swift node renderer. This unlocks natural authored patterns like `marker(workspace)` and `row(workspace)` without claiming full Swift typechecking, overloads, closures, or arbitrary execution.
- Verification for the let/function helper slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (40 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (90 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the helper slice: method-specific action schema UX, richer string/number helpers, EventBridge/runtime polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar capability manifests are now surfaced in the left-sidebar custom host, not just backend validation/denied-action payloads. `SidebarView` preserves manifest summaries from `sidebar.list`, matches them to the selected custom sidebar, and renders a compact trust strip showing safe-default/no-manifest state, requested/allowed method counts, invalid manifest errors, and denied requested methods. This makes the manifest/trust backend work visible where authors select and run sidebars; manifests still do **not** grant dangerous methods.
- Verification for the manifest/trust UI slice passed: `bun test apps/desktop/web/src/components/Sidebar.test.tsx` (25 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (88 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the manifest/trust UI slice: richer Swift view-helper/function coverage, EventBridge/runtime action schema polish, or a broader completion audit against macOS `CmuxSwiftRender` gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `Picker(..., selection:)` support is now implemented as a live-data/read-only control. The constrained Swift parser resolves `$`/`.constant(...)` selection bindings, renders the selected value in a compact picker row, and renders trailing picker option views using the existing Swift child parser (including `ForEach(workspaces)` option lists). This keeps picker selection state visible without claiming full `@State` or two-way picker editing.
- Verification for the Picker control slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (38 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (87 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Picker slice: manifest-granted privilege UX/design, richer Swift view-helper/function coverage, or a broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `TextField(..., text:)` and `Slider(value:in:)` support is now implemented as live-data/read-only controls. The constrained Swift parser resolves `$` bindings and `.constant(...)`/`Binding.constant(...)` expressions through the same binding helper used by `Toggle`, renders `TextField` as a read-only input, renders `Slider` as an accessible read-only range track with bounds and current value, supports trailing label content, and keeps the docs clear that full `@State`/two-way editing is still follow-up work.
- Verification for the TextField/Slider control slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (36 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (85 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the TextField/Slider slice: `Picker` read-only selection display, manifest-granted privilege UX/design, or a broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI parity slice completed: Windows/Tauri custom-sidebar Swift `Toggle(..., isOn:)` support is now implemented as a live-data/read-only control. The constrained Swift parser resolves `$` bindings and `.constant(...)`/`Binding.constant(...)` expressions, renders an accessible `role="switch"` with on/off visual state, supports trailing label content, and composes with the existing `.onTapGesture { cmux(...) }` wrapper when a toggle-shaped row should fire an action. This does **not** claim full `@State` or two-way input editing; `TextField`/editable `Toggle`/`Slider`/`Picker` remain follow-up state-engine parity work.
- Verification for the Toggle control slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (34 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (83 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Toggle slice: `TextField`/`Slider`/`Picker` read-only display bindings, manifest-granted privilege UX/design, or a broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift `.onTapGesture { cmux(...) }` support is now implemented for non-button views. Trailing-closure modifiers without parentheses are parsed, tap gestures that contain `cmux(...)` lower to plain safe-scoped button wrappers around the authored view, and the wrapper preserves visual modifiers on the inner view while avoiding heavy default button chrome. This turns the documented "any view tappable" pattern from a no-op into working UI without implementing the full state/gesture engine.
- Verification for the onTapGesture interaction slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (32 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (81 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: stateful controls spike (`Toggle`/`TextField` display + bindings), manifest-granted privilege UX/design, or broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar capability manifests now exist as trust/discoverability metadata. Adjacent `<name>.manifest.json` files are ignored as sidebars, parsed during `sidebar.list`/`sidebar.validate`, and reported with `trusted`, `requested_methods`, `allowed_requested_methods`, `denied_requested_methods`, `policy`, and validation errors. `custom_sidebar_action_invoke` now accepts the sidebar `sourcePath`; denied actions include matching manifest context, and `CustomSidebarSurface` surfaces manifest-requested denied methods in the inline action error. Manifests do not grant dangerous methods yet; they make the trust gap explicit without weakening the safe default policy.
- Verification for the capability-manifest/trust metadata slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar --lib --no-run`, `cargo check -p cmux-desktop`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (30 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (79 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: manifest-granted privilege UX/design, stateful controls spike, or broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI/backend parity slice completed: Windows/Tauri Swift-authored custom-sidebar actions now preserve typed `cmux(...)` params instead of stringifying every value. The constrained Swift evaluator now passes JSON-compatible numbers, booleans, `nil`/`null`, arrays, ternary results, enum-like tokens, and resolved live-data fields (`workspaceCount`, `workspaces[0].selected`, etc.) through to `custom_sidebar_action_invoke`. This closes the basic typed-param coercion gap for authored Swift actions; method-specific schema validation/coercion and trust manifests remain follow-up work.
- Verification for the typed Swift action-param slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (30 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (79 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: per-sidebar capability manifest/trust UX, stateful controls spike, or broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift layout/decorator modifier coverage expanded. `CustomSidebarSurface` now parses/renders safe CSS-backed `.layoutPriority`, `.offset`, `.zIndex`, `.aspectRatio`, `.scaledToFit`, `.scaledToFill`, `.clipShape`, `.clipped`, `.shadow`, `.border`, `.stroke`, `.blur`, `.brightness`, `.contrast`, `.saturation`, `.grayscale`, `.rotationEffect`, `.scaleEffect`, plus `.fill`/`.tint` color aliases. These lower to vetted style fields/classes only, keeping authored sidebar styling expressive without arbitrary CSS injection.
- Verification for the layout/decorator modifier slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (29 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (78 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: typed action param coercion/trust UX, stateful controls spike, or begin a broader completion audit against macOS `CmuxSwiftRender`/EventBridge gaps.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift value-helper coverage expanded for common array authoring patterns. The constrained parser/evaluator now supports `workspaces.first`, `workspaces.last`, `.contains(...)`, `.reversed()`, `.prefix(n)`, `.suffix(n)`, `.dropFirst(n)`, `.dropLast(n)`, `.enumerated()`, `Array(workspaces.enumerated())`, and tuple-style `ForEach(...){ index, workspace in ... }` closure params. This makes natural SwiftUI list slicing/enumeration examples render instead of warning/no-oping.
- Verification for the value-helper slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (27 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (76 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: remaining layout/decorator modifiers (`overlay`, `clipShape`, `shadow`, `layoutPriority`, offsets), typed action param coercion/trust UX, or begin a small stateful-control spike.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift presentation modifier coverage expanded for documented text/list/SF Symbol modifiers. `CustomSidebarSurface` now parses/renders `.truncationMode`, `.textCase`, `.underline`, `.strikethrough`, `.listStyle`, `.scrollContentBackground(.hidden)`, `.imageScale`, `.symbolRenderingMode`, and `.symbolVariant` as safe classes/styles. This closes another UI-authorship gap where docs encouraged natural SwiftUI modifiers that previously no-op'd in the constrained renderer.
- Verification for the text/list/symbol modifier slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (25 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (74 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: richer value helpers (`enumerated`, `dropFirst`, conversions/min/max), remaining layout/decorator modifiers, or begin a small stateful-control spike.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift leaf/layout coverage expanded with `ScrollView(.horizontal/.vertical, showsIndicators:)`, stack `spacing:` for `VStack`/`HStack`/`ZStack`, `Button(role: .destructive/.cancel)`, `.listRowBackground(...)`, and `.listRowSeparator(.hidden)`. These lower to safe CSS overflow containers, gap styles, role-tinted buttons, and list-row presentation classes in `CustomSidebarSurface`.
- Verification for the scroll/button/list-row slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (23 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (72 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work: richer value helpers (`enumerated`, `dropFirst`, conversions/min/max), remaining text/list modifiers (`truncationMode`, underline/strikethrough, list style), or begin a small stateful-control spike.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift modifier coverage expanded for richer leaf parity. `CustomSidebarSurface` now parses/renders `.italic()`, `.monospaced()`/`.monospacedDigit()`, `.lineLimit(n)`, `.multilineTextAlignment(...)`, `.opacity(...)`, `.fixedSize()`, `.disabled(...)`, and `.help(...)` in addition to the earlier `.font`, `.bold`, color, padding/background/corner radius/frame subset. These map to safe CSS styles/classes plus rendered `title` and button `disabled` attributes; no arbitrary CSS is accepted.
- Verification for the richer Swift leaf-modifier slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (21 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (70 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the latest modifier and scroll/button/list-row slices: richer value helpers (`enumerated`, `dropFirst`, conversions/min/max), remaining text/list modifiers, or begin a small stateful-control spike.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar Swift subset coverage expanded again toward macOS `CmuxSwiftRender`. `CustomSidebarSurface` now parses/renders simple `if let name = path` optional binding, `ZStack`, `Grid`, `GridRow`, `Menu`, and basic shape nodes (`Circle`, `Rectangle`, `RoundedRectangle(cornerRadius:)`, `Capsule`) with existing modifier styling. `Menu` renders as a native disclosure, `Grid/GridRow` use compact CSS grid rows, and shapes inherit safe color modifiers. This is still a constrained interpreter, not the full Swift type/state system.
- Verification for the `if let`/shape/grid/menu slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (19 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (68 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the latest modifier slice: scroll/lazy stack options, button roles, list row modifiers, richer value helpers (`enumerated`, `dropFirst`, conversions), then stateful controls.
- Latest UI/backend parity slice completed: authored custom-sidebar actions on Windows/Tauri now have a safe default capability scope at the shared backend seam. `custom_sidebar_action_invoke` allows sidebar validation/navigation, workspace selection, surface focus/navigation, sidebar read methods, and sidebar presentation metadata updates, while denying browser automation, debug methods, remote/SSH configuration, close/delete operations, and broad file/system mutations with `custom_sidebar_capability_denied` plus policy/allowed-method data. `system.capabilities` advertises the custom-sidebar action policy, and `CustomSidebarSurface` now shows inline action-denied/action-failed messages instead of only logging. This closes the immediate untrusted authored-action scoping gap; richer per-sidebar manifests/trust UX and typed param coercion remain follow-up work.
- Verification for the custom-sidebar action capability slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_action_policy --lib --no-run`, `cargo check -p cmux-desktop`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (17 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (66 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the action-scope and shape/grid/menu slices: continue remaining macOS `CmuxSwiftRender` coverage around richer text modifiers (`italic`, monospaced, line limits), scroll/lazy stack options, button roles/disabled/help, richer array/value helpers, and eventually stateful controls.
- Latest UI/backend parity slice completed: Windows/Tauri custom-sidebar `.swift` files now render through a broader constrained Swift-to-IR subset in `CustomSidebarSurface`. The subset is deliberately shaped after the macOS `CmuxSwiftRender` `RenderNode` model and currently supports common authored patterns: `VStack`/`LazyVStack`, `HStack`/`LazyHStack`, `ZStack`, `Group`, `Text` with `\(…)` interpolation, `Divider`, `Spacer`, `Button("title") { cmux(...) }`, `Button(action: { cmux(...) }) { ... }`, `Button(role:)`, `Label`, `Image(systemName:)`, `ProgressView(value:total:)`, `ScrollView`, `List`, `Section`, `Menu`, `Grid`, `GridRow`, basic shapes (`Circle`, `Rectangle`, `RoundedRectangle`, `Capsule`), `EmptyView`, simple `if/else`, simple `if let`, `ForEach(workspaces)`, `ForEach(workspaces.indices)`, simple `for` ranges such as `0..<workspaceCount`, array `.count`/`.indices`, subscript paths like `workspaces[i].title`, stack `spacing:`, and a constrained visual modifier subset: `.font`, `.bold`, `.italic`, `.monospaced`, `.lineLimit`, `.multilineTextAlignment`, `.foregroundColor`/`.foregroundStyle`, `.opacity`, `.fixedSize`, `.disabled`, `.help`, `.padding`, `.background`, `.cornerRadius`, `.listRowBackground`, `.listRowSeparator`, and `.frame(maxWidth: .infinity)`. It evaluates root placeholders (`workspaceCount`, `selectedTitle`, etc.), row fields (`w.id`, `w.title`, `w.selected`), simple ternaries, and captures SwiftUI-authored `cmux(...)` actions into the safe-scoped `custom_sidebar_action_invoke` dispatcher. `.swift` sources load/reload/hot-reload like `.json` sources. This is **not** the full macOS Swift interpreter yet: user funcs, richer array methods, stateful inputs, and full EventBridge parity remain follow-up work.
- Verification for the expanded Swift subset + modifier renderer slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (16 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (65 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the Swift subset/modifier/action-scope/shape-grid-menu slices: continue richer macOS `CmuxSwiftRender` leaf coverage, then audit per-sidebar capability manifests/trust UX and typed action params.
- Latest UI/backend parity slice completed: Windows/Tauri `cmux sidebar select <name>` now validates a custom sidebar, emits `cmux://custom-sidebar-select`, and returns the selected sidebar payload instead of `not_supported`. The React left sidebar now discovers valid custom sidebars through `custom_sidebar_action_invoke("sidebar.list")`, exposes a compact custom-sidebar picker in the workspace header, consumes CLI-driven select events, swaps the left column into a custom-sidebar host with Back/Reload controls, and renders the selected sidebar through `CustomSidebarSurface`. This closes the `sidebar.select` / left-sidebar activation gap for JSON sidebars and the existing Swift preview path, but **does not** implement arbitrary SwiftUI interpretation, SwiftUI-authored `cmux(...)` capture, capability scoping, full macOS event-catalog parity, or stateful SwiftUI inputs.
- Verification for the custom-sidebar select/left-host slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_select_payload --lib --no-run`, `cargo check -p cmux-desktop`, `bun test apps/desktop/web/src/components/Sidebar.test.tsx apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (34 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the first Swift subset slice: expand toward broader macOS `CmuxSwiftRender` coverage; the later action-scope slice above added the safe default authored-action policy.
- Previous UI/backend parity slice completed: Windows/Tauri `cmux sidebar reload [name]` validates custom sidebars, emits a targeted `cmux://custom-sidebar-reload` event, and returns the event name, targeted valid paths, sidebar entries, and validation payload to CLI/socket callers. `CustomSidebarSurface` listens for that event and refreshes matching open JSON panes immediately; open JSON custom-sidebar panes also poll their source file every 1.5s for save-time hot reload. That slice closed the Windows/Tauri `sidebar.reload` and JSON pane hot-reload gap; the later select/left-host slice above closed `sidebar.select`.
- Verification for the custom-sidebar reload/hot-reload slice passed: `cargo fmt -p cmux-desktop`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_reload_payload --lib --no-run`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx` (10 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx` (35 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after reload/select and the first Swift subset slice: expand toward broader macOS `CmuxSwiftRender` coverage; later slices above added safe default capability scoping and richer Swift leaf coverage.
- Previous UI/backend parity slice completed: JSON custom sidebars got an authored action host on Windows/Tauri. Added Tauri command `custom_sidebar_action_invoke` that wraps the same `handle_control_request` dispatcher used by the named-pipe socket and returns the native `{ ok, value | error }` envelope. `CustomSidebarSurface` supports JSON `button` blocks and action-object overrides on `workspaceList` / `selectedTabs` rows; action params recursively interpolate live placeholders including root session fields and row-local `{workspace.id}` / `{workspace.title}` / `{workspace.index}` / `{tab.id}` / `{tab.title}` before invoking the shared dispatcher. Default row behavior still selects workspaces / focuses tabs. Later slices above closed `sidebar.reload`, hot-reload, `sidebar.select`, the left-sidebar picker host, SwiftUI `cmux(...)` capture, and safe default action capability scoping.
- Verification for the JSON custom-sidebar action-host slice passed: `cargo fmt -p cmux-desktop`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_action_reply --lib --no-run`, `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx` (34 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Next likely custom-sidebar parity work after the action-host/reload/select and first Swift subset slices: expand toward broader macOS `CmuxSwiftRender` coverage; later slices above added safe default capability scoping and richer Swift leaf coverage.
- Previous UI/backend parity slice completed: Windows/Tauri custom-sidebar panes render `.json` custom sidebars instead of only showing the generic preview. `CustomSidebarSurface` loads JSON source files through `file_explorer_read_file`, validates the document shape, supports block types `heading`, `text`, `divider`, `stat`, `workspaceList`, and `selectedTabs`, interpolates `{sourceName}`, `{workspaceCount}`, `{selectedTitle}`, `{selectedId}`, `{unreadTotal}`, and `{portTotal}`, and wires JSON workspace/tab rows to `workspace.select` / `surface.focus` through the existing session hook. Later slices above closed broader JSON action dispatch, `sidebar.reload`, hot-reload, `sidebar.select`, the left-sidebar picker host, and the first `.swift` subset renderer; full macOS SwiftUI interpretation remains follow-up parity work.
- Verification for the JSON custom-sidebar renderer slice passed: `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx` (33 passed), `bun run --cwd apps/desktop/web typecheck`, `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only), and `cargo check -p cmux-desktop`.
- Historical next-step note from the renderer/action-host/reload slices: `sidebar.select`/left-picker activation is now done; continue with SwiftUI interpretation or a constrained Swift-to-IR renderer using the existing macOS package as the parity source.
- Previous UI/backend parity slice completed: Windows/Tauri `cmux sidebar open <name>` validates a discovered custom sidebar and binds the focused pane to a real `custom-sidebar` preview surface instead of returning `not_supported`. The session layer persists the source path in the pane's existing `file_path` and marks `surface_kind = "custom-sidebar"`; the desktop control socket resolves the target/focused pane and returns the updated surface payload. The React workspace renderer mounts `CustomSidebarSurface`, a live session-data preview that shows source name/path, workspaces, tabs, unread counts, ports, branch/dirty state, progress/status/metadata/log counts, and remote state. The command switcher knows the `custom-sidebar` surface label/keywords. Later slices above closed `sidebar.reload`, hot-reload, JSON authored tap actions, `sidebar.select`, and the left-sidebar picker host; arbitrary SwiftUI interpretation/action capture remains follow-up parity work.
- Verification for the custom-sidebar open/preview slice passed: `cargo fmt -p cmux-cli -p cmux-desktop`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop custom_sidebar_validation --lib --no-run`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, `cargo test -p cmux-cli command_forward --lib` (27 passed), `bun test apps/desktop/web/src/components/CustomSidebarSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/switcherEntries.test.ts` (50 passed), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (existing Vite large-chunk warning only).
- Historical next-step note: the JSON action host, hot-reload/reload behavior, left-sidebar picker host, Swift subset renderer, SwiftUI-authored `cmux(...)` tap actions, and safe default capability scoping are now implemented. Continue from `C:\Users\User\coding\work\ashlr-mux\cmux`.
- Local caveats: the worktree is very dirty with many unrelated/untracked files; do not revert anything. `apps/desktop/src-tauri/src/control_socket.rs` and `crates/cmux-cli/src/command_forward.rs` may be untracked in this state, so `git diff -- <file>` can be misleading. Direct runnable `cmux-desktop` Rust tests often hit `STATUS_ENTRYPOINT_NOT_FOUND`; use `--no-run` unless live behavior is required.
- Finish parity between `ashlr-mux` and `cmux`; the previously unchecked Browser automation row in `TODO.md` is now checked, but the active goal is not complete until a broader UI/backend parity audit confirms no required work remains.
- Previous UI/backend parity slice completed: custom-sidebar CLI/socket commands are no longer an undocumented hole on Windows/Tauri. The desktop control socket advertises and handles `sidebar.list`, `sidebar.validate`, `sidebar.reload`, `sidebar.select`, and `sidebar.open`; `sidebar.list` / `sidebar.validate [name]` inspect `~/.config/cmux/sidebars` (or `CMUX_SIDEBARS_DIR`) and return a structured validation report with Swift-over-JSON preference, JSON parse errors, empty-file errors, warnings for Swift syntax interpretation not yet available, valid/invalid counts, and selected-name filtering. The Rust CLI routes `cmux sidebar [list|validate|reload|select|open]` to those methods. Later slices above replaced the temporary `not_supported` behavior for `sidebar.open`, `sidebar.reload`, and `sidebar.select`.
- Verification for the custom-sidebar command slice passed: `cargo fmt -p cmux-cli -p cmux-desktop`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, `cargo test -p cmux-cli command_forward --lib` (27 passed), `cargo test -p cmux-desktop custom_sidebar_validation --lib --no-run`, and `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`.
- Latest UI/backend parity slice completed: `extension.sidebar.snapshot` now includes an interpreter-ready `data` tree in addition to the raw snapshot fields. The new `data` object mirrors the macOS `CustomSidebarDataContextBuilder` contract for authored/custom sidebars: `workspaces`, `workspaceCount`, `selectedTitle`, `selectedId`, `unreadTotal`, `clock`, and the Windows/Tauri `events` bootstrap context, with per-workspace `tabs`, ports, unread counts, branch/dirty, PRs, progress, remote, and optional-field omission semantics. This gives the eventual in-process/remote authored-sidebar renderer a stable UI-facing input instead of forcing it to adapt mixed backend/raw fields. Updated `docs/custom-sidebars.md`, `docs/data-driven-sidebar-plan.md`, and `docs/cli-contract.md` to describe `data`.
- Verification for the custom-sidebar `data` projection slice passed: `cargo fmt -p cmux-desktop`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop extension_sidebar_snapshot --lib --no-run`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, `cargo test -p cmux-desktop live_event_subscribers --lib --no-run`, and `cargo test -p cmux-desktop derived_session_events --lib --no-run`. A direct `cargo test -p cmux-desktop extension_sidebar_snapshot --lib` attempted to execute the assertions but hit this machine's known `STATUS_ENTRYPOINT_NOT_FOUND` desktop test-loader failure before tests ran.
- Latest UI/backend parity slice completed: Windows/Tauri now derives pane lifecycle and focus events from authoritative session layout diffs. `SessionEventSummary` now tracks pane summaries per workspace; `derived_session_event_specs` emits `pane.created`, `pane.closed`, and `pane.focused` with category `pane`, stable `pane_ref`, optional `pane_id`, `surface_ids`, `selected_surface_id`, and previous focus metadata. Bootstrap streams now emit current pane state before surface state, and custom-sidebar docs include `--category pane` so authored sidebars can react to split/focus changes without falling back to generic session refreshes.
- Verification for the pane event derivation slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop derived_session_events --lib --no-run`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, `cargo test -p cmux-desktop live_event_subscribers --lib --no-run`, and `cargo test -p cmux-desktop extension_sidebar_snapshot --lib --no-run`.
- Latest UI/backend parity slice completed: Windows/Tauri now derives named `sidebar.*` event-stream events from authoritative session diffs. `SessionEventSummary` now tracks sidebar progress, status entries, metadata entries, metadata blocks, and log entries per workspace; `derived_session_event_specs` emits `sidebar.progress.updated`, `sidebar.progress.cleared`, `sidebar.metadata.updated`, `sidebar.metadata.cleared`, `sidebar.log.appended`, and `sidebar.log.cleared` with category `sidebar` and workspace-scoped payloads. This gives authored-sidebar/custom tooling precise reduce triggers for progress/metadata/log changes instead of only generic `session.changed` refreshes. Updated `docs/events.md`, `docs/custom-sidebars.md`, and `docs/data-driven-sidebar-plan.md` to include `--category sidebar` and the Windows/Tauri sidebar event status.
- Verification for the sidebar event derivation slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop derived_session_events --lib --no-run`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, `cargo test -p cmux-desktop live_event_subscribers --lib --no-run`, and `cargo test -p cmux-desktop extension_sidebar_snapshot --lib --no-run`.
- Latest UI/backend parity slice completed: `extension.sidebar.snapshot` now includes a real `events` bootstrap tree sourced from `ControlEventState`, not placeholder cursor fields. The payload now exposes `events.latest`, `events.recent` (last 50 retained events), `events.category_counts`, `events.name_counts`, `events.latest_seq`/`seq`, `events.oldest_seq`, `events.next_seq`, `events.retained_count`, and `events.boot_id`; top-level `seq`/`latest_seq` now mirror the event cursor. Updated `docs/custom-sidebars.md` and `docs/data-driven-sidebar-plan.md` so authored-sidebar tooling can bootstrap from `events.*` and then use `cmux events --reconnect` for live updates.
- Verification for the custom-sidebar event bootstrap slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop extension_sidebar_snapshot --lib --no-run`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, `cargo test -p cmux-desktop live_event_subscribers --lib --no-run`, and a final `cargo test -p cmux-desktop extension_sidebar_snapshot --lib --no-run`.
- Latest UI/backend parity slice completed: `cmux events` now consumes the live Windows/Tauri event stream incrementally instead of buffering until EOF. Added `stream_rpc_with_handler(...)` in `crates/cmux-cli/src/transport.rs` so stream frames are processed as they arrive; `run_events_command` now prints and flushes frames immediately, honors `--no-ack`, accepts `--no-heartbeats`, persists `--cursor-file` after each processed event, enforces `--limit` client-side for live streams, and performs actual reconnect loops with the latest processed `seq`. This fixes the consumer-side regression introduced by making `events.stream` a real live backend stream.
- Verification for the live `cmux events` consumer slice passed: `cargo fmt -p cmux-cli`, `cargo test -p cmux-cli --bin cmux events_command_tests`, `cargo test -p cmux-cli stream_rpc_collects_raw_frames_until_eof --lib`, `cargo check -p cmux-cli`, and `cargo test -p cmux-cli --lib` (95 passed).
- Latest UI/backend parity slice completed: Windows/Tauri `events.stream` is now a real live stream instead of bounded replay plus close. `cmux-ipc` now exposes `ControlStream::Frames` and `ControlStream::Live`; the serve loop writes initial raw NDJSON frames and then keeps the connection open for channel-delivered frames. `ControlEventState` now tracks live subscribers with name/category filters; `record_event` fans out matching frames while preserving the replay ring and durable JSONL log; and each live stream gets periodic heartbeat frames via a lightweight IPC sleep helper. The normal RPC fallback for `events.stream` still returns the bounded snapshot payload.
- Verification for the live event-stream slice passed: `cargo fmt -p cmux-ipc -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-ipc server --lib`, `cargo test -p cmux-cli stream_rpc_collects_raw_frames_until_eof --lib`, `cargo test -p cmux-desktop live_event_subscribers --lib --no-run`, `cargo check -p cmux-ipc -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, and `cargo test -p cmux-desktop derived_session_events --lib --no-run`.
- Previous UI/backend parity slice completed: Windows/Tauri event replay now emits a richer session-derived lifecycle catalog instead of only generic `session.changed`. `ControlEventState` remembers a compact previous session summary and derives `workspace.created`, `workspace.closed`, `workspace.selected`, `workspace.renamed`, `workspace.reordered`, `surface.created`, `surface.closed`, and `surface.selected` from the authoritative session-change path, so UI actions and socket/backend mutations produce the same named event-stream refresh triggers. Updated `docs/events.md` to tell custom sidebars to use session/workspace/surface categories while preserving the caveat that the remaining macOS event catalog is still follow-up parity work.
- Verification for the richer event-semantics slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop derived_session_events --lib --no-run`, `cargo check -p cmux-desktop`, and `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`.
- Latest UI/backend parity slice completed: Windows/Tauri event replay now has the documented durable JSONL audit log. `record_event` appends each retained event to `~/.cmuxterm/events.jsonl`, creates `~/.cmuxterm` as needed, and rotates the current log to `events.jsonl.1` when the next write would exceed 16 MiB. Added a focused rotation regression for the log helper and updated `docs/events.md`, `docs/cli-contract.md`, and `docs/data-driven-sidebar-plan.md` to remove disk JSONL rotation from the unproven event-stream caveats.
- Verification for the durable event log slice passed: `cargo fmt -p cmux-desktop`, `cargo test -p cmux-desktop event_log_append --lib --no-run`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop event_filters --lib --no-run`, and `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`. Desktop Rust tests remain compile-gated with `--no-run` where appropriate on this Windows machine.
- Earlier UI/backend parity slice completed: Windows/Tauri gained the first real `events.stream` foundation instead of a documented-but-missing method. Extended `cmux-ipc` with a stream-takeover hook for raw NDJSON frames; added `ControlEventState` with a 4096-entry in-memory replay ring; registered it in Tauri state; advertised and dispatched `events.stream`; recorded redacted `session.changed` events from the existing `notify_session_changed` path so UI and socket session mutations create refresh signals; added ack/event/heartbeat frame builders with `after_seq`, `name`, `category`, `limit`, and heartbeat filtering; routed `cmux events` through a stream transport path with `--after`, `--after-seq`, `--cursor-file`, repeated `--name`/`--category`, `--limit`, `--no-ack`, and `--no-heartbeat`. The newer live-stream slice above supersedes its original bounded-replay caveat.
- Verification for the event-stream foundation slice passed: `cargo fmt -p cmux-ipc -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-ipc server --lib`, `cargo test -p cmux-cli --lib`, `cargo check -p cmux-ipc`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop event_filters --lib --no-run`, and `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`. Newer slices above supersede the original limitations around continuous subscribers, richer session-derived event names/categories, and authored-sidebar `events.*` bootstrap data; remaining Wave B work is in-process authored-sidebar EventBridge wiring and broader macOS event-catalog coverage.
- Earlier UI/backend parity slice completed: the documented custom-sidebar bootstrap read method became real on Windows/Tauri. Added `extension.sidebar.snapshot` plus `sidebar.snapshot` alias to the desktop control socket and `system.capabilities`; the payload is built from the existing `workspace.list` / `surface.list` summary helpers and includes selected workspace metadata, ordered `workspaces`, `workspace_groups`, custom-sidebar authoring aliases (`selectedId`, `selectedTitle`, `workspaceCount`, `unreadTotal`, per-workspace `tabs`, `ports`, `portCount`, `tabCount`, `branch`, `dirty`, `pr`, `prs`, `progress`, `panel_directories`, `git_branches`), and the raw session/runtime fields. Newer event-bootstrap work above replaced the original event-cursor placeholders with real `events.*` data. Added CLI routes `cmux sidebar-snapshot` and `cmux extension-sidebar-snapshot`, help/classification entries, and docs in `docs/cli-contract.md`, `docs/custom-sidebars.md`, and `docs/data-driven-sidebar-plan.md`.
- Verification for the extension sidebar snapshot slice passed: `cargo fmt -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-cli command_forward --lib`, `cargo test -p cmux-cli --lib`, `cargo test -p cmux-desktop extension_sidebar_snapshot --lib --no-run`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, and `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`. Desktop Rust tests remain compile-gated with `--no-run` where appropriate on this Windows machine. Next likely parity target: event streaming / EventBridge into authored sidebars, because `docs/data-driven-sidebar-plan.md` still tracks Wave B as incomplete.
- Latest UI/backend parity slice completed: per-surface shell activity (`panelShellActivityStates`) is now end-to-end on Windows/Tauri. Added `SessionPanelShellActivityStateSnapshot` / `SessionPanelShellActivitySnapshot` and `SessionWorkspaceSnapshot.panel_shell_activity`; generated `@cmux/core-types`; exposed `surface.report_shell_state` plus legacy `report_shell_state`/`report-shell-state`; routed `cmux report-shell-state ...` and `cmux surface report-shell-state ...`; included `panel_shell_activity` and counts in `workspace.list` / `workspace.sidebar_state`; projected `shell_activity` / `shell_activity_state` in `surface.list`; pruned shell activity on close and moved it with panels split into new workspaces; and rendered a quiet shell-running chip in the sidebar when any surface reports `commandRunning`.
- Verification for the shell-activity slice passed: `cargo fmt -p cmux-core -p cmux-cli -p cmux-desktop`, `bun run core-types:generate`, `cargo test -p cmux-cli command_forward --lib`, `cargo check -p cmux-core`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, `bun run core-types:check-drift`, `bun run --cwd apps/desktop/web typecheck`, `bun run --cwd apps/desktop/web build`, `bun test apps/desktop/web/src/sidebar/badges.test.ts apps/desktop/web/src/sidebar/sessionBadges.test.ts apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx`, `cargo test -p cmux-desktop apply_set_panel_shell_activity --lib --no-run`, `cargo test -p cmux-desktop surface_list_payload_includes_surface_metadata --lib --no-run`, `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`, and `cargo test -p cmux-core move_panel_to_new_workspace_extracts_panel_and_selects_destination --lib --no-run`. The ts-rs serde warnings and Vite large-chunk warning are existing, and desktop Rust tests remain compile-gated with `--no-run` where appropriate on this Windows machine.
- Latest UI/backend parity slice completed: live terminal process metadata is now queryable on Windows/Tauri. `debug.terminals` now merges snapshot data with live `TerminalState` by panel id and returns `runtime_surface_ready`, `terminal_id`, `root_pid`/`process_root_pid`, `descendant_pids`, `child_pids`, `process_count`, `foreground_pid`, `foreground_process_name`, `foreground_process_source`, and `process_error`; the foreground field is explicitly labeled as a PID-tree leaf approximation when it is not the shell root. The Rust CLI now forwards `cmux debug-terminals [--workspace WORKSPACE]` to `debug.terminals` instead of classifying it without mapping it.
- Verification for the terminal process metadata slice passed: `cargo fmt -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-cli command_forward --lib`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, and `cargo test -p cmux-desktop terminal_runtime_snapshot --lib --no-run`. Direct execution of `cargo test -p cmux-desktop terminal_runtime_snapshot --lib` hit this machine's known `STATUS_ENTRYPOINT_NOT_FOUND` loader issue before tests ran; the compile/no-run gate passed.
- Desktop icon/run checkpoint: release packaging initially failed because `browser.rs` uses `Webview::open_devtools` / `close_devtools`, which require Tauri's `devtools` feature in release builds. Added `devtools` to the `tauri` dependency features in `apps/desktop/src-tauri/Cargo.toml`, then `bun run desktop:web:build`, `cargo build -p cmux-desktop --release`, and `bun run desktop:shortcut` all passed. The shortcut at `C:\Users\User\OneDrive\Desktop\cmux.lnk` now points at the fresh `C:\Users\User\coding\work\ashlr-mux\cmux\target\release\cmux-desktop.exe` built on July 9, 2026 at 8:38 AM.
- Latest UI/backend parity slice completed: panel TTY reporting is now persisted and queryable on Windows/Tauri. Added `SessionPanelTtySnapshot` and `SessionWorkspaceSnapshot.panel_ttys`; generated `@cmux/core-types`; exposed `surface.report_tty` plus legacy `report_tty`/`report-tty` aliases; added `cmux report-tty TTY [--workspace|--tab WORKSPACE] [--panel SURFACE]` and `cmux surface report-tty ...`; surfaced TTYs through `workspace.list`, `workspace.sidebar_state`, `surface.list` (`tty`/`tty_name`), and `debug.terminals`; panel TTY metadata is pruned on close and follows panels moved to new workspaces.
- Verification for the panel TTY slice passed: `cargo fmt -p cmux-core -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-cli command_forward --lib`, `cargo check -p cmux-core`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop apply_set_panel_tty --lib --no-run`, `cargo test -p cmux-desktop surface_list_payload_includes_surface_metadata --lib --no-run`, `cargo test -p cmux-core move_panel --lib --no-run`, `bun run core-types:generate`, `bun run core-types:check-drift`, `$env:TS_RS_EXPORT_DIR='apps/desktop/packages/core-types/src/generated'; cargo test -p cmux-core --features ts --lib`, and `cargo test -p cmux-desktop control_socket_methods_advertise --lib --no-run`. The ts-rs serde-attribute warnings are pre-existing; desktop Rust tests remain compile-gated with `--no-run` where appropriate on this Windows machine.
- Latest UI/backend parity slice completed: legacy agent PID registration now feeds Windows/Tauri sidebar port state. Added `SessionWorkspaceAgentPidSnapshot` and `SessionWorkspaceSnapshot.agent_pids`; generated `@cmux/core-types`; exposed `workspace.set_agent_pid` / `workspace.clear_agent_pid` plus legacy `set-agent-pid` / `set_agent_pid` and `clear-agent-pid` / `clear_agent_pid` aliases; routed `cmux set-agent-pid KEY PID [--workspace|--tab WORKSPACE]` and `cmux clear-agent-pid KEY ...` through the Rust CLI; persisted agent PID ownership facts in the session snapshot; `ports-kick` and the agent-port scanner now rescan registered root PID descendants; `workspace.list` and `workspace.sidebar_state` expose `agent_pids`, `agent_listening_ports`, `ports`, and `agent_pid_count`; CLI `sidebar-state` prints `ports=` and `agent_pid_count=`; and the existing sidebar port badge UI path renders the result.
- Verification for the agent PID/sidebar ports slice passed: `cargo fmt -p cmux-cli -p cmux-desktop -p cmux-core`, `cargo test -p cmux-cli command_forward --lib`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop workspace_list_payload_includes_workspace_runtime_metadata --lib --no-default-features --no-run`, `cargo test -p cmux-desktop apply_set_and_clear_workspace_agent_pid_updates_ownership_facts --lib --no-default-features --no-run`, `cargo test -p cmux-desktop control_socket_methods_advertise_browser_network_and_platform_gaps --lib --no-default-features --no-run`, `bun run core-types:generate`, `bun run core-types:check-drift`, `$env:TS_RS_EXPORT_DIR='apps/desktop/packages/core-types/src/generated'; cargo test -p cmux-core --features ts --lib`, and `cargo test -p cmux-cli --lib`. Desktop Rust tests were compile-gated with `--no-run` where appropriate due this machine's known loader issue; ts-rs emitted the existing serde-attribute warnings only.
- Desktop shortcut helper added: `scripts/desktop/install-desktop-shortcut.ps1` and `bun run desktop:shortcut`. It created `C:\Users\User\OneDrive\Desktop\cmux.lnk` pointing at `C:\Users\User\coding\work\ashlr-mux\cmux\target\release\cmux-desktop.exe`, so double-clicking that icon should launch the Windows cmux desktop app.
- Latest UI/backend parity slice completed: legacy/sidebar PR review metadata commands are now real on Windows/Tauri instead of backend/UI-adjacent only. Added `workspace.report_pr`, `workspace.report_review`, and `workspace.clear_pr` control-socket methods plus `report_pr`/`report-pr`, `report_review`/`report-review`, and `clear_pr`/`clear-pr` aliases; routed `cmux report-pr`, `cmux report-review`, and `cmux clear-pr` through the Rust CLI with legacy `--tab` support; upserts/clears `SessionWorkspaceSnapshot.panel_pull_requests`; includes `git_branch`, `panel_git_branches`, and `panel_pull_requests` in `workspace.list`; and reuses the existing rendered PR badge UI path.
- Verification for the PR/review sidebar metadata slice passed: `cargo fmt -p cmux-cli -p cmux-desktop`, `cargo test -p cmux-cli command_forward --lib`, `cargo test -p cmux-cli --lib`, `cargo check -p cmux-cli`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop workspace_list_payload_includes_panel_pull_requests --lib --no-default-features --no-run`, `cargo test -p cmux-desktop apply_workspace_panel_pull_request_upserts_and_clears_one_panel --lib --no-default-features --no-run`, and `bun test apps/desktop/web/src/sidebar/badges.test.ts apps/desktop/web/src/sidebar/sessionBadges.test.ts apps/desktop/web/src/components/Sidebar.test.tsx` (75 tests). Desktop Rust tests were compile-gated with `--no-run`, consistent with this machine's known desktop test loader issue.
- Latest UI/backend parity slice completed: rich workspace sidebar metadata entries and markdown metadata blocks are now end-to-end instead of placeholders. Added `SessionWorkspaceSidebarMetadataSnapshot`, `SessionWorkspaceSidebarMetadataBlockSnapshot`, `SessionWorkspaceSnapshot.sidebar_metadata_entries`, and `SessionWorkspaceSnapshot.sidebar_metadata_blocks`; generated `@cmux/core-types`; exposed `workspace.report_meta`, `workspace.clear_meta`, `workspace.list_meta`, `workspace.report_meta_block`, `workspace.clear_meta_block`, `workspace.list_meta_blocks`, and `workspace.reset_sidebar` through the desktop control socket and CLI aliases; added CLI text formatting for sidebar state/list commands; and rendered rich metadata rows/blocks gated by `sidebar.showCustomMetadata` and `sidebar.hideAllDetails`.
- Verification for the rich sidebar metadata slice passed: `cargo fmt -p cmux-core -p cmux-cli -p cmux-desktop`, `bun run core-types:generate`, `bun test apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx`, `cargo test -p cmux-cli command_forward --lib`, `cargo test -p cmux-cli --lib`, `cargo test -p cmux-desktop workspace_list_payload_includes_sidebar_metadata_entries_and_blocks --lib --no-default-features --no-run`, `bun run --cwd apps/desktop/web typecheck`, `bun run core-types:check-drift`, `cargo check -p cmux-desktop`, `cargo check -p cmux-cli`, `cargo check -p cmux-core --features ts`, `bun run --cwd apps/desktop/web build`, and `$env:TS_RS_EXPORT_DIR='apps/desktop/packages/core-types/src/generated'; cargo test -p cmux-core --features ts --lib` (291 passed). The only observed warnings were the existing ts-rs serde-attribute warnings and Vite's existing large-chunk warning.
- Latest UI/backend parity slice completed: workspace sidebar status/log metadata is now end-to-end instead of only documented. Added `SessionWorkspaceSidebarStatusSnapshot`, `SessionWorkspaceSidebarLogEntrySnapshot`, `SessionWorkspaceSnapshot.sidebar_status_entries`, and `SessionWorkspaceSnapshot.sidebar_log_entries`; generated `@cmux/core-types`; exposed `workspace.set_status`, `workspace.clear_status`, `workspace.list_status`, `workspace.log`, `workspace.clear_log`, `workspace.list_log`, and populated `workspace.sidebar_state` through the desktop control socket with CLI aliases; routed `cmux set-status`, `clear-status`, `list-status`, `log`, `clear-log`, and `list-log`; and rendered sidebar status/log detail rows gated by `sidebar.showCustomMetadata`, `sidebar.showLog`, and `sidebar.hideAllDetails`.
- Verification for the sidebar status/log slice passed: `bun test apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx`, `cargo test -p cmux-cli command_forward --lib`, `cargo test -p cmux-desktop workspace_list_payload_includes_sidebar_status_and_log --lib --no-default-features --no-run`, `bun run --cwd apps/desktop/web typecheck`, `bun run core-types:check-drift`, `cargo check -p cmux-desktop`, `cargo check -p cmux-cli`, `cargo check -p cmux-core --features ts`, and `bun run --cwd apps/desktop/web build`. The only observed warnings were the existing ts-rs serde-attribute warnings and Vite's existing large-chunk warning.
- Latest UI/backend parity slice completed: workspace sidebar progress is now end-to-end instead of a dormant settings key. Added `SessionWorkspaceSidebarProgressSnapshot` and `SessionWorkspaceSnapshot.sidebar_progress`, generated `@cmux/core-types`, exposed `workspace.set_progress`/`workspace.clear_progress`/`workspace.sidebar_state` through the desktop control socket (with `set-progress`/`clear-progress`/`sidebar-state` aliases), routed `cmux set-progress`, `cmux clear-progress`, and `cmux sidebar-state` through the Rust CLI, and rendered a compact sidebar progress meter gated by `sidebar.showProgress` and `sidebar.hideAllDetails`.
- Verification for the sidebar progress slice passed: `bun test apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx`, `cargo test -p cmux-cli command_forward --lib`, `bun run core-types:check-drift`, `bun run --cwd apps/desktop/web typecheck`, `bun run --cwd apps/desktop/web build`, `cargo test -p cmux-desktop workspace_list_payload_includes_sidebar_progress --lib --no-default-features --no-run`, `cargo check -p cmux-desktop`, `cargo check -p cmux-cli`, and `cargo check -p cmux-core --features ts`. Direct execution of the desktop Rust test still hit this machine's known `STATUS_ENTRYPOINT_NOT_FOUND`; the compile/no-run gate passed.
- Latest UI/backend parity slice completed: workspace remote/SSH state is no longer backend-only. Regenerated `@cmux/core-types` so `SessionWorkspaceRemoteSnapshot.has_ssh_options` is present, projected `workspace.remote` into sidebar badges, rendered an SSH/remote status badge with connected/connecting/error styling, and covered it at pure projector, generated-snapshot adapter, and rendered sidebar levels.
- Verification for the remote/sidebar parity slice passed: `bun test apps/desktop/web/src/sidebar/badges.test.ts apps/desktop/web/src/sidebar/sessionBadges.test.ts apps/desktop/web/src/components/Sidebar.test.tsx`, `bun run core-types:check-drift`, `bun run --cwd apps/desktop/web build`, `bun run --cwd apps/desktop/web typecheck`, `cargo check -p cmux-core --features ts`, and `cargo check -p cmux-desktop`.
- Follow-up UI parity slice completed: Settings sidebar detail toggles now actually gate the rendered sidebar badge lane. `App` passes `hideAllDetails`, `showBranchDirectory`, `showPullRequests`, `showSSH`, and `showPorts` into `Sidebar`, and `SidebarView` forwards them to the session badge projector. Render-level tests prove Hide All Details suppresses branch/PR/SSH/port badges and selective toggles can hide branch/PR/SSH while keeping port chips.
- Verification for the settings-gating slice passed: `bun test apps/desktop/web/src/sidebar/badges.test.ts apps/desktop/web/src/sidebar/sessionBadges.test.ts apps/desktop/web/src/components/Sidebar.test.tsx`, `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (Vite still reports the existing large-chunk warning).
- Additional sidebar link-settings parity slice completed: `makePullRequestsClickable`, `openPullRequestLinksInCmuxBrowser`, and `openPortLinksInCmuxBrowser` now reach the rendered sidebar. PR badges can render as inert pills when clickability is disabled, and PR/port badge clicks can be routed through the existing `session_new_browser_workspace` path via `newBrowserWorkspace(url)` instead of falling through to a plain href.
- Verification for the link-settings slice passed: `bun test apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx apps/desktop/web/src/sidebar/badges.test.ts apps/desktop/web/src/sidebar/sessionBadges.test.ts` (99 tests), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (same existing Vite large-chunk warning).
- Additional sidebar layout-settings parity slice completed: `wrapWorkspaceTitles` and `branchLayout` now reach the rendered sidebar. Wrapped titles get a dedicated row-text class/CSS clamp, and inline branch layout renders the projector's compact `branchSummaryText` once instead of duplicating per-branch pills.
- Verification for the layout-settings slice passed: `bun test apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx apps/desktop/web/src/sidebar/badges.test.ts apps/desktop/web/src/sidebar/sessionBadges.test.ts` (101 tests), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (same existing Vite large-chunk warning).
- Additional right-sidebar settings parity slice completed: `sidebar.rightMaxWidth`/generated `right_max_width` now reaches `FileExplorerPanel`. The right sidebar applies a max-width/flex-basis cap instead of ignoring the setting, with invalid/unset values leaving the default fixed width alone.
- Verification for the right-sidebar width slice passed: `bun test apps/desktop/web/src/components/FileExplorerPanel.test.tsx apps/desktop/web/src/components/WorkspaceList.test.tsx apps/desktop/web/src/components/Sidebar.test.tsx` (55 tests), `bun run --cwd apps/desktop/web typecheck`, and `bun run --cwd apps/desktop/web build` (same existing Vite large-chunk warning).
- Newly completed slice: JS-backed Browser automation on Windows/Tauri (`browser.eval`, `browser.wait`, `browser.snapshot`, screenshot, init scripts, core DOM interactions, getters, predicates, locators, frame/dialog/download, cookies/storage, tabs, console/errors, state save/load, addscript/addstyle) is implemented and live-proven against a child WebView.
- Next required step: continue the broader parity audit across `TODO.md`, command/socket capability coverage, generated TS types, CLI help, UI surfaces, and live desktop behavior before considering the overall goal complete.
- The previous Browser proxy row, `Per-WKWebView proxy observability/inspection once remote proxy path is shipped (URL, method, headers, body, status, timing)`, is checked and live-proven.
- Do **not** mark the overall goal complete until a requirement-by-requirement parity audit confirms the UI/backend state, generated types, CLI/socket surfaces, and live desktop behavior are all complete.

Latest audit result:

- The Windows/Tauri named-pipe control socket did not expose the rich `agent-browser` automation families even though `TODO.md` claimed Browser parity completion.
- Added those method names to `system.capabilities`; the JS-backed P0 subset is now real, and the still-unported rich families continue to return explicit `not_supported` instead of `method_not_found`.
- The implementation pass now replaces the temporary browser automation `not_supported` routing with real JS-backed handlers for the advertised browser automation families, including `browser.screenshot` and `browser.addinitscript`.
- Important caveat: `browser.screenshot` currently returns a valid PNG raster generated from page viewport/background state rather than a true native WebView compositor capture. It is current and hidden-safe, but a future native capture path would be higher fidelity.
- Live proof: `CMUX_BROWSER_AUTOMATION_REQUIRE_EVAL=1 ... python tests_v2\test_windows_browser_network_socket_live.py` passed against a real Windows child WebView, covering JS-backed automation, screenshot, init scripts, locators, frames/dialogs/downloads, cookies/storage, scripts/styles, tabs, console/errors, and state save/load.
- Follow-up audit found and fixed a CLI parity contradiction: `TODO.md` claimed extended `cmux browser ...` grammar was complete, but `crates/cmux-cli/src/command_forward.rs` only routed navigation/devtools/network/platform-gap browser commands. The Rust CLI now routes agent-browser-style subcommands for snapshot/eval/wait, actions, getters/predicates, locators, frame/dialog/download, cookies/storage/tab, console/errors, highlight, state save/load, addinitscript/addscript/addstyle, and `goto`.
- `cmux browser` help now advertises the extended automation families, not just navigation/network commands.
- Verification for the CLI parity slice: `cargo fmt -p cmux-cli`, `cargo test -p cmux-cli`, `cargo check -p cmux-desktop`, `cargo test -p cmux-desktop --lib --no-run`, and `python -m py_compile tests_v2\test_windows_browser_network_socket_live.py tests_v2\test_browser_cli_agent_port.py` passed.
- Additional CLI shorthand fix: `cmux browser <opaque-surface-id> <agent-browser-command...>` now rewrites the leading target internally as `--surface <id>`, so opaque IDs work like `surface:N` refs. A regression also protects `browser tab switch` from confusing the current surface with the target tab.
- Added `docs/browser-automation.md` with identify -> choose surface -> snapshot -> act -> verify examples, target terminology, supported automation families, Network inspection, and explicit WKWebView hard gaps.
- Reconciled `docs/agent-browser-port-spec.md`: Phase 4 P2 rows are now resolved as supported Network inspection plus strict `not_supported` for interception/emulation/trace/screencast/raw input where WKWebView lacks correct semantics; Phase 5/6 docs rows now point to the mapping table and browser automation docs.
- Targeted browser-doc scan now has no unchecked rows in `TODO.md` or `docs/agent-browser-port-spec.md`.

Latest proxy proof slice:

- Added a debug/local direct proxy proof that exercises the real broker/observer path without SSH credentials.
- `debug.browser.start_direct_proxy` starts a panel-scoped direct proxy broker and returns an HTTP proxy URL.
- Proxied child WebViews now get a distinct WebView2 data directory plus explicit `--proxy-server=...` args so per-WebView proxy settings actually take effect on Windows.
- The proxy broker now supports absolute-form HTTP proxy forwarding in addition to SOCKS5 and HTTP CONNECT, plus a target override used only by the local proof harness to avoid loopback/proxy-bypass behavior.
- Relay observation now records captured traffic before expected teardown errors are propagated, so short HTTP exchanges reliably emit Network records.
- `tests_v2/test_windows_browser_network_socket_live.py` now has:
  - baseline mode for live Network API shape;
  - `CMUX_BROWSER_NETWORK_REQUIRE_PROXY_BROKER_RECORD=1` for parsed broker proxy records;
  - `CMUX_BROWSER_NETWORK_REQUIRE_PROXY_RECORD=1` for live child-WebView parsed proxy records.
- Live proof passed on Windows named pipe:
  - `python tests_v2\test_windows_browser_network_socket_live.py`
  - `CMUX_BROWSER_NETWORK_REQUIRE_PROXY_BROKER_RECORD=1 ... python tests_v2\test_windows_browser_network_socket_live.py`
  - `CMUX_BROWSER_NETWORK_REQUIRE_PROXY_RECORD=1 ... python tests_v2\test_windows_browser_network_socket_live.py`

Latest implemented work:

- Windows desktop control socket now has a truthful `system.capabilities` endpoint listing handled methods, including browser network inspection and explicit WKWebView unsupported-gap methods.
- Browser WKWebView hard gaps now return explicit backend `not_supported` errors, and the CLI routes viewport/geolocation/offline/trace/screencast/raw-input commands to those explicit socket methods.
- CLI browser network inspection now accepts the stress-harness/user-friendly ordering `cmux browser --surface <id> network requests` as well as `cmux browser network --surface <id>`.
- CLI now exposes browser request inspection as `cmux browser network|network-requests|requests`, mapping to `browser.network.requests` with surface/workspace filters plus URL/method/since/limit filters.
- CLI `cmux browser` help now advertises the full Network inspection contract: URL, method, headers, body previews, status, timing, proxy attribution, record notes, and opaque proxy tunnel observations.
- Command Palette now has a browser-scoped `Show Network Requests` command that opens the Browser devtools Network lane.
- Browser devtools now has a real Network lane in the UI, backed by a new Tauri `browser_network_requests` command that reuses the same backend recorder as `browser.network.requests`.
- Browser devtools Network lane now mirrors backend filtering/count state more faithfully: URL/method filters, shown/matched/retained counts, active filter chips, and filter-aware empty-state copy.
- Browser devtools Network lane now exposes the backend cursor/limit controls too: `After request id`, `Limit`, active chips, and visible request IDs in rows.
- Browser network retention is now manageable from backend/API/CLI/UI: `browser.network.clear`, Tauri `browser_clear_network_requests`, `cmux browser network clear`, and a Network panel `Clear records` action.
- Command Palette now also exposes browser-scoped `Clear Network Records`, which calls `browser_clear_network_requests`, clears the local Network UI state, and opens the Network lane.
- v2 browser harness/tests now know about `browser.network.clear`, and the remote browser proof tests clear the per-panel Network buffer immediately before the navigation/subresource assertions they care about.
- v2 browser contract/live proof tests now require network records to expose the backend `note` field, and remote proxy proofs require the cleartext proxy note on main-document and favicon/subresource records.
- Browser devtools Network detail panes now show full backend timing metadata: started, completed, and duration, not just the row duration chip.
- Browser devtools Network detail panes are now self-contained: summary metadata plus request/response body size, preview kind, and truncation state are visible inside the expanded inspector.
- Browser devtools Network detail panes now render per-record backend notes, so opaque proxy tunnel caveats are visible on the exact record being inspected.
- Browser network records now expose explicit request/response body preview kind (`text`, `binary`, `empty`, `unavailable`) plus observer body-preview capture limit, and the Network UI displays preview labels/cap metadata.
- `browser.network.requests` now exposes bounded `responseBody` and `responseBodyTruncated`, matching the existing request-body capture and making proxy/custom-scheme body inspection symmetrical.
- Proxy tunnel observations now have an opaque fallback record path: when encrypted HTTPS/SOCKS5 tunnel bytes cannot be parsed as cleartext HTTP, `browser.network.requests` still records authority URL, `CONNECT`, successful tunnel status, byte preview metadata, attribution, and timing as `proxy-stream-tunnel`.
- The serialized `browser.network.requests` reply shape is now covered for opaque tunnel records, including camelCase fields such as `proxyAttribution`, `responseStatus`, body preview metadata, timing, and `note`.
- `cmux ssh` now has a real CLI orchestrator path, help text, SSH option parsing, workspace creation, remote configure call, relay port metadata, startup script generation, and JSON output.
- Remote reconnect plumbing exists through `workspace.remote.reconnect`, with private saved config and public `has_ssh_options` instead of leaking raw identity/options.
- Relay-map startup now retries in the background so the remote bootstrap can create `$HOME/.cmux/relay/<port>.daemon_path` before the proxy daemon transport is attempted.
- Browser proxy runtime now supports workspace brokers, per-panel brokers, panel-scoped observers, existing browser-pane migration after proxy readiness, and cleartext HTTP request/response observation into `browser.network.requests`.
- Session-level proxy observers now have direct bridge coverage: panel observers record `panel` attribution, workspace observers record only when one browser pane matches the proxy URL, and ambiguous shared workspace-proxy panes are skipped instead of misattributed.
- A reproducible Windows live-smoke harness now verifies a freshly built desktop named-pipe listener exposes browser Network requests/clear and returns the expected live JSON observer shape.
- `debug.browser.attach_webview` now lets the Windows live-smoke harness attach a real child WebView through the control socket; strict mode has passed against a local HTTP page and observed a live `wkwebview-navigation` Network record.

Known blockers / caveats:

- A fresh `target\debug\cmux-desktop.exe` was built and launched successfully in this environment; `target\debug\cmux.exe rpc system.identify` and `system.capabilities` now work against `\\.\pipe\cmux`.
- Live child-WebView proxy records are now proven locally via `debug.browser.start_direct_proxy` plus `debug.browser.attach_webview`; the strict smoke captures parsed URL, method, headers, response body, status, timing, and `panel` attribution.
- `CMUX_SSH_TEST_HOST`, `CMUX_SSH_TEST_PORT`, and `CMUX_SSH_TEST_IDENTITY` were unset when last checked; `docker` was not installed/available in PATH, so SSH/Docker browser proxy e2e could not run from this shell.
- Runnable `cmux-desktop` Rust tests often fail locally on this Windows machine with `STATUS_ENTRYPOINT_NOT_FOUND`; use `--no-run` compile gates unless investigating that loader issue.
- Docker SSH e2e has not been proven.
- There is a Python contract wrinkle: one test treats `ssh_startup_command` as a script path, another inspects it like command text. Current output preserves `ssh_startup_command` as path and adds `ssh_startup_command_text` for command consumers.

Recommended next slice:

1. Run a broad completion audit before marking the active goal complete: inspect `TODO.md`, command/socket capability coverage, generated TS types, Browser UI Network lane, CLI help, and live desktop behavior.
2. Keep the proxy proof commands handy:
   - `python tests_v2\test_windows_browser_network_socket_live.py`
   - `$env:CMUX_BROWSER_NETWORK_REQUIRE_PROXY_BROKER_RECORD='1'; python tests_v2\test_windows_browser_network_socket_live.py`
   - `$env:CMUX_BROWSER_NETWORK_REQUIRE_PROXY_RECORD='1'; python tests_v2\test_windows_browser_network_socket_live.py`
3. If an SSH test host appears, still run the remote browser proofs as extra coverage:
   - `python tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py`
   - `python tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py`
4. Compile with focused gates:

```powershell
cargo fmt -p cmux-desktop
cargo check -p cmux-desktop
cargo test -p cmux-desktop --lib --no-run
python -m py_compile tests_v2\test_windows_browser_network_socket_live.py
```

Useful narrow inspection commands:

```powershell
rg -n "struct BrowserNetwork|BrowserNetworkRecord|BrowserNetworkRequests|browser_network_requests_for_control|record_browser|record_proxy|record_proxy_http_exchange_observation" apps/desktop/src-tauri/src/browser.rs
Get-Content apps/desktop/src-tauri/src/browser.rs | Select-Object -Skip 300 -First 170
Get-Content apps/desktop/src-tauri/src/browser.rs | Select-Object -Skip 520 -First 160
Get-Content apps/desktop/src-tauri/src/browser.rs | Select-Object -Skip 920 -First 250
rg -n "browser.network.requests|browser_network_requests|BrowserNetwork" apps/desktop/src-tauri/src/control_socket.rs apps/desktop/web/src -S
```

# Continuation Checkpoint

Date: 2026-07-09

## Windows Browser Network Live Socket Smoke Slice - 2026-07-09

Added and ran a reproducible Windows live-smoke harness for the current desktop control socket; the final proxy observability row remains live WebView/SSH-test gated.

What changed:

- Added `tests_v2/test_windows_browser_network_socket_live.py`.
- The script is Windows-only and skips elsewhere.
- It uses `target/debug/cmux.exe` against the named pipe and starts `target/debug/cmux-desktop.exe` hidden only when no desktop listener is already reachable.
- It verifies:
  - `system.identify`;
  - `system.capabilities`;
  - advertised `browser.network.requests`, `browser.network.clear`, and `browser.open_split`;
  - browser surface creation through `browser.open_split`;
  - `browser.network.clear` returns the expected panel and cleared count;
  - `browser.network.requests` returns the expected live JSON shape, including observer source, filter support, body capture cap, proxy attribution mode, and note;
  - `browser.navigate` routes for the created browser surface.
- It closes the browser surface it creates and terminates only the desktop process it started itself.
- Added an opt-in strict record proof mode:
  - `CMUX_BROWSER_NETWORK_REQUIRE_RECORD=1`;
  - `CMUX_BROWSER_NETWORK_TEST_URL=<real URL>`;
  - optional `CMUX_BROWSER_NETWORK_URL_TOKEN=<expected URL substring>`;
  - optional `CMUX_BROWSER_NETWORK_TIMEOUT_S=<seconds>`.
- Added `debug.browser.attach_webview` to the control socket so strict mode can create the same native child WebView path normally driven by React before requiring a record.
- Strict mode validates each returned Network record shape, including request/response headers, body preview kind/size/truncation, status key, timing, proxy attribution key, and note key.

Manual/live evidence collected:

- `cargo build -p cmux-desktop` passed and produced a current `target/debug/cmux-desktop.exe`.
- Launched rebuilt `target/debug/cmux-desktop.exe` hidden, latest pid `12276`.
- `target\debug\cmux.exe rpc system.identify` returned app `cmux`, transport `windows-named-pipe`, pipe `\\.\pipe\cmux`.
- `target\debug\cmux.exe rpc system.capabilities` returned the current method list including `browser.network.requests`, `browser.network.clear`, and `debug.browser.attach_webview`.
- Manual `browser.open_split` created `surface-2`; `browser.network.clear` and `browser.network.requests` returned the expected live empty Network observer shape.
- A hidden-window navigation attempt to a local HTTP page updated the browser URL but did not produce navigation records, so it is not sufficient to check the final TODO row.
- Cleaned up the manual browser surface afterward; `surface.list` returned only the original terminal surface.
- After adding `debug.browser.attach_webview`, strict mode against a temporary local HTTP URL passed with `PASS: Windows desktop control pipe emitted a live browser Network record for http://127.0.0.1:<port>/index.html`.
- Environment check after the live smoke:
  - `CMUX_SSH_TEST_HOST`, `CMUX_SSH_TEST_PORT`, and `CMUX_SSH_TEST_IDENTITY` were unset;
  - `docker --version` failed because `docker` is not available in PATH.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo build -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib control_socket::tests::control_socket_methods_advertise_browser_network_and_platform_gaps --no-run` passed.
- `python -m py_compile tests_v2\test_windows_browser_network_socket_live.py` passed.
- `python tests_v2\test_windows_browser_network_socket_live.py` passed against the fresh desktop listener.
- `CMUX_BROWSER_NETWORK_REQUIRE_RECORD=1` with `CMUX_BROWSER_NETWORK_TEST_URL=about:blank` fails immediately with a clear error requiring a real URL.
- `CMUX_BROWSER_NETWORK_REQUIRE_RECORD=1` against a temporary local HTTP URL now passes after `debug.browser.attach_webview` creates the native child WebView.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live remote proxy traffic, ideally via SSH/WebView remote tests with `CMUX_SSH_TEST_HOST`, before claiming full completion for headers/body/status proxy observability.

## Browser Network Control JSON Contract Slice - 2026-07-09

Strengthened the raw API contract for browser Network records; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/src-tauri/src/browser.rs`.
- Added `browser::tests::network_reply_serializes_tunnel_record_for_control_socket_clients`.
- The test records an opaque SOCKS5 tunnel observation, calls `browser_network_requests_for_control`, serializes the reply with `serde_json::to_value`, and verifies the exact camelCase JSON fields consumed by the control socket, CLI JSON output, and Python v2 client:
  - `panelId`;
  - observer `source`;
  - observer `proxyAttributionMode`;
  - record `source`;
  - `transport`;
  - `proxyAttribution`;
  - `method`;
  - `url`;
  - `responseStatus`;
  - `requestBodyPreviewKind`;
  - `responseBodyPreviewKind`;
  - `requestBodySize`;
  - `responseBodySize`;
  - truncation flags;
  - `startedAtMs`;
  - `completedAtMs`;
  - `durationMs`;
  - `note`.
- The test also asserts snake_case aliases are not emitted in the serialized control payload.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib browser::tests::network_reply_serializes_tunnel_record_for_control_socket_clients --no-run` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH/WebView proxy proof before claiming full completion.

## Browser Network CLI Help Parity Slice - 2026-07-09

Closed a CLI discoverability gap for the browser Network inspector contract; the final proxy observability row remains live-test gated.

What changed:

- Updated `crates/cmux-cli/src/dispatch.rs`.
- `cmux browser` help now says `cmux browser network` inspects browser Network records with:
  - URL;
  - method;
  - headers;
  - body previews;
  - status;
  - timing;
  - proxy attribution;
  - record notes;
  - cleartext and opaque proxy tunnel observations.
- Added `dispatch::tests::browser_help_advertises_network_observability_fields` so CLI help cannot drift behind backend/UI Network record parity.

Verification:

- `cargo fmt -p cmux-cli` passed.
- `cargo test -p cmux-cli dispatch::tests::browser_help_advertises_network_observability_fields -- --nocapture` passed.
- `cargo test -p cmux-cli --lib -- --nocapture` passed, 93 tests.
- `cargo check -p cmux-cli` passed.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH/WebView proxy proof before claiming full completion.

## Browser Network Record Note Acceptance Slice - 2026-07-09

Strengthened the v2 acceptance harness for backend/UI parity around browser Network record notes; the final proxy observability row remains live-test gated.

What changed:

- Updated `tests_v2/test_browser_api_unsupported_matrix.py`.
- The generic browser network API contract now requires every returned Network record to include a `note` field, with a value that is either `null` or a string.
- Updated `tests_v2/test_ssh_remote_browser_move_rebinds_proxy.py`.
- The remote browser move/rebind proof now waits for a panel-attributed `proxy-stream-http` record whose note includes `HTTP request/response metadata parsed`.
- The final assertions also require the returned main-document proxy record to include that cleartext proxy note.
- Updated `tests_v2/test_ssh_remote_browser_favicon_uses_proxy.py`.
- The favicon/subresource proof now requires the same cleartext proxy note on the panel-attributed favicon record.

Why this matters:

- `note` is now part of the user-facing Network inspector parity story: cleartext records explain what was parsed, and opaque tunnel records explain what remains encrypted.
- The eventual live proof should fail if backend records carry data but omit the explanatory metadata the UI now renders.

Verification:

- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py tests_v2\cmux.py` passed.
- With `CMUX_SSH_TEST_HOST` unset, `python tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py` skipped cleanly.
- With `CMUX_SSH_TEST_HOST` unset, `python tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` skipped cleanly.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH/WebView proxy proof before claiming full completion.

## Browser Network Opaque Tunnel Note UI Slice - 2026-07-09

Closed a UI parity gap for the new opaque proxy tunnel backend records; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Browser Network expanded details now render a `Record Note` section whenever `record.note` is present.
- This surfaces backend caveats directly on opaque tunnel records, including the distinction between successful tunnel establishment and unavailable encrypted origin headers.
- Updated `apps/desktop/web/src/styles.css`.
- Added full-width record-note styling inside the Network details grid.
- Updated `apps/desktop/web/src/components/BrowserSurface.test.tsx`.
- Added an opaque `proxy-stream-tunnel` fixture covering:
  - `CONNECT`;
  - `https://secure.example/`;
  - `socks5`;
  - `panel proxy`;
  - status `200`;
  - timing;
  - binary request/response previews;
  - missing request/response headers;
  - rendered backend record note.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 15 tests / 120 expect calls.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts apps/desktop/web/src/palette/useCommandPalette.test.ts` passed, 128 tests / 820 expect calls.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH/WebView proxy proof before claiming full completion.

## Browser Proxy Opaque Tunnel Observability Slice - 2026-07-09

Closed a backend observability hole for encrypted remote browser traffic; the final TODO row remains live-test gated.

What changed:

- Updated `apps/desktop/src-tauri/src/browser.rs`.
- Added `record_proxy_tunnel_observation_with_attribution(...)`.
- Added `proxy-stream-tunnel` records for proxy observations whose captured bytes cannot be parsed as cleartext HTTP.
- Opaque tunnel records include:
  - authority URL, using `https://host/` for port 443 and `tcp://host:port/` otherwise;
  - method `CONNECT`;
  - `responseStatus: 200` to represent successful tunnel establishment;
  - bounded upstream/downstream byte previews with binary/text/empty preview kind and truncation flags;
  - started/completed/duration timing;
  - transport (`socks5` or `http-connect`);
  - proxy attribution (`panel` or `workspace`);
  - a note clarifying that encrypted origin headers remain unavailable without interception.
- Updated Network observer summary copy for tunnel-only and mixed tunnel/rich records.
- Updated `apps/desktop/src-tauri/src/session.rs`.
- The session proxy observer bridge now falls back to opaque tunnel records instead of silently dropping observations when HTTP parsing fails.
- Added Rust regression coverage:
  - `browser::tests::proxy_tunnel_observation_records_opaque_https_metadata`;
  - `session::tests::panel_proxy_observer_bridge_records_opaque_tunnel_metadata`, using SOCKS5 to cover the common remote browser proxy path.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib browser::tests::proxy_tunnel_observation_records_opaque_https_metadata --no-run` passed.
- `cargo test -p cmux-desktop --lib session::tests::panel_proxy_observer_bridge_records_opaque_tunnel_metadata --no-run` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed.
- Attempting to run the two focused desktop tests still hit the known local Windows/Tauri loader issue: `STATUS_ENTRYPOINT_NOT_FOUND`.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need a running desktop/control socket plus SSH/WebView proxy proof before claiming full completion.

## Browser Network Clear Palette Slice - 2026-07-09

Closed a small UI discoverability gap for browser Network retention management; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `dispatchBrowserNetworkCleared(...)` and a matching browser-surface listener so external UI actions can reset the visible Network records for the matching panel.
- Updated `apps/desktop/web/src/hooks/useSession.ts`.
- Added `clearBrowserNetworkRecords(...)`, wired to Tauri `browser_clear_network_requests`.
- Updated `apps/desktop/web/src/palette/commandCatalog.ts`, `intentPlan.ts`, and `useCommandPalette.ts`.
- Added a browser-scoped `Clear Network Records` command, planned as `clearBrowserNetworkRecords`, which invokes the backend clear, emits the local clear event, and opens Browser devtools to the Network lane.
- Updated `BrowserSurface.test.tsx`, `commandCatalog.test.ts`, and `intentPlan.test.ts`.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 92 tests / 705 expect calls.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts apps/desktop/web/src/palette/useCommandPalette.test.ts` passed, 127 tests / 806 expect calls.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH/WebView proxy proof before claiming full completion.

## Browser Network Self-Contained Detail UI Slice - 2026-07-09

Closed another UI inspection gap for browser/proxy observability; the final proxy row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `networkPreviewKindLabel(...)` and `networkBooleanLabel(...)`.
- Renamed the details disclosure from `Inspect headers/body` to `Inspect metadata/headers/body`.
- Added a `Summary` section inside each expanded Network record with:
  - id;
  - method;
  - response status;
  - transport;
  - source;
  - proxy attribution.
- Added body metadata rows above each request/response body preview:
  - size;
  - preview kind;
  - truncated yes/no.
- Updated `apps/desktop/web/src/styles.css` with body-metadata spacing.
- Updated `BrowserSurface.test.tsx` to assert the new self-contained metadata renders.

Why this matters:

- The row already had many chips, but the expanded inspector was not self-contained.
- For the final proxy observability proof, a user can now expand one record and see URL/method/status/source/transport/proxy context, timing, headers, body metadata, and body preview without cross-referencing row chips.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 13 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 115 tests.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Browser Network Timing Detail UI Slice - 2026-07-09

Closed a small UI inspection gap for the final browser/proxy observability row; the row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `networkTimestampLabel(...)`.
- Added a `Timing` section to each Network request details disclosure.
- The details now show:
  - `started` as backend `startedAtMs`;
  - `completed` as backend `completedAtMs` or `pending`;
  - `duration` using the same duration/fallback logic as the row chip.
- Updated `BrowserSurface.test.tsx` to assert the timing section and raw timing values render.

Why this matters:

- The remaining TODO explicitly calls out URL, method, headers, body, status, and timing.
- The Network UI already rendered URL/method/status/headers/body and a duration chip, but did not expose the full timing tuple returned by the backend.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 13 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 115 tests.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Browser Network Clear Harness/Acceptance Slice - 2026-07-09

Connected the new Network clear API to the v2 harness and live-proof tests; the final proxy observability row remains live-test gated.

What changed:

- Updated `tests_v2/cmux.py`.
  - Added `browser_network_clear(panel_id)`.
  - Resolves the surface id and calls `browser.network.clear`.
- Updated `tests_v2/test_browser_api_unsupported_matrix.py`.
  - Added `browser.network.clear` to `EXPECTED_BROWSER_METHODS`.
  - Exercises `browser.network.clear` and asserts:
    - returned panel/surface id;
    - integer `clearedCount` / `cleared_count`.
  - Updated PASS text to mention request inspection/clear.
- Updated `tests_v2/test_ssh_remote_browser_move_rebinds_proxy.py`.
  - Clears the browser Network buffer immediately after moving the browser into the SSH workspace and before navigating to the remote localhost URL.
  - Asserts the clear reply reports an integer cleared count before waiting for the panel-attributed proxy record.
- Updated `tests_v2/test_ssh_remote_browser_favicon_uses_proxy.py`.
  - Opens the browser split at `about:blank`.
  - Clears the browser Network buffer.
  - Navigates to the remote localhost page after the clear, so the favicon/subresource record must be produced by the current proof run.

Why this matters:

- The previous slice implemented Network clearing across backend/API/CLI/UI, but the v2 harness did not expose it.
- The remote proof tests previously waited for matching records in whatever was retained. Clearing immediately before the asserted navigation/subresource makes the live proof stronger and less vulnerable to stale records.

Verification:

- `python -m py_compile tests_v2\cmux.py tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` passed.
- `python tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote browser move/proxy regression`.
- `python tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote favicon proxy regression`.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Browser Network Clear Records Slice - 2026-07-09

Closed another Network inspector completion gap; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/src-tauri/src/browser.rs`.
- Added `BrowserNetworkClearReply`.
- Added `browser_clear_network_requests_for_control(...)`, which clears retained network records for one panel and returns `clearedCount`.
- Added Tauri command `browser_clear_network_requests`.
- Added backend test `network_requests_can_be_cleared_per_panel`.
- Updated `apps/desktop/src-tauri/src/lib.rs` to register the Tauri command.
- Updated `apps/desktop/src-tauri/src/control_socket.rs`.
  - Advertises `browser.network.clear`.
  - Handles `browser.network.clear` with the same browser-surface validation as `browser.network.requests`.
  - Existing capability test now asserts the clear method.
- Updated `crates/cmux-cli/src/command_forward.rs`.
  - Added `cmux browser network clear ...`.
  - Added aliases `cmux browser network-clear ...`, `clear-network`, and `clear-requests`.
  - Added CLI mapping tests.
- Updated `crates/cmux-cli/src/dispatch.rs` browser help to mention clearing network records.
- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
  - Added `BrowserNetworkClearReply`.
  - Added `Clear records` Network header action.
  - Calls `browser_clear_network_requests`, clears the local panel list, and refreshes.
- Updated `apps/desktop/web/src/styles.css` for Network header action layout and disabled button state.
- Updated `BrowserSurface.test.tsx` to assert the visible `Clear records` action.

Why this matters:

- The backend retained a bounded per-panel Network buffer, and UI/API/CLI could query it, but users had no way to reset the buffer without closing/recreating the pane.
- Long-running remote browser sessions need a clear action to isolate the next navigation/subresource proof cleanly.

Verification:

- `cargo fmt -p cmux-desktop -p cmux-cli` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 13 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `cargo test -p cmux-desktop --lib browser::tests::network_requests_can_be_cleared_per_panel --no-run` passed.
- `cargo check -p cmux-desktop -p cmux-cli` passed.
- `cargo test -p cmux-cli --lib -- --nocapture` passed, 92 tests.
- `cargo test -p cmux-desktop --lib control_socket::tests::control_socket_methods_advertise_browser_network_and_platform_gaps --no-run` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 115 tests.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run` passed.
- `python -m py_compile tests_v2\cmux.py tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` passed.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Browser Network Cursor/Limit UI Parity Slice - 2026-07-09

Closed another UI-behind-backend gap in the browser Network inspector; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `browserNetworkLimitValue(...)`.
- Extended `browserNetworkRequestParams(...)` to emit:
  - backend `limit` from UI input, clamped to `0..200` with default `50`;
  - backend `sinceId` from the new cursor input.
- Extended `browserNetworkActiveFilters(...)` to show active chips for:
  - `After: <request id>`;
  - non-default `Limit: <n>`.
- Added Network panel controls:
  - `After request id`;
  - `Limit`.
- `Clear` now resets URL, method, cursor, and limit state.
- Network rows now display each request id so the cursor field is discoverable/usable from the UI.
- Updated the filter grid in `apps/desktop/web/src/styles.css` for the expanded controls.
- Updated `BrowserSurface.test.tsx` to cover:
  - `sinceId` payload construction;
  - limit clamping/defaulting;
  - cursor/limit active chips;
  - rendered controls and visible request IDs.

Why this matters:

- The backend, control socket, and CLI already supported `sinceId` and configurable `limit` for `browser.network.requests`.
- Before this slice, the UI could only request the latest 50 and could not use the backend cursor. This made UI inspection weaker than CLI/API inspection for long-running browser panes.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 13 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 115 tests.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Proxy Observer Bridge Coverage Slice - 2026-07-09

Strengthened the non-live proof for the final browser/proxy observability row by testing the session observer bridge that feeds remote proxy bytes into `browser.network.requests`.

What changed:

- Updated `apps/desktop/src-tauri/src/session.rs`.
- Factored the production observers through small testable helpers:
  - `record_proxy_observation_for_browser_panel(...)`;
  - `record_workspace_proxy_observation_for_browser_panel(...)`;
  - `proxy_tunnel_protocol_label(...)`.
- `PanelBrowserProxyObserver` now delegates to the panel helper while preserving `panel` attribution.
- `WorkspaceBrowserProxyObserver` now delegates to the workspace helper while preserving conservative attribution behavior.
- Added session tests proving:
  - a panel proxy observation records `proxy-stream-http` metadata on the target browser panel with `proxyAttribution == "panel"`;
  - a workspace proxy observation records only when exactly one browser pane matches the workspace proxy URL, with `proxyAttribution == "workspace"`;
  - two browser panes sharing the same workspace proxy URL are treated as ambiguous and produce no record instead of a false attribution.

Why this matters:

- Earlier browser tests proved HTTP parsing and record storage.
- Earlier session tests proved browser-pane/proxy-url matching.
- This slice proves the missing bridge between those layers, which is the exact path live remote proxy observations use before the final WebView/SSH acceptance tests can pass.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib session::tests::panel_proxy_observer_bridge_records_panel_attributed_network_metadata --no-run` passed.
- `cargo test -p cmux-desktop --lib session::tests::workspace_proxy_observer_bridge_records_only_unambiguous_browser_panel --no-run` passed.
- `cargo test -p cmux-desktop --lib session::tests::workspace_proxy_observer_bridge_skips_ambiguous_shared_proxy_panels --no-run` passed.
- Attempting to execute the focused Rust tests with `cargo test -p cmux-desktop --lib proxy_observer_bridge -- --nocapture` still hit the known local Windows/Tauri loader issue: `STATUS_ENTRYPOINT_NOT_FOUND`.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run` passed.
- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`, so no live desktop listener was available.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Browser Network Body Preview Contract Slice - 2026-07-09

Made browser request/response body inspection more explicit in both backend payloads and UI; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/src-tauri/src/browser.rs`.
- `BrowserNetworkRecord` now includes:
  - `requestBodyPreviewKind`;
  - `responseBodyPreviewKind`.
- Preview kinds are:
  - `unavailable` for WKWebView navigation-only records;
  - `empty` for rich records with empty body bytes;
  - `text` for UTF-8 previews;
  - `binary` for non-UTF-8 previews, preserving the existing `<binary body: N bytes>` placeholder.
- `BrowserNetworkObserverSummary` now includes `bodyCaptureLimitBytes`, so API/UI consumers know the bounded preview cap.
- Updated proxy/custom-scheme/navigation tests to assert preview kind and capture-limit metadata.
- Added a backend binary-body test for proxy-observed request/response bodies.
- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
  - Network details now label body previews as text/binary/unavailable.
  - Network summary now shows the body preview cap when provided.
- Updated `BrowserSurface.test.tsx` to assert the visible preview label and capture-cap chip.
- Updated live/contract tests:
  - `tests_v2/test_browser_api_unsupported_matrix.py` now requires preview kind and body capture limit fields.
  - `tests_v2/test_ssh_remote_browser_move_rebinds_proxy.py` now expects text preview for the remote HTML/body assertion.
  - `tests_v2/test_ssh_remote_browser_favicon_uses_proxy.py` now expects binary preview for favicon/image body assertion.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 13 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib browser::tests::network_requests_records_navigation_metadata --no-run` passed.
- `cargo test -p cmux-desktop --lib browser::tests::custom_scheme_network_requests_capture_headers_status_body_size_and_timing --no-run` passed.
- `cargo test -p cmux-desktop --lib browser::tests::proxy_http_exchange_marks_binary_body_previews --no-run` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 115 tests.
- `python tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote browser move/proxy regression`.
- `python tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote favicon proxy regression`.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run` passed.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions with real WebView traffic.

## Browser Network Filter Count/Empty-State UI Slice - 2026-07-09

Closed a small but visible UI-behind-backend gap in the browser Network inspector; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `browserNetworkActiveFilters(...)` and `browserNetworkEmptyMessage(...)`.
- The Network summary now shows:
  - returned/shown count;
  - backend `filteredCount` as matched count;
  - total retained count;
  - active URL/method filter chips when filters are set;
  - existing observer/source/body/proxy capability chips.
- Empty network results now distinguish between no records yet and no records matching active filters.
- Added a subtle active-filter chip style in `apps/desktop/web/src/styles.css`.
- Updated `BrowserSurface.test.tsx` to cover active-filter summaries, filter-aware empty-state copy, and rendered matched count.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 13 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 115 tests.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions.

## 2026-07-09 Follow-Up: Control Socket Capabilities Endpoint

Advanced backend parity for v2 capability discovery, but did not mark the remaining proxy observability TODO complete.

What changed:

- Added `system.capabilities` to the Windows desktop control socket.
- The endpoint returns:
  - `version: 2`
  - `methods`
  - `transport: windows-named-pipe`
- The method list is intentionally truthful to what the current Windows control socket handles, including aliases.
- The list includes browser observability and platform-gap methods such as:
  - `browser.network.requests`
  - `browser.viewport.set`
  - `browser.geolocation.set`
  - `browser.offline.set`
  - `browser.trace.start`
  - `browser.trace.stop`
  - `browser.network.route`
  - `browser.network.unroute`
  - `browser.screencast.start`
  - `browser.screencast.stop`
  - `browser.input_mouse`
  - `browser.input_keyboard`
  - `browser.input_touch`
- Added a unit-style compile test ensuring the advertised method list includes browser network inspection and explicit unsupported WKWebView gaps.

Verification:

```powershell
cargo fmt -p cmux-desktop
cargo test -p cmux-desktop --lib control_socket::tests::control_socket_methods_advertise_browser_network_and_platform_gaps --no-run
cargo test -p cmux-desktop --lib control_socket::tests::not_supported_errors_use_socket_contract_shape --no-run
cargo check -p cmux-desktop
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_read_screen_capture_pane_parity.py tests_v2\test_rename_window_workspace_parity.py tests_v2\test_ssh_remote_cli_relay.py
```

Live socket status:

- `target\debug\cmux.exe rpc system.capabilities` and `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`; no desktop control socket listener is running in this environment.

Still incomplete:

- `TODO.md` remains unchecked.
- This improves backend capability discovery but does not prove live WebView/proxy traffic.

## 2026-07-09 Follow-Up: Browser Unsupported-Gap Contract

Advanced browser API/CLI parity for WKWebView platform gaps, but did not mark the remaining proxy observability TODO complete.

What changed:

- Added explicit backend `not_supported` arms in `control_socket.rs` for:
  - `browser.viewport.set`
  - `browser.geolocation.set`
  - `browser.offline.set`
  - `browser.trace.start`
  - `browser.trace.stop`
  - `browser.network.route`
  - `browser.network.unroute`
  - `browser.screencast.start`
  - `browser.screencast.stop`
  - `browser.input_mouse`
  - `browser.input_keyboard`
  - `browser.input_touch`
- Added CLI mappings for the same unsupported families where applicable:
  - `cmux browser <surface> viewport 800 600`
  - `cmux browser <surface> geo LAT LON`
  - `cmux browser <surface> offline true|false`
  - `cmux browser <surface> trace start|stop`
  - `cmux browser <surface> screencast start|stop`
  - `cmux browser <surface> input mouse|keyboard|touch ...`
- Improved the browser CLI parser so positional surface selectors before browser subcommands are preserved, e.g. `cmux browser surface-1 viewport 800 600`.
- Added focused CLI mapper coverage for these unsupported browser families.
- Added a control-socket unit check that `not_supported(...)` uses the expected socket error shape.

Verification:

```powershell
cargo fmt -p cmux-cli -p cmux-desktop
cargo test -p cmux-cli --lib command_forward::tests::maps_browser_subcommands -- --nocapture
cargo test -p cmux-desktop --lib control_socket::tests::not_supported_errors_use_socket_contract_shape --no-run
cargo test -p cmux-cli --lib -- --nocapture
cargo check -p cmux-desktop
python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_browser_cli_agent_port.py scripts\stress-cli-socket-api.py
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
```

Live socket status:

- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`; no desktop control socket listener is running in this environment.

Still incomplete:

- `TODO.md` remains unchecked.
- This improves the explicit browser API/CLI platform-gap contract; it does not prove live WebView/proxy traffic.

## 2026-07-09 Follow-Up: CLI Browser Network Harness Ordering

Advanced CLI/stress-harness compatibility for browser network observability, but did not mark the remaining TODO complete.

What changed:

- `browser_subcommand(...)` now uses a browser-specific splitter that can find the browser subcommand after selector flags.
- This supports the stress harness/user ordering:
  - `cmux browser --surface surface-1 network requests`
- Existing canonical forms still work:
  - `cmux browser network --surface surface-1`
  - `cmux browser network requests`
  - `cmux browser requests --panel surface-1`
- Added regression coverage for option-before-subcommand browser network requests.
- Added regression coverage so `cmux browser network requests` is treated as the network request command, not as a request for panel id `requests`.

Verification:

```powershell
cargo fmt -p cmux-cli
cargo test -p cmux-cli --lib command_forward::tests::maps_browser_subcommands -- --nocapture
cargo test -p cmux-cli --lib -- --nocapture
python -m py_compile scripts\stress-cli-socket-api.py tests_v2\test_browser_api_unsupported_matrix.py
cargo check -p cmux-cli
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
```

Live socket status:

- `target\debug\cmux.exe rpc system.identify` still failed with `could not connect to \\.\pipe\cmux`; no desktop control socket listener is running in this environment.

Still incomplete:

- `TODO.md` remains unchecked.
- This proves CLI/harness argument mapping, not live WebView/proxy traffic.

## 2026-07-09 Follow-Up: CLI Browser Network Requests

Advanced command-line parity for browser observability, but did not mark the remaining TODO complete.

What changed:

- Added `cmux browser network`, `cmux browser network-requests`, and `cmux browser requests`.
- These map to the existing `browser.network.requests` control-socket method.
- Added workspace/surface selector support through the existing CLI selector helpers.
- Added filter support:
  - `--url-contains` / `--urlContains` / `--url`
  - `--method`
  - `--since-id` / `--sinceId` / `--after-id` / `--afterId`
  - `--limit`
- Added validation so non-numeric `--limit` fails before socket I/O.
- Added `browser.network.requests` to ambient workspace scoping.
- Updated `cmux browser --help` to advertise `network`.
- Added CLI mapper coverage for full network request payloads and invalid limits.

Verification:

```powershell
cargo fmt -p cmux-cli
cargo test -p cmux-cli --lib command_forward::tests::maps_browser_subcommands -- --nocapture
cargo test -p cmux-cli --lib dispatch::tests::mapped_socket_command_help_is_concrete_for_control_aliases -- --nocapture
cargo test -p cmux-cli --lib -- --nocapture
cargo run -p cmux-cli --bin cmux -- browser --help
cargo check -p cmux-cli
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py
```

Still incomplete:

- `TODO.md` remains unchecked.
- This proves the CLI mapping and compile/test contract, not live WebView/proxy traffic.
- Live WebView/proxy traffic still needs to be exercised through a running desktop control socket/browser surface.

## 2026-07-09 Follow-Up: Command Palette Browser Network Inspector

Advanced browser observability UI discoverability, but did not mark the remaining TODO complete.

What changed:

- Added `palette.browserNetwork` to the command-palette intent registry.
- Added the `Show Network Requests` browser-scoped command row beside Console/React Grab.
- The row is searchable by `network`, `requests`, `headers`, `body`, `status`, `proxy`, and `webview`.
- `planIntent("browserNetwork", ...)` now opens the focused browser pane's developer-tools drawer on `panel: "network"`.
- Widened the command plan type to include the `"network"` browser devtools panel.
- Added catalog coverage proving the row is browser-scoped/searchable.
- Added intent coverage proving the command targets the active browser pane and opens the Network panel.

Verification:

```powershell
bun run --cwd apps/desktop/web typecheck
bun test apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts apps/desktop/web/src/palette/useCommandPalette.test.ts
bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts apps/desktop/web/src/palette/useCommandPalette.test.ts
cargo check -p cmux-desktop
python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py
```

Still incomplete:

- `TODO.md` remains unchecked.
- Live WebView/proxy traffic still has not been proven against a running desktop control socket/browser surface.

## 2026-07-09 Follow-Up: Browser Devtools Network UI

Advanced the UI side of the remaining browser proxy observability row, but did not mark it complete.

What changed:

- Added a Tauri command `browser_network_requests(...)` in `apps/desktop/src-tauri/src/browser.rs`.
- Registered the command in `apps/desktop/src-tauri/src/lib.rs`.
- The command reuses `browser_network_requests_for_control(...)`, so the desktop UI and socket API see the same retained request records and filters.
- Extended `BrowserSurface` with a fourth developer-tools lane: `Network`.
- The Network lane loads the latest 50 records for the pane and displays:
  - method,
  - URL,
  - response status,
  - timing,
  - request/response body sizes and truncation marker,
  - request/response header counts,
  - transport,
  - observer source,
  - observer summary/note.
- Added refresh/loading/error/empty states for the Network lane.
- Widened `useSession.showBrowserDeveloperTools(...)` to accept `"network"` so the lane can persist through existing session state.
- Added CSS for the network summary and request rows.
- Added static render coverage proving the Network tab and request rows render.

Verification:

```powershell
cargo fmt -p cmux-desktop
cargo check -p cmux-desktop
cargo test -p cmux-desktop --lib browser::tests::network_requests_records_navigation_metadata --no-run
bun run --cwd apps/desktop/web typecheck
bun test apps/desktop/web/src/components/BrowserSurface.test.tsx
bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/intentPlan.test.ts apps/desktop/web/src/palette/useCommandPalette.test.ts
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py
```

Still incomplete:

- `TODO.md` remains unchecked.
- The Network lane has compile/static-render proof, but not a live desktop/WebView proof in this environment.
- Live WebView/proxy traffic still needs to be exercised through a running desktop control socket/browser surface, ideally through the remote SSH/browser e2e.

## 2026-07-09 Follow-Up: Browser Network Response Body Inspection

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `response_body` / `responseBody` and `response_body_truncated` / `responseBodyTruncated` to `BrowserNetworkRecord`.
- Added `captures_response_body` / `capturesResponseBody` to the observer summary.
- `record_custom_scheme_network_request(...)` now captures bounded response body text and truncation state, matching existing request body behavior.
- `record_proxy_http_exchange_observation(...)` now captures bounded cleartext HTTP response body text and truncation state from proxy stream bytes.
- Strengthened browser unit coverage for:
  - navigation records exposing empty response-body fields,
  - custom-scheme response body capture,
  - proxy response body capture,
  - bounded/truncated proxy request and response body capture.
- Strengthened `tests_v2/test_browser_api_unsupported_matrix.py` so socket API records must expose response body and response truncation fields.

Verification:

```powershell
cargo fmt -p cmux-desktop
python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py
cargo check -p cmux-desktop
cargo test -p cmux-desktop --lib browser::tests::proxy_http_exchange_records_cleartext_request_response_metadata --no-run
cargo test -p cmux-desktop --lib browser::tests::proxy_http_exchange_marks_bounded_request_and_response_body_capture --no-run
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py
```

Still incomplete:

- `TODO.md` remains unchecked.
- Live WebView/proxy traffic still has not been proven against a running desktop control socket/browser surface.
- Docker SSH e2e remains unproven in this environment.

## 2026-07-09 Compact Handoff: Remote Browser Proxy / `cmux ssh`

The active goal is still **not complete**. We have made major remote-browser-proxy progress, but the remaining unchecked TODO is:

- `TODO.md`: `Per-WKWebView proxy observability/inspection once remote proxy path is shipped (URL, method, headers, body, status, timing)`

Do not check that item yet. The proxy broker, daemon RPC foundation, browser network recorder, and pane-scoped proxy wiring are in place, but the Windows `cmux ssh` command is still not fully wired to create/configure a live remote workspace with relay metadata end-to-end.

### Current Dirty/Active Files

- `apps/desktop/src-tauri/src/remote_proxy.rs` is new/untracked and important.
- `apps/desktop/src-tauri/src/control_socket.rs` is untracked in status but contains active control-socket work; do not discard.
- `apps/desktop/src-tauri/src/session.rs` has large active changes for persistence, pane ids, remote metadata, browser proxy URLs, and panel broker observers.
- `apps/desktop/src-tauri/src/lib.rs` registers `remote_proxy`.
- `crates/cmux-ssh/src/ssh_batch.rs` contains relay-map daemon transport support.
- `TODO.md` has only one unchecked item left, but it is not actually safe to mark complete.

Path-limited status before this handoff showed:

```text
 M TODO.md
 M apps/desktop/src-tauri/src/lib.rs
 M apps/desktop/src-tauri/src/session.rs
 M crates/cmux-ssh/src/ssh_batch.rs
?? CONTINUATION.md
?? apps/desktop/src-tauri/src/control_socket.rs
?? apps/desktop/src-tauri/src/remote_proxy.rs
```

### Remote Proxy Work Already Landed

- Browser network API: `browser.network.requests` records navigation/custom-scheme/proxy observations, including request/response headers, body sizes/truncation, and timing.
- WebView proxy URL support: browser attach/update accepts `proxy_url`; Wry/WebView2 receives `http://host:port` or `socks5://host:port`; pane snapshots expose `browser_proxy_url`.
- Remote workspace metadata: `workspace.remote.status/configure/disconnect/clear` plus remote daemon/proxy snapshot payloads.
- `workspace.remote.configure` accepts `remote_daemon_path`, `remote_daemon_relay_port`, `identity_file`, and `ssh_options`, with snake_case and camelCase variants where applicable.
- `remote_proxy.rs` implements SOCKS5 and HTTP CONNECT handshakes, a loopback broker, daemon JSON-lines RPC, daemon process launch, SSH daemon launch, and relay-map daemon launch.
- `crates/cmux-ssh/src/ssh_batch.rs` now has `daemon_transport_arguments_from_relay_map(relay_port)`, reading `$HOME/.cmux/relay/<relay_port>.daemon_path` and running `serve --stdio`.
- Broker traffic observation captures bounded upstream/downstream byte prefixes and feeds cleartext HTTP proxy observations into browser network records.
- Pane-scoped proxy foundation exists: when a workspace daemon runtime is live, new browser panes can get unique local broker ports and `PanelBrowserProxyObserver` attributes traffic directly to that panel.

### Last Known Passing Gates

These passed after the per-pane broker identity work:

```powershell
cargo fmt -p cmux-ssh -p cmux-desktop
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py
```

Avoid relying on runnable `cmux-desktop` Rust test binaries on this Windows machine: they previously failed locally with `STATUS_ENTRYPOINT_NOT_FOUND`. Prefer `--no-run` compile gates unless specifically investigating that loader issue.

### Latest CLI Inspection Findings

The next bottleneck is `cmux ssh` command orchestration.

- `crates/cmux-cli/src/classify.rs` recognizes `ssh` as a known socket-backed command.
- `crates/cmux-cli/src/dispatch.rs` currently supports only:
  - `RunRpc`
  - `RunControl(ControlCommand)`
  - local diff-viewer commands
  - hooks installer
  - failures/help/version
- `crates/cmux-cli/src/main.rs` executes one control method per `RunControl`.
- `crates/cmux-cli/src/command_forward.rs` does **not** map `ssh` yet.
- Existing backend seams can probably support a CLI-side orchestrator:
  - call `workspace.create` with `initial_terminal_command`, `initial_terminal_input`, and `initial_terminal_environment`;
  - call `workspace.remote.configure` with `remote_daemon_relay_port`, destination, port, identity, ssh options, local proxy port, etc.;
  - print the expected `cmux ssh` payload.
- Do not fake this as a one-shot `ControlCommand`: tests expect multi-step behavior, generated startup scripts, relay-specific daemon path mapping, and rich JSON payload.

### `cmux ssh` Test Contract To Preserve

`tests_v2/test_ssh_remote_cli_metadata.py` and `tests_v2/test_ssh_remote_cli_relay.py` expect `cmux ssh` to:

- Support `cmux ssh --help` with a `cmux ssh` header and “Create a new workspace” wording.
- Create/select a workspace and return `workspace_id` or resolvable `workspace_ref`.
- Return `remote_relay_port`, `ssh_command`, `ssh_terminal_command`, `ssh_startup_command`, and `ssh_env_overrides`.
- Generate a local startup script whose text stages a remote bootstrap under `$HOME/.cmux/relay/<port>.bootstrap.sh`.
- Install/use `$HOME/.cmux/bin/cmux` and `$HOME/.cmux/relay/<port>.daemon_path` on the remote host.
- Preserve shell integration variables including `CMUX_SOCKET_PATH`, `CMUX_WORKSPACE_ID`, `CMUX_TAB_ID`, `CMUX_SURFACE_ID`, and `CMUX_PANEL_ID`.
- Configure remote metadata without exposing raw `ssh_options` or `identity_file` in public payloads.
- Give each workspace a distinct relay port/control path.
- Honor `--ssh-option` overrides such as `StrictHostKeyChecking=no`, lowercase variants, and ControlMaster/ControlPersist/ControlPath overrides.

### Recommended Next Slice

Implement a real `DispatchPlan::RunSsh(Vec<String>)` or similar multi-step executor, not a normal `RunControl`.

Suggested path:

1. Add `cmux-ssh` as a dependency of `cmux-cli`.
2. Add a pure `ssh` parser module in `crates/cmux-cli` that handles the tested flags first: destination positional, `--port`, `--name`, `--identity`, repeated `--ssh-option`.
3. Add pure helpers that build:
   - durable SSH options with cmux defaults,
   - unique ControlPath when not overridden,
   - `ssh_command`,
   - `ssh_terminal_command`,
   - local `ssh_startup_command` file,
   - remote bootstrap text.
4. Add `DispatchPlan::RunSsh(Vec<String>)` and route `command == "ssh"` before `control_command_for`.
5. In `main.rs`, implement the orchestrator:
   - resolve socket/password once,
   - `workspace.create`,
   - derive workspace id/ref/window id from response,
   - call `workspace.remote.configure` with `remoteDaemonRelayPort` and `auto_connect: true`,
   - print a combined JSON payload.
6. Only after that passes compile gates, continue toward a live Docker e2e.

Be conservative: it is better to land this in tested pure slices than to fake the payload and confuse the remaining proxy-observability TODO.

### 2026-07-09 Follow-Up: `cmux ssh` CLI Orchestrator Slice

The Windows CLI now has a real `cmux ssh` route instead of falling through to “socket command not ported”.

Implemented in this slice:

- Added `crates/cmux-cli/src/ssh.rs`.
- Added `cmux-ssh`, `base64`, and `uuid` dependencies to `cmux-cli`.
- Added `DispatchPlan::RunSsh(Vec<String>)` and routed `command == "ssh"` to it.
- Added concrete `cmux ssh --help` usage text with “Create a new workspace”.
- Added SSH argument parsing for:
  - destination positional,
  - `--port` / `-p`,
  - `--name`,
  - `--identity` / `-i`,
  - repeated `--ssh-option`.
- Added invocation parsing protection so `--ssh-option`, `--identity`, `-i`, and `-p` keep their following values paired.
- Added a pure SSH command planner that builds:
  - `ssh_command`,
  - `ssh_terminal_command`,
  - generated startup script text,
  - remote bootstrap script text,
  - default/override-aware `StrictHostKeyChecking`, `ControlMaster`, `ControlPersist`, and `ControlPath`,
  - relay metadata and `ssh_env_overrides`.
- Added a Windows executor path in `crates/cmux-cli/src/main.rs` that:
  - writes the generated startup script under temp as `cmux-ssh-startup-<uuid>.sh`,
  - calls `workspace.create` with `initial_terminal_command` and env overrides,
  - calls `workspace.remote.configure` with destination/port/identity/options, local proxy port, persistent slot, and `remoteDaemonRelayPort`,
  - prints merged JSON including `remote_relay_port`, `local_proxy_port`, `ssh_command`, `ssh_terminal_command`, `ssh_startup_command`, `ssh_env_overrides`, and `remote`.

Verification after this slice:

```powershell
cargo fmt -p cmux-cli
cargo test -p cmux-cli --lib -- --nocapture
cargo check -p cmux-cli
cargo test -p cmux-cli --bins --no-run
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py
```

Important caveats:

- This is not yet proven against the live Docker SSH e2e.
- `workspace.remote.reconnect` was missing at this point in the handoff; it is implemented in the follow-up section below.
- The executor currently calls `workspace.remote.configure` immediately after `workspace.create`; live e2e may need bootstrap-readiness/retry timing so the relay-map daemon path exists before daemon transport starts.
- The existing Python tests appear to have a contract wrinkle around `ssh_startup_command`: metadata test treats it as a generated script path, while relay test inspects it like command text. Do not “fix” this with fake strings; validate canonical behavior and add explicit payload aliases if needed.
- The remaining `TODO.md` browser proxy observability item is still unchecked.

### 2026-07-09 Follow-Up: `workspace.remote.reconnect`

Closed the next obvious backend gap for the SSH metadata flow.

Implemented in this slice:

- Added a safe public `has_ssh_options` boolean to `SessionWorkspaceRemoteSnapshot`.
- `configured_remote_snapshot(...)` sets `has_ssh_options` when identity/options are present, without exposing raw `identity_file` or `ssh_options`.
- `SessionState` now keeps private in-memory `remote_configs` keyed by workspace id.
- `configure_workspace_remote_for_control(...)` stores the private config for later reconnect.
- `clear_workspace_remote_for_control(...)` removes the private config and stops the workspace broker.
- Added `reconnect_workspace_remote_for_control(...)`, which reuses the saved config with `auto_connect: true`.
- Registered `workspace.remote.reconnect` in `apps/desktop/src-tauri/src/control_socket.rs`.
- Cleared/unconfigured reconnect now returns `invalid_state` with “not configured”, matching `tests_v2/test_ssh_remote_cli_metadata.py`.

Verification after this slice:

```powershell
cargo fmt -p cmux-core -p cmux-desktop
cargo test -p cmux-core session::tests:: -- --nocapture
cargo test -p cmux-desktop --lib control_socket::tests::workspace_list_payload_includes_remote_proxy_endpoint --no-run
cargo check -p cmux-desktop
cargo test -p cmux-cli --lib -- --nocapture
```

Known local limitation:

- The runnable desktop test binary still fails on this Windows machine with `STATUS_ENTRYPOINT_NOT_FOUND`; the corresponding `--no-run` compile gate passes. This matches the earlier known local limitation.

Recommended next slice:

1. Build/run the `cmux` CLI locally against a live app/control socket if possible and exercise `cmux ssh --help`.
2. Run or simulate `cmux ssh 127.0.0.1 --port 1 --name ...` enough to inspect the returned JSON and generated startup script.
3. Then tackle bootstrap timing/readiness for the relay-map daemon path before attempting the Docker SSH e2e.

### 2026-07-09 Follow-Up: `cmux ssh` Shell Feature Merge + CLI Smoke

Tightened the CLI planner against the remaining SSH metadata contract.

Implemented in this slice:

- `SshCommandBuildOptions` now accepts `existing_ghostty_shell_features`.
- Windows `run_ssh_command(...)` passes ambient `GHOSTTY_SHELL_FEATURES` into the planner.
- `ssh_env_overrides.GHOSTTY_SHELL_FEATURES` now merges existing comma-separated features and appends `ssh-env,ssh-terminfo` without duplicates.
- Added planner regression coverage for `cursor,title -> cursor,title,ssh-env,ssh-terminfo`.
- Ran the real built CLI help path:
  - `cargo run -p cmux-cli --bin cmux -- ssh --help`
  - Output starts with `cmux ssh` and includes “Create a new workspace...”.
- Ran a real built CLI invalid-port smoke:
  - `cargo run -p cmux-cli --bin cmux -- ssh 127.0.0.1 --port 0`
  - It fails before socket I/O with `Error: cmux ssh --port must be 1-65535`, as expected.

Verification after this slice:

```powershell
cargo fmt -p cmux-cli
cargo test -p cmux-cli --lib ssh::tests:: -- --nocapture
cargo run -p cmux-cli --bin cmux -- ssh --help
cargo run -p cmux-cli --bin cmux -- ssh 127.0.0.1 --port 0
cargo test -p cmux-cli --lib -- --nocapture
cargo check -p cmux-cli
cargo test -p cmux-cli --bins --no-run
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py
```

Current caveats remain:

- The full live `cmux ssh` flow still needs to be exercised against a running app/control socket.
- The relay/bootstrap install path is not yet proven against Docker SSH e2e.
- The `ssh_startup_command` Python tests still have an apparent path-vs-command-text tension; validate canonical behavior before changing output shape.

### 2026-07-09 Follow-Up: Relay-Map Bootstrap Timing

Closed another likely Docker SSH e2e blocker.

Implemented in this slice:

- `ssh_terminal_command` now embeds the generated startup script as base64, decodes it into the remote `cmux_tmp`, `chmod +x`s it, then runs `/bin/sh "$cmux_tmp"`.
- This fixes the previous gap where the command created `cmux_tmp` but did not populate it before execution.
- Relay-map daemon startup in `configure_workspace_remote_for_control(...)` no longer fails permanently on the first attempt when `$HOME/.cmux/relay/<port>.daemon_path` does not exist yet.
- For `remote_daemon_relay_port`, the desktop now schedules a bounded background retry loop:
  - checks the workspace still exists,
  - retries SSH daemon transport from the relay-map path,
  - marks the remote proxy/daemon `connected`/`ready` on success,
  - marks `error` with `bootstrap failed after retry N: ...` on exhaustion.
- Initial auto-connect remote state is now `connecting` / daemon `bootstrapping` / proxy `connecting` until the broker is actually ready.

Verification after this slice:

```powershell
cargo fmt -p cmux-cli
cargo test -p cmux-cli --lib ssh::tests:: -- --nocapture
cargo check -p cmux-cli
cargo fmt -p cmux-desktop
cargo check -p cmux-desktop
cargo test -p cmux-desktop --lib session::tests:: --no-run
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
cargo test -p cmux-cli --lib -- --nocapture
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py
```

Live control socket status:

- `target\debug\cmux.exe rpc system.identify` failed with `could not connect to \\.\pipe\cmux` because no desktop control socket is running in this environment.
- `Get-Process` only showed the CLI `cmux.exe`, not the desktop app/control listener.

Recommended next slice:

1. Start the desktop app/control socket, or use an available harness, then run `cmux ssh 127.0.0.1 --port 1 --name ssh-meta-test` and inspect JSON/startup script.
2. If Docker is available, attempt `tests_v2/test_ssh_remote_cli_relay.py` next; the relay-map retry was specifically added for that path.
3. Resolve the remaining `ssh_startup_command` path-vs-command-text contract if the live tests expose it.

### 2026-07-09 Follow-Up: SSH Startup Payload Contract

Tightened the CLI output contract around the generated SSH bootstrap payload.

Implemented in this slice:

- Added non-breaking `ssh_startup_command_text` to the `cmux ssh` JSON output.
- `ssh_startup_command` remains the generated local startup script path for the metadata contract.
- `ssh_startup_command_text` exposes the remote command text that contains `PATH="$HOME/.cmux/bin:$PATH"` and `CMUX_SOCKET_PATH=127.0.0.1:<relay_port>`, addressing the path-vs-command-text ambiguity without breaking the path contract.
- Added a planner test that decodes `cmux_remote_bootstrap_b64` from the generated startup script and verifies the canonical remote bootstrap markers:
  - PATH and CMUX socket exports,
  - workspace/surface/panel placeholder exports,
  - login shell branching,
  - zsh/bash wrapper installation,
  - relay TTY report and ports kick commands.

Verification after this slice:

```powershell
cargo fmt -p cmux-cli
cargo test -p cmux-cli --lib ssh::tests:: -- --nocapture
cargo check -p cmux-cli
cargo test -p cmux-cli --lib -- --nocapture
cargo test -p cmux-cli --bins --no-run
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py
```

Notes:

- This does not by itself make the older relay test consume `ssh_startup_command_text`; it gives the CLI an explicit payload for command-text consumers while preserving `ssh_startup_command` as a path.
- Full validation still requires a live desktop control socket and, ideally, the Docker SSH fixture.

### 2026-07-09 Follow-Up: Existing Browser Pane Proxy Attribution

Tightened the remaining browser proxy observability path.

Implemented in this slice:

- Added `browser_panels_for_workspace(...)` / `collect_browser_panels(...)` to find all existing browser panes in a remote workspace.
- When a workspace daemon/proxy becomes ready, both the explicit daemon-path path and the relay-map retry path now call `start_existing_browser_panel_proxies_for_workspace_control(...)`.
- Existing browser panes are upgraded from the shared workspace proxy URL to individual panel-scoped local broker URLs when the remote proxy runtime becomes available.
- Those panel brokers use `PanelBrowserProxyObserver`, so proxy observations record directly to the panel id instead of relying on URL matching through `WorkspaceBrowserProxyObserver`.
- Added a regression proving the browser-pane collector returns existing browser panes and excludes terminal panes.

Verification after this slice:

```powershell
cargo fmt -p cmux-desktop
cargo check -p cmux-desktop
cargo test -p cmux-desktop --lib session::tests::browser_panels_for_workspace_collects_existing_browser_panes --no-run
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
cargo test -p cmux-cli --lib -- --nocapture
python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py
```

Important caveat:

- `TODO.md` still keeps `Per-WKWebView proxy observability/inspection...` unchecked because live WebView/proxy traffic has not been proven against a running desktop control socket/browser surface in this environment.

## Active Goal

Reach feature and UI parity between `ashlr-mux` and `cmux`, with both frontend and backend fully fleshed out. The current working assumption is that the web/Tauri UI is behind the backend and should be advanced slice-by-slice with tests.

Do not mark this goal complete until there is a comprehensive parity audit and proof, not just one feature slice.

## Operating Notes

- Workspace: `C:\Users\User\coding\work\ashlr-mux\cmux`.
- Use `apply_patch` for manual edits.
- The worktree is very dirty from ongoing parity work; do not revert unrelated changes.
- Treat current files as authoritative.
- `apply_patch` paths are relative to `C:\Users\User\coding\work\ashlr-mux`, so prefix repo paths with `cmux/`.
- Prefer small targeted `rg` searches after compaction; broad searches were truncating.

## Recently Completed Slices

### Active Callback Scheme

Implemented active callback scheme parity for navigation links.

- Added `apps/desktop/src-tauri/src/auth_environment.rs`.
- Registered `active_callback_scheme` in `apps/desktop/src-tauri/src/lib.rs`.
- Updated session deep-link parsing to accept active schemes such as `cmux-dev://workspace`.
- Updated palette link builders and intent plans to use the active scheme.
- Verified with Rust auth/session tests, palette tests, and `bun run typecheck`.

### Browser Import Wizard And Start Command

Implemented a Settings-based browser import flow and backend start command.

- Added `apps/desktop/web/src/settings/browserImportPlan.ts`.
- Added source profile selection, scope controls, separate/merge mode controls, destination selectors, Start Import button, and status text to Settings.
- Added Tauri command `browser_import_start`.
- Added request validation and capture support via `CMUX_UI_TEST_BROWSER_IMPORT_CAPTURE_PATH`.
- Verified with backend browser import tests, SettingsPane/browser import plan tests, and `bun run typecheck`.

### Blank Browser Import Hint

Implemented an import hint on blank browser tabs.

- Added hint buttons to `BrowserSurface`:
  - `BrowserImportHintImportButton`
  - `BrowserImportHintSettingsButton`
  - `BrowserImportHintDismissButton`
- Wired workspace and app settings callbacks.
- Added styles and component tests.
- Verified with BrowserSurface, SettingsPane, browserImportPlan tests, and `bun run typecheck`.

### Browser Import Fixture/Capture Parity

Added deterministic test fixture/capture hooks matching canonical UI test contracts.

- `browser_import_profiles()` now honors `CMUX_UI_TEST_BROWSER_IMPORT_FIXTURE`.
- Fixture supports `{"browserName":"Helium","profiles":["You","austin"]}`.
- Synthetic profile paths use `cmux-ui-test://browser-import/...`.
- `browser_import_start` capture writes canonical camelCase JSON:
  - `mode`
  - `scope`
  - `entries`
  - `sourceProfiles`
  - `destinationKind`
  - `destinationName`
  - optional `destinationProfileId`
- Verified with `cargo fmt`, backend browser import tests, frontend browser import tests, and `bun run typecheck`.

### Browser Import Destination Profile Parity

Added backend-owned browser import destination profiles and wired them through the Settings UI.

- `browser_import_destination_profiles` returns destination profiles from Tauri.
- The command honors `CMUX_UI_TEST_BROWSER_IMPORT_DESTINATIONS`, including the canonical `["Default"]` fixture.
- Destination fixtures can also use object entries with `id`, `displayName`, and `isDefault`.
- `App.tsx` loads destination profiles on startup and refresh.
- `SettingsOverlay` and `SettingsPane` pass destination profiles through to the browser import controls.
- Merge and single-destination imports use the selected existing destination profile.
- Separate-profile imports still default to canonical create-new behavior, but their per-profile selectors also expose existing destination choices.
- Verified with `cargo fmt`, backend browser import tests, focused Settings/browser-import tests, and `bun run typecheck`.

### Settings-Open UI-Test Capture Hook

Added the canonical capture hook used by blank browser import hint Settings tests.

- Added `apps/desktop/src-tauri/src/ui_test_hooks.rs`.
- Registered Tauri command `settings_open_capture`.
- The command writes `{opened,target,used_open_window_override}` to `CMUX_UI_TEST_SETTINGS_OPEN_CAPTURE_PATH` when that env var is set.
- `App.tsx` invokes the hook from the shared `openSettings` callback, so `BrowserImportHintSettingsButton` targeting `browserImport` is captured without blocking normal Settings opening.
- Verified with `cargo fmt`, `cargo test -p cmux-desktop --lib ui_test_hooks::tests:: -- --nocapture`, and `bun run typecheck`.

### Browser Import Next-Driven Wizard Flow

Moved the browser import UI closer to canonical flow behavior.

- `SettingsPane` now starts browser import as a `Next`-driven wizard.
- Step 1 introduces detected browsers.
- Step 2 shows source-profile selection.
- Step 3 shows cookies/history/additional-data, separate-vs-merge, destination selectors, status, and `Start Import`.
- Existing static tests can request Step 3 through `browserImportInitialWizardStep` so final-state controls remain easy to verify without a DOM interaction harness.
- Verified with `bun test src/components/SettingsPane.test.tsx src/settings/browserImportPlan.test.ts` and `bun run typecheck`.

### Browser History Clear Double Flash

Closed the TODO bug where browser `cmd+shift+H`/clear-history feedback only flashed once.

- Added `dispatchPanelFlashSequence` in `Workspace.tsx`.
- Browser toolbar clear-history now dispatches two timed flash pulses before clearing history.
- Palette clear-history dispatch also uses the same two-pulse helper.
- Added a Workspace component test proving the two-pulse event sequence.
- Verified with `bun test src/components/Workspace.test.tsx src/components/BrowserSurface.test.tsx` and `bun run typecheck`.

### Background Terminal Title Updates

Closed the P0 TODO where terminal title updates were suppressed while a workspace was not focused.

- Added backend OSC window-title parsing to the terminal PTY pump.
- The parser recognizes OSC `0;title` and `2;title` with BEL or ST terminators and preserves split sequences across chunks.
- The pump now updates the owning workspace process title from Tauri via `set_process_title_for_panel`, so sidebar/switcher title freshness no longer depends solely on a mounted xterm `onTitleChange` handler.
- The existing frontend `onTitleChange` path remains as a live-view backup and dedupes through the session mutation gate.
- Verified with `cargo fmt`, `cargo test -p cmux-desktop --lib terminal::tests:: -- --nocapture`, `cargo test -p cmux-core set_process_title -- --nocapture`, and `bun run typecheck`.

### Stale TODO Cleanup

Closed stale TODOs after verifying the current implementation.

- `Add cmd+shift+p palette with all commands` is already implemented by `useCommandPalette`/`CommandPaletteOverlay` and covered by command catalog plus interaction tests.
- `Right-click tab should allow renaming that workspace` is already implemented by workspace row context menus and inline rename state.
- Verified with `bun test src/palette/commandCatalog.test.ts src/palette/useCommandPalette.test.ts src/components/CommandPaletteInteractions.test.tsx` and `bun test src/sidebar/contextMenu.test.ts src/components/WorkspaceList.test.tsx src/components/Sidebar.test.tsx`.

### Terminal File Drop Paths

Closed the terminal drag/drop bug where dropped files/images inserted URLs instead of filesystem paths.

- Added terminal drop helpers that extract local paths from nonstandard `File.path`, `text/uri-list`, and `text/plain` `file://` payloads.
- `file:///C:/...` drops normalize to Windows paths and `file://server/share/...` drops normalize to UNC paths.
- Paths with spaces are quoted before being sent to the PTY.
- `TerminalSurface` now intercepts file drops and writes the normalized path payload through `terminal_write`, preventing the webview/xterm default URL insertion.
- Verified with `bun test src/components/TerminalSurface.test.ts` and `bun run typecheck`.

### Active Tab Close Button Visibility

Closed the UI TODO where the current/active tab close button was only visible on hover.

- Selected workspace rows now add `is-visible` to the close affordance.
- CSS keeps `.cmux-sidebar-row-close.is-visible` visible even without row hover.
- Added a WorkspaceList regression test for selected-row close visibility.
- Verified with `bun test src/components/WorkspaceList.test.tsx` and `bun run typecheck`.

## Most Likely Next Slice

Re-scan for the next highest-value parity gap outside the browser-import seam.

Browser import now has:

- source fixture support,
- destination fixture support,
- canonical start capture,
- settings-open capture,
- blank-tab hint controls,
- and a `Next`-driven Settings-hosted import flow.

Remaining browser-import caveats:

- The flow is still hosted inside Settings rather than a separate native window named `Import Browser Data`.
- Actual data migration into a Windows browser-profile store is still not implemented.

Next work should start with targeted scans of remaining unchecked or caveated areas in `BACKLOG.md`, plus any canonical UI tests that still reference unsupported Windows/Tauri behavior.

## Canonical Browser Import Test Facts

`cmuxUITests/BrowserImportProfilesUITests.swift` expects:

- Browser fixture env: `CMUX_UI_TEST_BROWSER_IMPORT_FIXTURE`.
- Destination fixture env: `CMUX_UI_TEST_BROWSER_IMPORT_DESTINATIONS`.
- Capture env: `CMUX_UI_TEST_BROWSER_IMPORT_CAPTURE_PATH`.
- Multiple source profiles default to separate profile import.
- Merge mode uses `BrowserImportDestinationPopup-merge`.
- Additional data changes capture `scope` to `everything`.
- Hint settings click should eventually capture settings-open target `"browserImport"`.
- Hint dismiss should hide the hint.

Current web UI is Settings-section based, not the exact canonical standalone multi-step wizard. Keep advancing parity pragmatically while preserving tests.

## Last Known Verification

Recent commands that passed:

- `cargo fmt`
- `cargo test -p cmux-desktop --lib browser_import::tests:: -- --nocapture`
- `cargo test -p cmux-desktop --lib ui_test_hooks::tests:: -- --nocapture`
- `cargo test -p cmux-desktop --lib auth_environment::tests:: -- --nocapture`
- `cargo test -p cmux-desktop --lib session::tests::parse_session_navigation_uri -- --nocapture`
- `bun test src/components/BrowserSurface.test.tsx src/components/SettingsPane.test.tsx src/settings/browserImportPlan.test.ts`
- `bun test src/settings/browserImportPlan.test.ts src/components/SettingsPane.test.tsx`
- `bun test src/palette/cmuxNavigationLinks.test.ts src/palette/intentPlan.test.ts`
- `bun run typecheck`

## Post-Compact Snapshot

The two most recent UI slices are complete and verified:

- Terminal drag/drop now writes normalized filesystem paths instead of `file://` URLs.
- The selected sidebar tab close button is visible without hover.
- Terminal keyboard focus is restored when returning from a browser surface to a visible focused terminal pane.
- Delivered notifications marked unread now move to the top of the notification center list, with a UI action and backend command.
- Sidebar tab reorder drag state now clears on cancelled/outside drag endings.
- Terminal surfaces show a loading status while the backend session is opening.
- Sidebar/tab header now has a browser workspace icon immediately left of the plus button.
- Notification popover buttons now show outside focus/hover outlines.
- Delivered notification rows now have a right-click context menu for marking read/unread.
- The title bar now has a `?` shortcut-help button that opens Settings directly to Shortcuts.
- Cmd/Ctrl-clicking terminal HTTP(S) links now opens them in the cmux browser surface.
- Agent waiting-input notifications now include the custom panel/terminal title when set.

### Terminal Focus After Browser Surfaces

Closed the bug where opening a browser tab/surface could leave arrow keys and other terminal shortcuts routed away from xterm after returning to the terminal.

- `Workspace` now passes `isActive` to each persistent `TerminalSurface` only when that pane is both the visible terminal surface and the focused panel.
- `TerminalSurface` refocuses xterm on active transitions, while preserving intentional focus in the terminal find bar or textbox input.
- Added Workspace regressions proving a visible focused terminal receives active focus restoration state and a focused browser pane does not activate its hidden terminal.
- Verified with `bun test src/components/Workspace.test.tsx`, `bun test src/components/TerminalSurface.test.ts`, and `bun run typecheck`.

### Notification Mark-Unread Ordering

Closed the notification bug where a delivered notification marked unread stayed buried in its previous read-list position.

- `NotificationStore::mark_unread` now moves a read notification to the front when it transitions back to unread.
- Added desktop command `notification_mark_unread` and registered it with Tauri.
- Added web host adapter `markNotificationUnread`.
- `NotificationsOverlay` now exposes a Mark unread button for read delivered notifications, while unread delivered notifications keep Mark read.
- Verified with `cargo test -p cmux-core notifications::tests:: -- --nocapture`, `cargo test -p cmux-desktop --lib notifications::tests:: -- --nocapture`, `bun test src/host/notifications.test.ts src/components/NotificationsOverlay.test.tsx`, and `bun run typecheck`.

### Sidebar Drag Cleanup

Closed the sidebar reorder bug where a dragged tab could remain dimmed with the blue drop indicator visible after drag completion/cancellation.

- `WorkspaceList` now installs active-drag cleanup listeners for window `dragend`, outside/window `drop`, window `blur`, and document `visibilitychange`.
- Drop cleanup is deferred one tick so a valid row drop can still read the dragged id before the global safety net clears state.
- Added a WorkspaceList regression for the cleanup event contract.
- Verified with `bun test src/components/WorkspaceList.test.tsx` and `bun run typecheck`.

### Terminal Loading Indicator

Closed the UI/UX TODO for showing terminal startup progress.

- `TerminalSurface` now starts with `terminalStarting=true`, clears it after `terminal_open` returns a live session id, and also clears it on startup failure so the terminal error remains visible.
- Added a `role="status"` loading badge with spinner text `Starting terminal...`.
- Added terminal CSS for the non-interactive loading pill/spinner.
- Verified with `bun test src/components/TerminalSurface.test.ts` and `bun run typecheck`.

### Browser Icon Beside Plus

Closed the UI/UX TODO for adding a browser icon to the left of the plus/new-workspace button.

- Added a shared `browser` SVG icon to `@cmux/webviews`.
- `SidebarView` now accepts `onNewBrowserWorkspace` and renders a `New browser workspace` icon button immediately before the plus button.
- The live `Sidebar` binds that button to `newBrowserWorkspace()`.
- Added Sidebar render coverage for the icon and placement.
- Verified with `bun test src/components/Sidebar.test.tsx` and `bun run typecheck`.

### Notification Focus Outlines

Closed the UI/UX TODO for notification popover buttons showing outside focus/hover outlines.

- Added `:focus-visible` outside outline rings for notification header controls, jump rows, and delivered-notification row action buttons.
- Hover/focus states now share the same highlighted background while focus adds a visible external ring.
- Added a CSS regression in `NotificationsOverlay.test.tsx`.
- Verified with `bun test src/components/NotificationsOverlay.test.tsx` and `bun run typecheck`.

### Notification Context Menu

Closed the UI/UX TODO for right-click marking delivered notifications as read or unread.

- `NotificationsOverlay` now tracks a delivered-notification context menu opened from row `onContextMenu`.
- The context menu uses the existing `markNotificationRead` and `markNotificationUnread` host actions, disables the action that would be a no-op, and closes on Escape, outside click, or action.
- Added notification context-menu CSS with fixed positioning, disabled states, and the same outside focus ring as the rest of the popover controls.
- Added regressions for the menu item state helper and context-menu CSS contract.
- Verified with `bun test src/components/NotificationsOverlay.test.tsx src/host/notifications.test.ts` and `bun run typecheck`.

### Shortcut Help Icon

Closed the UI/UX TODO for adding a question-mark shortcut discovery affordance.

- `WindowTitlebar` now accepts `onOpenShortcutHelp` and renders a compact `?` utility button beside Settings.
- `App` wires that button to `openSettings({ section: "shortcuts" })`, reusing the existing Settings deep-link behavior so users land on the editable shortcut list.
- Added focused titlebar regression coverage for the button label, title, styling class, and placement before native caption controls.
- Verified with `bun test src/components/WindowTitlebar.test.tsx` and `bun run typecheck`.

### Terminal Cmd-Click Browser Links

Closed the UI/UX TODO for opening terminal links in cmux instead of an external browser.

- `TerminalSurface` now registers an xterm link provider for HTTP(S) URLs when a browser-open callback is supplied.
- Link activation is modifier-gated: Cmd-click on macOS/WebKit-style events or Ctrl-click on Windows/Linux-style events opens the link; plain clicks remain terminal-safe.
- `Workspace` wires each terminal's link callback to `openBrowserUrl(panelId, url)`, so the existing backend opens the clicked URL in that pane's browser surface.
- `App` passes the existing `browser.openTerminalLinksInCmuxBrowser` setting into `Workspace`, defaulting to enabled.
- Added regressions for terminal URL range detection, modifier gating, Workspace callback routing, and the off-switch.
- Verified with `bun test src/components/TerminalSurface.test.ts src/components/Workspace.test.tsx` and `bun run typecheck`.

### Waiting-Input Notification Titles

Closed the UI/UX TODO for waiting-input notifications including the custom terminal/panel title.

- Added desktop command `notification_record_waiting_input`, storing one stable delivered notification per workspace/panel in the existing notification center.
- The backend notification builder trims and uses the panel title in `Waiting for input: <title>`, falls back cleanly when no custom panel title exists, and preserves activation target ids.
- Added web host adapter `recordWaitingInputNotification`.
- `Workspace` now records the waiting-input notification when an off-focus agent panel asks for attention, after the existing flash/unread behavior.
- `agentAttentionNotificationRequest` projects the current session snapshot into the backend request, preferring `panel_titles[].custom_title` and including workspace title/id.
- Verified with `cargo fmt`, `cargo test -p cmux-desktop --lib notifications::tests:: -- --nocapture`, `bun test src/session/agentAttention.test.ts src/host/notifications.test.ts src/components/Workspace.test.tsx`, and `bun run typecheck`.

Confirmed remaining unchecked TODO clusters after the latest scan:

- Remote SSH/proxy work: remove automatic `ssh -L` mirroring, add transport-scoped SOCKS5/HTTP CONNECT broker, extend `cmuxd-remote` proxy RPC, wire remote WKWebView proxy config, and add browser proxy e2e tests.
- Remote terminal sizing: tmux-style PTY resize coordinator plus multi-attachment resize tests.
- Integrations: Claude Code warm pool and install flow, Codex integration, OpenCode integration.
- Focused UI bugs: none currently unchecked in the Bug section after the latest pass.
- P0 refactors: remove index-based APIs, make CLI commands workspace-relative via `CMUX_WORKSPACE_ID`, and require explicit `close-workspace` target.
- UI/UX polish: all focused UI/UX TODO items in `TODO.md` are now checked off.

## Immediate Resume Step

Start with a small, verifiable UI/UX parity item rather than the huge remote-proxy/P0 refactor surface unless the user redirects. Recommended next slice:

- Move to the P0 refactor or remote proxy cluster next.
- Preserve the current dirty worktree; use targeted `git diff -- <file>` before editing any file already touched.
- Add or update a focused regression test, then run the smallest relevant `bun test ...` plus `bun run typecheck`.

If choosing backend-heavy work instead, begin with targeted reads for the P0 CLI workspace-relative refactor, not broad remote-proxy implementation, because it is easier to slice and verify.

## Compact Handoff - 2026-07-08

Current working directory: `C:\Users\User\coding\work\ashlr-mux\cmux`.

User goal remains active: reach feature/UI/backend parity between `ashlr-mux` and `cmux`; do not mark complete without a comprehensive audit. The user asked to continue long-running work, then asked to prepare for context compaction.

Latest completed cluster: all focused UI/UX TODO items are checked off. Verified recently with targeted `bun test`, `bun run typecheck`, `cargo fmt`, and notification-focused `cargo test` commands as recorded above.

Worktree warning: extremely dirty, with many modified and untracked files across desktop, web, crates, and tests. Do not revert or clean. Use targeted diffs before touching files already changed. `CONTINUATION.md` is untracked and should remain the local handoff ledger.

Remaining unchecked TODOs are now larger backend/P0 clusters:

- Remote SSH/proxy broker and WKWebView proxy wiring.
- Remote PTY resize coordinator and multi-attachment resize tests.
- Claude/Codex/OpenCode integration items.
- P0 API cleanup: remove index-based APIs.
- P0 CLI targeting: make CLI commands workspace-relative using `CMUX_WORKSPACE_ID`, not focused workspace.
- P0 safety: require explicit `close-workspace` target.

Recommended next slice after compaction: P0 CLI workspace-relative targeting. This is likely the smallest backend-heavy parity item with a clean verification path.

Start with narrow reads, not broad repo scans:

```powershell
rg -n "CMUX_WORKSPACE_ID|ControlCommand|send-panel|send-key-panel|new-split|new-pane|new-surface|close-surface|list-panes|list-pane-surfaces|list-panels|focus-pane|focus-panel|surface-health|close-workspace" crates/cmux-cli/src apps/desktop/src-tauri/src crates/cmux-ipc/src crates/cmux-core/src/session_ops.rs
```

Likely files to inspect first:

- `crates/cmux-cli/src/command_forward.rs`
- `crates/cmux-cli/src/dispatch.rs`
- `crates/cmux-cli/src/rpc.rs`
- `apps/desktop/src-tauri/src/control_socket.rs`
- `apps/desktop/src-tauri/src/session.rs`
- `crates/cmux-ipc/src/control_request_parser.rs`
- `crates/cmux-ipc/src/control_response_encoder.rs`

Implementation hypothesis to validate before editing:

- Add an optional workspace target derived from `std::env::var("CMUX_WORKSPACE_ID")` on CLI-forwarded workspace-scoped commands.
- Prefer the env workspace target in desktop control/session handlers.
- Fall back to selected/focused workspace only when the env var is absent, preserving compatibility.
- Add focused tests proving a background workspace is targeted when `CMUX_WORKSPACE_ID` is present, and current focused-workspace behavior remains when it is absent.

Do not start with the remote proxy cluster unless explicitly redirected; it is much wider and less likely to be completed cleanly in one compacted context window.

## CLI Workspace-Scoped Control Progress - 2026-07-08

Started the P0 `CMUX_WORKSPACE_ID` CLI targeting work.

Completed in this slice:

- Added `CMUX_WORKSPACE_ID_ENV` and `ControlCommand::with_ambient_workspace_id` in `crates/cmux-cli/src/command_forward.rs`.
- Mapped workspace/surface/browser control commands now receive `workspace_id` from `CMUX_WORKSPACE_ID` when the command is workspace-scoped and did not already specify a workspace.
- `crates/cmux-cli/src/main.rs` applies the ambient workspace id before sending mapped `RunControl` commands over the control pipe. Raw `rpc` calls remain explicit/raw.
- `apps/desktop/src-tauri/src/control_socket.rs` now lets `workspace.current` and `surface.list` honor scoped workspace params.
- Surface default resolution now chooses the focused/default surface inside `workspace_id`/`workspace_ref` when present, and falls back to the selected UI workspace when absent.
- Surface command responses now use the same scoped workspace for returned `surface.list` payloads after mutations.
- Added regressions for CLI ambient injection and backend background-workspace current/list/default-surface resolution.

Verified:

- `cargo fmt`
- `cargo test -p cmux-cli command_forward::tests:: -- --nocapture`
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture`

Important remaining work before checking off the TODO:

- The TODO's legacy affected command list still includes unmapped commands/aliases such as `send`, `send-key`, `send-panel`, `send-key-panel`, `new-pane`, `new-surface`, `list-panes`, `focus-pane`, `focus-panel`, and `surface-health`.
- Add explicit command-forward mappings and backend control methods/tests for those remaining aliases, or prove they are intentionally superseded and update the TODO wording.
- The separate P0 `close-workspace` no-args safety item is still open.

## CLI Workspace-Scoped Control Completion - 2026-07-08

Completed the rest of the P0 `CMUX_WORKSPACE_ID` CLI targeting TODO and marked it checked in `TODO.md`.

Added/finished:

- CLI mappings for the remaining affected aliases: `send`, `send-key`, `send-panel`, `send-key-panel`, `new-pane`, `new-surface`, `list-panes`, `focus-pane`, `focus-panel`, and `surface-health`.
- Concrete dispatch help text for those aliases.
- Explicit workspace-scope parsing for surface commands via `--workspace`, `--workspace-id`, and `--workspace-ref`.
- Selector normalization so `surface:N`/numeric handles become `surface_ref`, while actual `--surface surface-1` and `--panel surface-1` preserve the appropriate id key.
- Backend control methods for `surface.focus`, `surface.health`, `surface.send_text`, and `surface.send_key`.
- Live terminal input injection by panel id via `terminal_write_panel`.
- Workspace-scope-safe `surface_ports_kick_target` resolution for `workspace_id + surface index`.
- A focused P0 regression proving every affected command carries ambient `CMUX_WORKSPACE_ID`.

Verification:

- `cargo fmt`
- `cargo test -p cmux-cli -- --nocapture` (77 passed)
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture` (28 passed)

Known limitation:

- Windows `surface.send_text` / `surface.send_key` now work for live terminal sessions. The macOS cold-input queue/localized terminal-input error stack is not fully replicated yet; if strict parity requires queued sends to not-yet-mounted terminals, add a follow-up TODO or extend `TerminalState`/session startup input handling.

Next recommended P0 slice:

- `Remove close-workspace with no args — require explicit workspace short ID or UUID, with clear error message if missing`.

## Close-Workspace Explicit Target P0 - 2026-07-08

Completed and checked off the P0 `close-workspace` safety item.

Changes:

- `cmux close-workspace` and `cmux workspace close` now reject missing workspace targets with clear CLI errors.
- `cmux close-workspace --index 0` now rejects `--index`; close requires `workspace:N` or workspace id style targets.
- `workspace.close` on the desktop control socket now uses a close-specific resolver and no longer falls back to the selected workspace.
- Raw `workspace.close` rejects empty params and numeric `index` params, while still accepting `workspace_id` and `workspace_ref`.
- Help text no longer advertises closing the selected workspace.

Verification:

- `cargo fmt`
- `cargo test -p cmux-cli -- --nocapture` (78 passed)
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture` (29 passed)

Next recommended P0 slice:

- `Remove all index-based APIs in favor of short ID refs (surface:N, pane:N, workspace:N, window:N)`.

## Desktop Icon Packaging Polish - 2026-07-08

Completed a small desktop identity polish slice:

- Verified the Tauri bundle already points at `apps/desktop/src-tauri/icons/icon.ico` and `Assets.xcassets/AppIcon.appiconset/512.png`.
- Regenerated `apps/desktop/src-tauri/icons/icon.ico` from the existing cmux app artwork as a multi-resolution Windows ICO.
- The ICO now embeds `16x16`, `24x24`, `32x32`, `48x48`, `64x64`, `128x128`, and `256x256` frames instead of only `256x256`, which should make Desktop/Explorer/taskbar/installer contexts render more crisply.

Verification:

- Opened `Assets.xcassets/AppIcon.appiconset/512.png` visually.
- Verified ICO contents with Pillow: `[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]`.

## P0 Index API Cleanup Input Slice - 2026-07-09

Made concrete progress on the remaining P0 `Remove all index-based APIs in favor of short ID refs (surface:N, pane:N, workspace:N, window:N)` item, but did not check it off yet.

Completed in this slice:

- CLI workspace selectors now reject `--index` with a clear error and require `workspace:N` refs or workspace ids.
- CLI surface selectors now reject `--index` with a clear error and require `surface:N` refs or surface ids.
- Bare numeric workspace positionals still normalize to `workspace:N` refs, preserving a human migration path without sending raw `index` params.
- Surface-only commands now accept positional `surface:N` refs, e.g. `cmux close-surface surface:1`.
- Desktop control-socket workspace/surface resolver helpers no longer accept raw `"index"` selector params.
- Raw surface resolver calls with `"index"` now fail closed instead of silently falling back to the focused surface.
- Bulk workspace close now ignores legacy `indices`/`indexes`; refs/ids remain supported.
- Help text no longer advertises workspace/surface index selectors.

Verification:

- `cargo fmt`
- `cargo test -p cmux-cli -- --nocapture` (79 passed)
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture` (29 passed)

Remaining before checking off the P0:

- Control-socket response payloads still include legacy `index` fields for workspace/surface list shapes.
- Python v1/v2 e2e helpers still consume/emit numeric `index` selectors in several places.
- Need a broader response-contract/test-harness migration to refs-first/no-index output before the P0 can honestly be marked complete.

## P0 Index API Cleanup Output Slice - 2026-07-09

Continued the same remaining P0 without checking it off.

Completed in this slice:

- Removed legacy `index` fields from control-socket workspace list rows.
- Removed legacy `index` fields from control-socket workspace group member rows.
- Removed legacy `index` and `index_in_pane` fields from control-socket surface list rows. Because `surface.health` reuses those rows, health output now also moves toward `surface:N` refs rather than numeric surface indexes.
- Added Rust assertions proving workspace/surface list rows no longer expose those index fields and still expose `workspace:N` / `surface:N` refs.
- Updated `tests_v2/cmux.py` helper resolution/list convenience APIs to derive temporary zero-based ordinals from `workspace:N`, `surface:N`, and `pane:N` refs instead of reading `index` fields from list responses.

Verification:

- `cargo fmt`
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture` (29 passed)
- `python -m py_compile tests_v2\cmux.py`

Remaining before checking off the P0:

- `workspace.reorder` still uses `to_index` / `target_index` style destination params in CLI and control socket.
- `tests_v2/cmux.py` still emits `params["index"]` for reorder/move convenience paths.
- Older v1-style tests/helpers still contain many numeric `index` expectations and should either be migrated to refs or proven out of scope for the v2/control API.
- Some browser locator APIs legitimately use an `"index"` property for DOM nth-selection; do not conflate those with workspace/surface/pane/window handle APIs.

## P0 Index API Cleanup Reorder Slice - 2026-07-09

Continued the remaining P0 without checking it off.

Completed in this slice:

- CLI `workspace reorder` / `workspace move` now emit `before_workspace_ref` / `after_workspace_ref` or corresponding ids instead of `to_index`.
- CLI `--to-index` / `--target-index` are explicitly rejected with a clear migration error instead of being accepted as index-shaped API.
- Desktop control socket `workspace.reorder` now resolves `before_workspace_ref` / `after_workspace_ref` and id variants to the existing internal reorder insertion index.
- Raw control socket `to_index` / `target_index` params are rejected for `workspace.reorder`.
- Removed the now-unused generic `i64_param` helper from the control socket.
- Updated `tests_v2/cmux.py` so reorder/move convenience methods no longer emit raw `params["index"]`; numeric convenience targets now become `before_workspace_ref` / `before_surface_ref`.
- Migrated v2 tests that read `surface.health()[*]["index"]` to use `ref` / `surface_ref` / `id` handles instead:
  - `tests_v2/test_focus_notification_dismiss.py`
  - `tests_v2/test_tab_dragging.py`
  - `tests_v2/test_visual_screenshots.py`

Verification:

- `cargo fmt`
- `cargo test -p cmux-cli -- --nocapture` (79 passed)
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture` (30 passed)
- `python -m py_compile tests_v2\cmux.py tests_v2\test_focus_notification_dismiss.py tests_v2\test_tab_dragging.py tests_v2\test_visual_screenshots.py`

Remaining before checking off the P0:

- Older v1-style tests and helper code still contain numeric `index` payload expectations.
- `--to-index` / `--target-index` strings remain only as rejected CLI aliases/tests.
- Browser DOM nth-selector APIs still legitimately use `"index"` and are unrelated to workspace/surface/pane/window handle refs.

## P0 Index API Cleanup Legacy Test Slice - 2026-07-09

Continued the same P0 without checking it off.

Completed in this slice:

- Legacy `tests/cmux.py` `surface_health()` now derives `ref` / `surface_ref` handles and no longer exposes an `index` field.
- Migrated old v1-style tests that consumed `surface_health()[*]["index"]` to use `surface:N` refs or ids:
  - `tests/test_focus_notification_dismiss.py`
  - `tests/test_tab_dragging.py`
  - `tests/test_visual_screenshots.py`
  - `tests/test_multi_workspace_focus.py`
- Removed stale numeric `index` fields from fake socket list-response fixtures:
  - `tests/test_claude_hook_clear_running_status.py`
  - `tests/test_cli_claude_teams_main_vertical.py`
  - `tests/test_cli_claude_teams_tmux_sequence.py`
  - `tests/test_cli_omx_hud_tmux_split.py`
  - `tests/test_cli_omo_tmux_respawn_pane.py`
  - `tests/test_cli_tmux_compat_split_window_surface_ref.py`
  - `tests/test_cli_layout_focus_contract.py` window fixture
- Replaced one diagnostic pane-state `"index"` field with a derived `surface:N` ref in `tests/test_cmd_option_t_close_other_tabs_in_pane.py`.

Verification:

- `python -m py_compile tests\cmux.py tests\test_focus_notification_dismiss.py tests\test_tab_dragging.py tests\test_visual_screenshots.py tests\test_multi_workspace_focus.py tests\test_cli_claude_teams_main_vertical.py tests\test_cli_claude_teams_tmux_sequence.py tests\test_cli_omx_hud_tmux_split.py tests\test_cli_omo_tmux_respawn_pane.py tests\test_cli_tmux_compat_split_window_surface_ref.py tests\test_cmd_option_t_close_other_tabs_in_pane.py tests\test_claude_hook_clear_running_status.py tests\test_cli_layout_focus_contract.py`

Remaining before checking off the P0:

- `tests/test_cli_layout_focus_contract.py` still expects `reorder-surface --index` to send `{"index": 0}`. This should be handled by a real `reorder-surface` CLI/backend contract slice, not by fixture-only editing.
- `tests_v2/test_browser_api_extended_families.py` still uses `"index"` for browser DOM nth-selection; this is legitimate and unrelated to workspace/surface/pane/window handle refs.

## P0 Index API Cleanup Completion - 2026-07-09

Completed and checked off the P0 `Remove all index-based APIs in favor of short ID refs (surface:N, pane:N, workspace:N, window:N)`.

Final cleanup:

- Updated `tests/test_cli_layout_focus_contract.py` so `reorder-surface --index` is expected to fail as not ported instead of expecting `surface.reorder` with `{"index": 0}`.
- Updated the `tests_v2/cmux.py` header comment so numeric helper conveniences are described as resolving through short refs, not index APIs.
- Final Python API scan showed only `tests_v2/test_browser_api_extended_families.py` using `"index"` for browser DOM nth-selection, which is unrelated to workspace/surface/pane/window handles and should remain.
- Remaining Rust `index` hits are internal resolver variable names, explicit rejection tests for `"index"` / `--index` / `to_index`, or local loop counters used to derive refs.
- Marked the TODO item `[x]` in `TODO.md`.

Verification:

- `cargo test -p cmux-cli -- --nocapture` (79 passed)
- `cargo test -p cmux-desktop --lib control_socket::tests:: -- --nocapture` (30 passed)
- `python -m py_compile tests\test_cli_layout_focus_contract.py`
- `python -m py_compile` over the legacy test/helper migration set from the previous slice.

## Remote Proxy/Resize Checklist Reconciliation - 2026-07-09

Reconciled stale unchecked Issue 151 TODO rows against current implementation evidence and marked them complete in `TODO.md`.

Evidence:

- `docs/remote-daemon-spec.md` marks port-mirroring removal, transport-scoped SOCKS5/CONNECT broker, daemon proxy stream RPC, WKWebView proxy wiring, browser proxy e2e coverage, PTY resize coordinator, and resize tests as `DONE`.
- `daemon/remote/cmd/cmuxd-remote/main.go` advertises and dispatches `proxy.open`, `proxy.close`, `proxy.write`, `proxy.stream.subscribe`, `session.attach`, `session.resize`, `session.detach`, and `pty.resize`.
- `daemon/remote/cmd/cmuxd-remote/main_test.go` and `ws_rpc_test.go` cover proxy stream RPC data/eof flow and invalid parameter cases.
- `daemon/remote/cmd/cmuxd-remote/main_test.go`, `ws_pty.go`, and `ws_pty_test.go` cover smallest-screen-wins resize state and attachment transitions.
- `tests_v2/test_ssh_remote_docker_forwarding.py`, `test_ssh_remote_docker_reconnect.py`, `test_ssh_remote_proxy_bind_conflict.py`, and `test_ssh_remote_browser_move_rebinds_proxy.py` cover SOCKS/CONNECT HTTP + WebSocket egress, reconnect continuity, bind-conflict `proxy_unavailable`, and absence of explicit forwarded ports.
- Search for automatic mirroring found no active `ssh -L` / `LocalForward` implementation; only docs and regression assertions remain.

Verification:

- `go test ./cmd/cmuxd-remote` from `daemon/remote` passed.

## Claude Integration Palette Affordance - 2026-07-09

Made a small UI parity improvement for the unchecked Claude Code integration menu work, but did not check off the native menubar TODO.

Completed:

- Updated the settings-toggle command palette contribution for `automation.claudeCodeIntegration` so the disabled-state command reads `Install Claude Code Integration` instead of the generic `Enable Claude Code Integration`.
- Added search keywords for `install`, `setup`, `hooks`, `hook`, `claude-code`, and `agent` so users can find the integration setup affordance by the words they are likely to type.
- Kept the existing toggle-setting intent path, so activation still writes the `automation.claudeCodeIntegration` config flag through the established config reducer/mutation pipeline.
- Added regression coverage in `apps/desktop/web/src/palette/settingsToggleContributions.test.ts`.

Verification:

- `bun test apps/desktop/web/src/palette/settingsToggleContributions.test.ts` passed.

Still open at that point (later slices below completed the native menu item and Codex/OpenCode rows):

- `TODO.md` still leaves `Add "Install Claude Code integration" menu item in menubar` unchecked because this slice improves command-palette/settings discoverability, not a true native menubar item.
- Warm-pool keyboard shortcut and Codex/OpenCode integration rows remain unchecked.

## Codex/OpenCode Integration Audit - 2026-07-09

Audited the unchecked `Codex integration` / `OpenCode integration` rows after the Claude affordance slice.

Findings:

- Backend/runtime provider support exists:
  - `crates/cmux-agent/src/lib.rs` defines `AgentSessionProviderId::{Codex, Claude, OpenCode}` and launch arguments for Codex `app-server --listen stdio://` and OpenCode `serve --hostname 127.0.0.1 --port 0 --print-logs`.
  - `apps/desktop/src-tauri/src/agent_session.rs` maps Codex/OpenCode providers into the agent-session bridge and has OpenCode HTTP-loopback handling.
  - `crates/cmux-agent-chat` has Codex and OpenCode transcript/stream parsing and process-store coverage.
  - `apps/desktop/src-tauri/src/lib.rs` desktop status exposes `agent_providers = ["codex", "claude", "opencode"]`.
- Settings/config parity is incomplete:
  - `crates/cmux-config/src/lib.rs::AutomationConfig`, generated `AutomationConfig.ts`, `defaultConfig.ts`, `SettingsPane.tsx`, `settingsToggleContributions.ts`, and settings search only expose integration booleans for Claude/Amp/Cursor/Gemini/Kiro.
  - There are no `codexIntegration` / `opencodeIntegration` config fields or settings toggle rows today.
- Existing integration booleans appear to be UI/config affordances in this Windows port; search found no backend consumers for `claudeCodeIntegration`, `ampIntegration`, `cursorIntegration`, `geminiIntegration`, or `kiroIntegration` beyond settings/search/default config.

Recommended next slice:

- Add `codexIntegration` and `opencodeIntegration` to `AutomationConfig` with default `true`, regenerate/update core TS bindings, surface them in `defaultConfig.ts`, `SettingsPane.tsx`, `settingsToggleContributions.ts`, and `settingsSearch.ts`, then run config/settings/palette tests.

## Pre-Compact Note - Codex/OpenCode Settings Slice - 2026-07-09

Paused before editing the Codex/OpenCode config/settings slice because context compaction is imminent.

Current state:

- No Codex/OpenCode config changes have been made yet in this slice.
- Confirmed `AutomationConfig` currently has:
  - `claudeCodeIntegration`
  - `ampIntegration`
  - `cursorIntegration`
  - `geminiIntegration`
  - `kiroIntegration`
  - but no `codexIntegration` / `opencodeIntegration`.
- Confirmed generated `apps/desktop/packages/core-types/src/generated/AutomationConfig.ts`, `apps/desktop/web/src/settings/defaultConfig.ts`, `apps/desktop/web/src/components/SettingsPane.tsx`, `apps/desktop/web/src/palette/settingsToggleContributions.ts`, and `apps/desktop/web/src/settings/settingsSearch.ts` mirror that missing surface.
- Existing integration booleans appear UI/config-only in the Windows port; `rg` found no backend consumers beyond settings/search/default config, so adding Codex/OpenCode toggles should not require backend behavior changes.

Exact next edit list:

- `crates/cmux-config/src/lib.rs`
  - add `codex_integration: bool` with serde rename `codexIntegration`
  - add `opencode_integration: bool` with serde rename `opencodeIntegration`
  - default both to `true`
  - update `key_casing_serializes_exactly` assertions to include both keys
- `apps/desktop/packages/core-types/src/generated/AutomationConfig.ts`
  - add `codexIntegration: boolean` and `opencodeIntegration: boolean`
  - this file is generated, but manual patch is acceptable if not running the generator
- `apps/desktop/web/src/settings/defaultConfig.ts`
  - default both to `true`
- `apps/desktop/web/src/components/SettingsPane.tsx`
  - add `Codex integration` and `OpenCode integration` rows to `AUTOMATION_FLAGS`
- `apps/desktop/web/src/palette/settingsToggleContributions.ts`
  - add `Codex Integration` and `OpenCode Integration` to `AUTOMATION_TOGGLES`
  - probably use disabled titles `Install Codex Integration` / `Install OpenCode Integration` and keywords `install`, `setup`, `hooks`, `agent`
- `apps/desktop/web/src/settings/settingsSearch.ts`
  - add alias text and entries for `automation:codex` / `automation:opencode`
- Tests likely needing updates:
  - `apps/desktop/web/src/components/SettingsPane.test.tsx`
  - `apps/desktop/web/src/palette/settingsToggleContributions.test.ts`
  - `apps/desktop/web/src/settings/configReducer.test.ts`
  - `apps/desktop/web/src/settings/configMutation.test.ts`
  - maybe `apps/desktop/web/src/settings/settingsSearch*.test.ts`

Suggested verification:

- `cargo test -p cmux-config`
- `bun test apps/desktop/web/src/palette/settingsToggleContributions.test.ts`
- `bun test apps/desktop/web/src/components/SettingsPane.test.tsx`
- `bun test apps/desktop/web/src/settings/configReducer.test.ts apps/desktop/web/src/settings/configMutation.test.ts apps/desktop/web/src/settings/settingsSearch.test.ts apps/desktop/web/src/settings/settingsSearchResults.test.ts`

## Codex/OpenCode Integration Settings Completion - 2026-07-09

Completed the Codex/OpenCode integration checklist slice and marked both rows complete in `TODO.md`.

What changed:

- Added `codexIntegration` and `opencodeIntegration` to `AutomationConfig` with defaults of `true` and exact camelCase serialization.
- Mirrored the fields into the generated TypeScript automation config shape and frontend default config.
- Surfaced both integrations in Settings, command-palette settings toggles, and settings search.
- Used disabled-state palette titles `Install Codex Integration` and `Install OpenCode Integration`, matching the Claude install affordance pattern.

Backend/parity evidence:

- `cmux-agent` already supports `AgentSessionProviderId::Codex` and `AgentSessionProviderId::OpenCode`.
- `cmux-agent-chat` has Codex/OpenCode stream and transcript parsing coverage.
- `cmux-desktop` agent-session bridge tests cover provider/session behavior.

Verification:

- `bun test apps/desktop/web/src/palette/settingsToggleContributions.test.ts apps/desktop/web/src/components/SettingsPane.test.tsx apps/desktop/web/src/settings/configReducer.test.ts apps/desktop/web/src/settings/configMutation.test.ts apps/desktop/web/src/settings/settingsSearch.test.ts apps/desktop/web/src/settings/settingsSearchResults.test.ts` passed with 206 tests.
- `cargo test -p cmux-config` passed with 84 tests.
- `cargo test -p cmux-agent -p cmux-agent-chat` passed.
- `cargo test -p cmux-desktop --lib agent_session::tests:: -- --nocapture` passed with 23 tests.

Still open after this slice:

- Warm pool of Claude Code instances mapped to a keyboard shortcut.
- Per-WKWebView proxy observability/inspection.

## Claude Code Integration Menubar/CLI Install Flow - 2026-07-09

Completed the native menu/CLI install-flow checklist row and marked it complete in `TODO.md`.

What changed:

- Added a local `cmux hooks claude install` CLI path in `crates/cmux-cli/src/hooks_installer.rs`.
- Supported documented Claude setup spellings: `cmux hooks claude install`, `cmux hooks setup --agent claude`, `cmux hooks setup claude`, and `cmux setup-hooks claude`.
- The installer reads the user's `cmux.json`, previews a unified diff that sets `automation.claudeCodeIntegration` to `true`, prompts `Type y to apply this change:`, validates the updated config through `cmux-config`, and writes only after confirmation.
- Added a Tauri native menu item named `Install Claude Code Integration...` under an `Integrations` menu.
- The menu handler opens a fresh cmux terminal workspace running the bundled CLI command `cmux hooks claude install`, so the user sees the diff and can confirm interactively inside cmux.

Verification:

- `cargo test -p cmux-cli -- --nocapture` passed with 84 tests.
- `cargo test -p cmux-desktop --lib cli::tests -- --nocapture` passed.
- `cargo test -p cmux-desktop --lib claude_installer_menu_command_uses_quoted_cli_path -- --nocapture` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed earlier after the native menu API wiring.

Notes:

- This slice intentionally completes the Claude Code integration install affordance, not the full cross-agent `cmux hooks setup` installer matrix. Unsupported hook installers still return explicit Windows-port messages.
- Remaining open TODO rows: Claude Code warm pool keyboard shortcut, and per-WKWebView proxy observability/inspection.

## Browser Network Requests Inspection Bridge - 2026-07-09

Advanced the remaining per-WKWebView proxy observability row, but did not mark it complete.

What changed:

- Added a per-browser-surface network observation buffer to `apps/desktop/src-tauri/src/browser.rs`.
- Records best-effort WKWebView/Tauri navigation observations with URL, method (`GET`), source, transport, start/completion timestamps, and bounded per-panel retention.
- Exposed `browser.network.requests` in `apps/desktop/src-tauri/src/control_socket.rs`, guarded so it only succeeds for browser surfaces.
- Kept `browser.network.route` and `browser.network.unroute` explicit `not_supported` responses because WKWebView interception/mocking is not implemented.
- Updated `tests_v2/test_browser_api_unsupported_matrix.py` so `browser.network.requests` is now expected to succeed and return the inspection payload shape.

Current limitation:

- This is not yet full proxy-level observability. Request headers, request body, response status, and response headers are returned as empty/null until the actual proxy broker stream path is ported/instrumented in the Windows/Tauri backend. The response includes an observer summary that states this limitation.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed.
- Attempted `cargo test -p cmux-desktop --lib browser::tests -- --nocapture`; the test binary compiled, then failed to launch with Windows `STATUS_ENTRYPOINT_NOT_FOUND`, likely a local runtime/DLL entrypoint issue. Use `--no-run` as the current compile verification in this environment unless that DLL issue is resolved.

Recommended next work:

- For true completion of the proxy observability TODO, find/port the local proxy broker from the legacy Swift side or add the equivalent Rust/Tauri broker layer, then emit full records into `BrowserWebviewState` with URL, method, request headers/body metadata, response status/headers, bytes, and timings.
- If proxy broker porting is too large for the next slice, switch to the other remaining TODO: Claude Code warm pool + keyboard shortcut.

## Claude Code Warm Pool Backend + Palette Front Door - 2026-07-09

Advanced the remaining warm-pool TODO, but did not mark it complete because the explicit keyboard shortcut binding is still not wired.

Backend completed in this slice:

- `crates/cmux-agent-chat/src/process_store.rs` now tracks unadopted warm Claude sessions separately from running UI sessions.
- Added `warm_claude_session`, `clear_warm_sessions`, warm-session status accessors, warm eviction, stale-exit cleanup, and start-time adoption by matching provider + working directory.
- Warm preparation does not emit `provider.started`; adoption through normal Claude start emits the existing started event with the warmed session id.
- `close_all` now terminates both active and warm sessions.
- `apps/desktop/src-tauri/src/agent_session.rs` exposes:
  - `provider.warmClaude`
  - `provider.warmPool.prepareClaude`
  - `provider.warmPool.status`
  - `provider.warmPool.clear`

Frontend completed in this slice:

- Added command-palette row `palette.warmClaudeCode` with title `Warm Claude Code`.
- The row is workspace-scoped and searchable by `agent`, `claude`, `code`, `warm`, `pool`, `prewarm`, `shortcut`, and `start`.
- `intentPlan` now maps it to `{ type: "warmClaudeCode", currentDirectory }` using the selected workspace cwd.
- `useCommandPalette` executes it through `host.invoke("agent_session_rpc", { message: { method: "provider.warmClaude", params } })`.

Verification:

- `cargo fmt -p cmux-agent-chat`
- `cargo test -p cmux-agent-chat process_store::tests::warm_claude -- --nocapture`
- `cargo test -p cmux-agent-chat process_store::tests::start_adopts_matching_warm_claude_session_without_second_spawn -- --nocapture`
- `cargo test -p cmux-agent-chat -- --nocapture` passed with 267 unit tests plus replay/doc tests.
- `cargo fmt -p cmux-desktop -p cmux-agent-chat`
- `cargo test -p cmux-desktop --lib agent_session::tests:: --no-run`
- `bun test apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed with 77 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.

Historical limitation before the shortcut completion below:

- At this point the TODO row `Warm pool of Claude Code instances mapped to a keyboard shortcut` was intentionally left unchecked because only the backend and palette front door existed.
- The following `Claude Code Warm Pool Shortcut Completion` section supersedes this limitation and records the actual shortcut/action routing that completed the row.

## Claude Code Warm Pool Shortcut Completion - 2026-07-09

Completed and checked off the warm-pool TODO row.

What changed after the palette/backend slice:

- Added default shortcut binding `agent.warmClaudeCode = "ctrl+alt+c"` in both:
  - `crates/cmux-config/src/lib.rs::ShortcutsConfig::default()`
  - `apps/desktop/web/src/settings/defaultConfig.ts::DEFAULT_SHORTCUTS_CONFIG`
- Added `apps/desktop/web/src/settings/shortcutRuntime.ts`, a runtime matcher for single-stroke configured shortcuts.
- `useCommandPalette` now listens for configured shortcuts while the palette is hidden, ignores editable text targets, maps `agent.warmClaudeCode` to the selected-workspace warm-Claude plan, and invokes the existing `provider.warmClaude` RPC with the selected workspace cwd.
- The command-palette row `Warm Claude Code` now displays shortcut hint `⌃⌥C`.
- Added regression coverage in:
  - `apps/desktop/web/src/settings/shortcutRuntime.test.ts`
  - `apps/desktop/web/src/palette/commandCatalog.test.ts`
  - `crates/cmux-config/src/lib.rs::defaults_match_schema`

Verification:

- `cargo fmt -p cmux-config -p cmux-agent-chat -p cmux-desktop`
- `bun test apps/desktop/web/src/settings/shortcutRuntime.test.ts apps/desktop/web/src/settings/shortcutBinding.test.ts apps/desktop/web/src/components/SettingsPane.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed with 203 tests.
- `cargo test -p cmux-config defaults_match_schema -- --nocapture` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `cargo test -p cmux-agent-chat process_store::tests::warm_claude -- --nocapture` passed.
- `cargo test -p cmux-agent-chat process_store::tests::start_adopts_matching_warm_claude_session_without_second_spawn -- --nocapture` passed.
- `cargo test -p cmux-desktop --lib agent_session::tests:: --no-run` passed.

Remaining unchecked TODO after this completion:

- `Per-WKWebView proxy observability/inspection once remote proxy path is shipped (URL, method, headers, body, status, timing)`

## Browser Network Requests Filter/Metadata Slice - 2026-07-09

Advanced the remaining browser proxy observability row, but did not mark it complete.

What changed:

- `browser.network.requests` now returns `totalCount`, `filteredCount`, and `returnedCount` alongside `requests`.
- Observer metadata now reports `supportsFilters` and `maxRecordsPerPanel`.
- Added query support for:
  - `urlContains` / `url_contains` / `url`
  - `method`
  - `sinceId` / `since_id` / `afterId` / `after_id`
  - `limit`
- The request list remains chronological after filtering; `limit` returns the most recent matching records while preserving order.
- Updated the v2 unsupported/browser matrix to assert the richer response and a filtered `limit: 1` call.

Verification:

- `cargo fmt -p cmux-desktop`
- `cargo test -p cmux-desktop --lib --no-run`
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py`

Still incomplete:

- This does not yet capture request headers, request bodies, response status, or response headers.
- Current records are still `wkwebview-navigation` best-effort observations. Full TODO completion still needs proxy-level instrumentation in the actual browser/proxy transport path, then records should be populated with URL, method, headers/body metadata, status, headers, and timings from that transport.

## Pre-Compact Resume Point - Browser Proxy Observability - 2026-07-09

Use this as the next restart point after compaction.

Current project state:

- `TODO.md` has exactly one unchecked item: `Per-WKWebView proxy observability/inspection once remote proxy path is shipped (URL, method, headers, body, status, timing)`.
- The warm Claude Code backend, palette action, and default `ctrl+alt+c` shortcut are complete and verified.
- `browser.network.requests` now exists and supports filters/metadata, but it is still navigation-observation level rather than true proxy/request-response inspection.
- The active goal must remain open; do not mark it complete until the remaining browser observability row is genuinely finished or a broader parity audit proves no work remains.

Most recent investigation before compaction:

- There is no obvious Rust desktop local proxy broker implementation in `apps/desktop/src-tauri/src`; the current browser layer uses Tauri `WebviewBuilder` plus navigation observation.
- `docs/remote-daemon-spec.md` says the remote proxy path is done on the daemon side, but desktop-side browser/proxy inspection is not yet wired into `BrowserWebviewState`.
- Tauri 2.11.3 exposes `WebviewBuilder::on_web_resource_request` in `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\tauri-2.11.3\src\webview\mod.rs`.
- Wry 0.55.1's Windows/WebView2 implementation has `WebResourceRequested` handling in `C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\wry-0.55.1\src\webview2\mod.rs`.
- Need inspect whether `on_web_resource_request` exposes final network response status/headers for normal navigation/subresource requests or only request/interception metadata. Do not assume it gives full response data.
- Existing custom URI schemes in `apps/desktop/src-tauri/src/lib.rs` (`cmux-diff-viewer`, `cmux-md`, `cmux-local-image`, `cmux-remote-image`) can be instrumented for real scheme-level request/response records if full proxy instrumentation is too large for the next slice.

Recommended first commands after compaction:

```powershell
Get-Content -Path C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\tauri-2.11.3\src\webview\mod.rs | Select-Object -Skip 455 -First 50
Get-Content -Path C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\wry-0.55.1\src\webview2\mod.rs | Select-Object -Skip 936 -First 120
```

Decision tree:

- If `on_web_resource_request` can observe true final response status/headers for normal WK/WebView traffic, wire it into `apps/desktop/src-tauri/src/browser.rs` and populate `BrowserNetworkRecord` with method, URL, headers/body metadata, status, response headers, and duration.
- If it only observes/intercepts request metadata, keep the global observer summary honest and either record request-only entries from it or instrument the custom scheme handlers as a real partial record source.
- If full proxy parity requires porting/building a local broker, leave the TODO unchecked, document the exact gap, and implement the smallest safe broker/instrumentation slice rather than overclaiming completion.

Verification baseline for the current state:

- `cargo fmt -p cmux-config -p cmux-agent-chat -p cmux-desktop`
- `bun run --cwd apps/desktop/web typecheck`
- `cargo test -p cmux-desktop --lib --no-run`
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py`

Known local limitation:

- Runnable `cmux-desktop` browser test binaries previously compiled but failed to launch on this machine with Windows `STATUS_ENTRYPOINT_NOT_FOUND`; prefer `--no-run` unless that DLL/runtime issue is fixed.

## Browser Custom-Scheme Network Record Slice - 2026-07-09

Advanced the remaining browser proxy observability row, but did not mark it complete.

What changed:

- `BrowserNetworkRecord` now includes:
  - `requestBodySize`
  - `requestBodyTruncated`
  - `responseBodySize`
- Added `record_custom_scheme_network_request` in `apps/desktop/src-tauri/src/browser.rs`.
- Added browser-webview label parsing so requests from child labels like `browser:<panel_id>` map back to the owning surface.
- Instrumented registered cmux custom URI schemes in `apps/desktop/src-tauri/src/lib.rs`:
  - `cmux-diff-viewer`
  - `cmux-md`
  - `cmux-local-image`
  - `cmux-remote-image`
- When those schemes are requested by a browser child webview, `browser.network.requests` now records real request headers, bounded request body text/size/truncation, response status, response headers, response body size, and duration.
- Observer metadata now reports richer capture capabilities when custom-scheme records are present, while preserving the honest WKWebView-navigation limitation for ordinary external traffic.
- `tests_v2/test_browser_api_unsupported_matrix.py` now asserts the richer per-record shape when records are returned.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py` passed.

Known verification limitation:

- Attempted `cargo test -p cmux-desktop --lib browser::tests:: -- --nocapture`; the test binary compiled but still failed to launch with Windows `STATUS_ENTRYPOINT_NOT_FOUND`, matching the previously documented local runtime/DLL issue.

Still incomplete:

- The remaining TODO row is still unchecked.
- This slice does not capture final response status/headers for ordinary external HTTP(S) navigations or subresources.
- True completion still needs the desktop browser/proxy transport path to emit records for proxied WebView traffic with URL, method, request headers/body metadata, response status/headers, and timing.

## Browser Proxy HTTP Parser Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `record_proxy_http_exchange_observation` in `apps/desktop/src-tauri/src/browser.rs`.
- Added a bounded cleartext proxy HTTP parser that:
  - parses request line, method, absolute-form or origin-form URL, `Host`, headers, and body prefix;
  - parses response status, response headers, and response body prefix;
  - emits normal `BrowserNetworkRecord` entries with source `proxy-stream-http`;
  - records tunnel transport labels such as `socks5` or `http-connect`;
  - keeps TLS payloads explicitly out of scope because they are opaque without interception.
- Updated observer summary detection so rich `proxy-stream-http` records advertise request headers/body metadata, response status/headers, body sizes, and timing just like custom-scheme records.
- Added compile-checked Rust tests for:
  - cleartext request/response metadata recording,
  - absolute-form URLs and non-default port URL derivation,
  - malformed/incomplete HTTP head rejection.

Why this matters:

- The Swift parity source (`Packages/macOS/CmuxRemoteWorkspace/.../RemoteDaemonProxySession.swift`) parses SOCKS5 and HTTP CONNECT handshakes, then forwards tunnel bytes. This new Rust parser is the missing observability component needed at that broker stream boundary once the Windows/Tauri broker path is ported or exposed.
- It still is not wired to a live Windows broker because `apps/desktop/src-tauri/src` does not currently contain the remote proxy broker/tunnel implementation despite the daemon-side docs/tests saying the remote proxy path is done.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py` passed.

Known verification limitation:

- Attempted `cargo test -p cmux-desktop --lib browser::tests::proxy_http -- --nocapture`; the test binary compiled but failed to launch with Windows `STATUS_ENTRYPOINT_NOT_FOUND`, matching the existing local runtime/DLL issue.

Still incomplete:

- The TODO row remains unchecked.
- To finish it for real, the Windows/Tauri desktop needs a live remote proxy broker/tunnel equivalent to the Swift `RemoteDaemonProxyTunnel`/`RemoteDaemonProxySession`, WebView proxy wiring via Wry/WebView2 proxy config or an equivalent supported Tauri path, and calls into `record_proxy_http_exchange_observation` or a richer TLS-safe tunnel observation at the stream boundary.

## Browser WebView Proxy URL Wiring Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- `browser_attach_webview` and `browser_update_webview` now accept optional `proxyUrl` / `proxy_url`.
- Proxy URLs are validated as explicit-port `http://host:port` or `socks5://host:port` endpoints.
- New browser child webviews apply the proxy via Tauri `WebviewBuilder::proxy_url`.
- `BrowserChild` stores the applied proxy URL and `BrowserWebviewReply` returns:
  - `proxyUrl`
  - `proxyApplied`
- Existing webviews reject proxy URL changes with a clear error because proxy config is creation-time WebView2/Wry state; callers must recreate the child webview to switch brokers.
- `BrowserSurface` accepts optional `proxyUrl` and forwards it to native `browser_attach_webview` / `browser_update_webview`.

Why this matters:

- Wry 0.55.1 documents Windows/Linux support for HTTP CONNECT and SOCKS5 proxy configuration, and its WebView2 backend converts the config into `--proxy-server=http://...` or `--proxy-server=socks5://...`.
- This gives the Windows/Tauri browser layer a concrete app-level seam for remote workspace proxy endpoints. The remaining missing piece is surfacing the live remote proxy endpoint from session/workspace state into `BrowserSurface.proxyUrl` and feeding stream bytes into the proxy HTTP observer.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --no-run` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py` passed.

Known verification limitation:

- Attempted `cargo test -p cmux-desktop --lib browser::tests::normalize_proxy_url -- --nocapture`; the test binary compiled but failed to launch with Windows `STATUS_ENTRYPOINT_NOT_FOUND`, matching the existing local runtime/DLL issue.

Still incomplete:

- The TODO row remains unchecked.
- Need session/workspace remote state to publish a live proxy endpoint into browser surfaces on Windows/Tauri.
- Need a Windows/Tauri remote proxy broker/tunnel implementation or bridge equivalent to the Swift `RemoteDaemonProxyTunnel`/`RemoteDaemonProxySession`.
- Need live broker stream observation calls into `record_proxy_http_exchange_observation` for cleartext HTTP traffic, with honest opaque handling for TLS-over-CONNECT streams.

## Browser Session Proxy URL Snapshot Plumbing Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added optional `browser_proxy_url` to `SessionPaneLayoutSnapshot` in `crates/cmux-core/src/session.rs`.
- Added the matching generated TypeScript field in `apps/desktop/packages/core-types/src/generated/SessionPaneLayoutSnapshot.ts`.
- Updated pane snapshot initializers in core and desktop test fixtures to include `browser_proxy_url`.
- Updated `Workspace` so persisted pane layout state maps `browser_proxy_url` into `BrowserSurface.proxyUrl`.
- Updated the control-socket `surface.list` browser payload to include `browser_proxy_url`.
- Strengthened the browser surface-list test to prove a SOCKS5 proxy endpoint is surfaced through IPC.

Why this matters:

- The earlier WebView proxy wiring gave `BrowserSurface` and the Tauri browser backend a creation-time `proxyUrl` seam.
- This slice connects session/workspace snapshot state to that seam, so a future remote broker can publish `http://host:port` or `socks5://host:port` into pane state and have the UI create the WebView with matching proxy routing.

Verification:

- `cargo fmt -p cmux-core -p cmux-desktop` passed.
- `cargo test -p cmux-core -p cmux-desktop --lib --no-run` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.

Still incomplete:

- The TODO row remains unchecked.
- There is still no live Windows/Tauri remote proxy endpoint publication into `browser_proxy_url`.
- There is still no Windows/Tauri remote proxy broker/tunnel implementation or bridge equivalent to the Swift `RemoteDaemonProxyTunnel`/`RemoteDaemonProxySession`.
- There are still no live broker stream observation calls into `record_proxy_http_exchange_observation`.

## Windows Remote Proxy Control-State Publication Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added structured remote workspace metadata to `SessionWorkspaceSnapshot`:
  - `SessionWorkspaceRemoteSnapshot`
  - `SessionWorkspaceRemoteDaemonSnapshot`
  - `SessionWorkspaceRemoteProxySnapshot`
- Added matching generated TypeScript types and exports.
- Added Tauri control-socket methods:
  - `workspace.remote.status`
  - `workspace.remote.configure`
  - `workspace.remote.disconnect` / `workspace.remote.clear`
- `workspace.list` / `workspace.current` now expose a stable `remote` object instead of always returning `null`.
- `workspace.remote.configure` validates `port` and `local_proxy_port` as `1..=65535`, accepting numeric strings for parity with the existing API contract.
- When a remote workspace has `local_proxy_port`, the Windows/Tauri session layer publishes `socks5://127.0.0.1:<port>` into pane-local `browser_proxy_url`.
- Browser opens inside a configured remote workspace inherit the remote proxy URL automatically.
- `surface.list` continues exposing `browser_proxy_url`, so external clients can observe the proxy-bound browser state.
- Added compile-checked tests for:
  - disconnected remote default payload,
  - configured remote proxy endpoint payload,
  - `local_proxy_port` parser behavior,
  - browser open inheriting a configured remote proxy URL.
- Cleaned the golden-session test helper so explicit pane fixtures cover the newer pane fields without repeatedly hand-listing them.
- Added narrow `#[allow(dead_code)]` annotations to intentional proxy parser seams and a test-only control helper so broader non-run Rust compile gates pass.

Why this matters:

- The previous slice made the UI capable of consuming `browser_proxy_url`.
- This slice gives the Windows/Tauri control plane a concrete way to publish remote proxy state into that field, matching the macOS `workspace.remote.configure/status` shape closely enough for the browser pane to be rebound.
- It still does not create the actual local SOCKS5/HTTP CONNECT broker or daemon RPC tunnel. A real broker must own the configured local port before this can satisfy live remote egress/inspection parity.

Verification:

- `cargo fmt -p cmux-core -p cmux-desktop -p cmux-golden` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_proxy_bind_conflict.py` passed.

Known verification limitation:

- Focused runnable `cmux-desktop` Rust tests still fail to launch locally with Windows `STATUS_ENTRYPOINT_NOT_FOUND`, matching the existing local runtime/DLL issue. The no-run compile gates pass.

Still incomplete:

- The TODO row remains unchecked.
- `workspace.remote.configure` publishes a configured endpoint but does not spawn or supervise a local broker.
- The Windows/Tauri desktop still needs the live remote proxy broker/tunnel equivalent to Swift `RemoteDaemonProxyTunnel`/`RemoteDaemonProxySession`.
- The live broker must call `record_proxy_http_exchange_observation` (or a richer TLS-aware observation path) for real traffic before the proxy observability TODO is complete.

## Windows Remote Proxy Broker Handshake Foundation Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `apps/desktop/src-tauri/src/remote_proxy.rs`.
- Ported the local proxy handshake foundation from macOS `RemoteDaemonProxySession`:
  - SOCKS5 greeting parsing with no-auth negotiation.
  - SOCKS5 CONNECT request parsing for IPv4, domain, and IPv6 targets.
  - HTTP CONNECT request parsing, including bracketed IPv6 targets.
  - Pipelined payload preservation after SOCKS5/CONNECT handshake bytes.
  - Success/failure response bytes for SOCKS5 and HTTP CONNECT.
  - A small `ProxyHandshake` state machine for split reads: SOCKS5 greeting -> SOCKS5 request, or one-shot HTTP CONNECT.
- Reused the module's `loopback_socks5_proxy_url` helper from the session remote-proxy publication path, so `browser_proxy_url` generation and the broker foundation share one URL format.
- Added compile-checked tests for:
  - no-auth SOCKS5 negotiation;
  - no-auth rejection;
  - SOCKS5 domain/IPv4/IPv6 target parsing;
  - SOCKS5 pipelined payload preservation;
  - HTTP CONNECT host/port and IPv6 target parsing;
  - HTTP CONNECT pipelined payload preservation;
  - split-read SOCKS5 state-machine progression;
  - one-step HTTP CONNECT state-machine progression.

Why this matters:

- The previous slice let Windows/Tauri publish a proxy endpoint into browser pane state.
- This slice starts the actual local broker port. It gives the future listener/daemon connector the exact handshake product it needs: target host/port, local success/failure bytes, protocol kind, and pending payload bytes that must be forwarded immediately after `proxy.open`.
- This intentionally mirrors the macOS behavior that fixed SOCKS pipelined-payload loss.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_proxy_bind_conflict.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- The Windows/Tauri desktop still needs a loopback listener that owns `local_proxy_port`.
- The broker still needs a daemon RPC connector for `proxy.open`, `proxy.write`, `proxy.close`, and `proxy.stream.subscribe`.
- The live broker must feed observed cleartext exchanges into `record_proxy_http_exchange_observation` and handle TLS tunnels honestly as opaque streams.

## Windows Remote Proxy Loopback Broker Foundation Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Extended `apps/desktop/src-tauri/src/remote_proxy.rs` from a parser-only module into a listener/broker foundation.
- Added `ProxyConnector` and `ProxyStream` traits. The broker is connector-driven so production can plug in daemon RPC instead of accidentally using local direct TCP egress.
- Added `LoopbackProxyBroker`:
  - binds `127.0.0.1:<port>`;
  - supports `port = 0` for OS-assigned test/dev ports;
  - exposes `local_port()` and `proxy_url()`;
  - owns an accept loop with cooperative shutdown;
  - spawns per-client proxy sessions.
- Added per-client broker flow:
  - read and process the SOCKS5/HTTP CONNECT handshake state machine;
  - write intermediate SOCKS5 no-auth responses;
  - on `OpenStream`, call the connector with protocol + target host/port;
  - write protocol-specific success/failure bytes to the browser/client;
  - forward preserved pipelined payload bytes to the remote stream before relay;
  - relay client<->remote bytes bidirectionally using cloned stream halves.

Why this matters:

- The previous slice produced the handshake target and pending payload. This slice turns that into the actual loopback listener/session shell that WebView2 can point at.
- The only intentional missing production piece is now the daemon-backed connector that opens/writes/subscribes/closes `cmuxd-remote` proxy streams. The broker should not need to be redesigned for that.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_proxy_bind_conflict.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- `LoopbackProxyBroker` is not yet started from `workspace.remote.configure`.
- The daemon RPC connector for `proxy.open`, `proxy.write`, `proxy.close`, and `proxy.stream.subscribe` is still missing.
- Live traffic observation is still not wired into `record_proxy_http_exchange_observation`.

## Windows Remote Proxy Daemon RPC Codec Foundation Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added daemon proxy RPC request builders in `apps/desktop/src-tauri/src/remote_proxy.rs`:
  - `proxy.open`
  - `proxy.write`
  - `proxy.close`
  - `proxy.stream.subscribe`
- Added response decoders for:
  - opened daemon `stream_id`;
  - `proxy.write` byte counts;
  - structured daemon RPC error details;
  - RPC response id mismatches.
- Added stream event decoding for pushed daemon events:
  - `proxy.stream.data`
  - `proxy.stream.eof`
  - `proxy.stream.error`
- Event decoding filters by expected `stream_id`, ignores unrelated/non-proxy events, and decodes payload bytes from base64.
- Added compile-checked tests covering request wire shape, success/error response decoding, and stream event decoding.

Why this matters:

- The previous broker foundation can accept local SOCKS5/HTTP CONNECT clients and relay through a generic `ProxyConnector`.
- This slice defines and verifies the JSON-lines codec needed for the production daemon-backed connector, without yet starting or supervising a remote daemon process.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_proxy_bind_conflict.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- There is still no running daemon process/client in the Windows/Tauri desktop path.
- There is still no daemon-backed `ProxyConnector` implementation plugged into `LoopbackProxyBroker`.
- `LoopbackProxyBroker` is still not started or supervised from `workspace.remote.configure`.
- Live traffic observation is still not wired into `record_proxy_http_exchange_observation`.

## Windows Remote Proxy Broker Supervisor State Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `RemoteProxyBrokerState` in `apps/desktop/src-tauri/src/remote_proxy.rs`.
- The state owns workspace-keyed `LoopbackProxyBroker` lifetimes behind a mutex.
- Added supervisor helpers to:
  - start a broker for a workspace with an injected `ProxyConnector`;
  - stop a workspace broker;
  - inspect a workspace broker proxy URL;
  - stop all brokers by clearing the state.
- Registered `RemoteProxyBrokerState::default()` with the Tauri app builder in `apps/desktop/src-tauri/src/lib.rs`.
- `configure_workspace_remote_for_control` and `clear_workspace_remote_for_control` now stop any existing broker for that workspace id after mutating the remote snapshot. This prevents stale loopback listeners once live broker start is wired.

Why this matters:

- The broker now has an app-owned lifetime container instead of being only a free-standing type.
- Reconfigure/disconnect has a cleanup seam ready before the production daemon connector starts opening real listeners.
- This deliberately does not start a fake or failing proxy from `workspace.remote.configure`; doing so would make the UI route browser traffic into a nonfunctional tunnel and overclaim backend readiness.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.

Still incomplete:

- The TODO row remains unchecked.
- There is still no running daemon process/client in the Windows/Tauri desktop path.
- There is still no daemon-backed `ProxyConnector` implementation plugged into `LoopbackProxyBroker`.
- `workspace.remote.configure` still publishes configured proxy metadata but does not start a live broker.
- Live traffic observation is still not wired into `record_proxy_http_exchange_observation`.

## Windows Remote Proxy Daemon Connector Foundation Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `DaemonProxyRpcClient` in `apps/desktop/src-tauri/src/remote_proxy.rs`.
- The client owns a line-oriented daemon writer, starts a background reader thread, routes daemon RPC responses by numeric `id`, and routes pushed `proxy.stream.*` events by `stream_id`.
- Added `DaemonProxyConnector`, implementing the broker's `ProxyConnector` trait on top of the daemon RPC client.
- Added `DaemonProxyStream`, implementing `Read`, `Write`, and `ProxyStream` over daemon events and `proxy.write` RPC calls.
- `proxy.open` now flows through `proxy.stream.subscribe` before returning a stream to the broker.
- `proxy.write` returns the daemon's acknowledged `written` count.
- Explicit `shutdown(Write|Both)` sends `proxy.close`; `Drop` only removes local subscription state to avoid blocking destructors on daemon RPC.
- Added a channel-backed fake daemon test harness that proves open -> subscribe -> write -> pushed data -> close request routing without launching SSH or `cmuxd-remote`.

Why this matters:

- The loopback broker now has a real production-shaped connector target instead of only a parser/trait seam.
- A future SSH/child-process slice can plug `cmuxd-remote serve --stdio` stdout/stdin into `DaemonProxyRpcClient::start(...)` and then pass `DaemonProxyConnector` to `LoopbackProxyBroker`.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.

Still incomplete:

- The TODO row remains unchecked.
- There is still no running SSH/child-process daemon stdio client in the Windows/Tauri desktop path.
- `workspace.remote.configure` still does not start a live `LoopbackProxyBroker`.
- Live traffic observation is still not wired into `record_proxy_http_exchange_observation`.

## Windows Remote Proxy Daemon Stdio Process Launcher Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `DaemonProxyProcess` in `apps/desktop/src-tauri/src/remote_proxy.rs`.
- The process launcher starts a child with piped stdin/stdout/stderr, feeds stdout/stdin into `DaemonProxyRpcClient::start(...)`, and exposes a ready `DaemonProxyConnector`.
- Added stderr draining to print daemon diagnostics with a `[cmux-remote-daemon]` prefix instead of blocking on a full stderr pipe.
- `DaemonProxyProcess::drop` kills/waits the child if it is still running, preventing orphaned stdio daemon transports.
- Added `spawn_ssh_daemon_proxy_process(...)`, which takes the existing `cmux_ssh::SshBatchConfiguration` and a remote daemon path, builds canonical `ssh ... <cmuxd-remote> serve --stdio` args through `daemon_transport_arguments(...)`, and returns the same daemon proxy process.

Why this matters:

- The Windows/Tauri proxy stack now has the production-shaped bridge from an SSH-launched `cmuxd-remote serve --stdio` process into the daemon RPC connector.
- The remaining session slice no longer needs to invent stdio plumbing; it needs to supply daemon launch metadata, start this process, pass its connector into `LoopbackProxyBroker`, and publish the broker result into workspace remote state.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.

Still incomplete:

- The TODO row remains unchecked.
- `workspace.remote.configure` does not yet expose enough daemon launch metadata to start `spawn_ssh_daemon_proxy_process(...)` safely.
- `workspace.remote.configure` still does not start a live `LoopbackProxyBroker`.
- Live traffic observation is still not wired into `record_proxy_http_exchange_observation`.

## Windows Remote Proxy Configure Live-Start Plumbing Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- `RemoteProxyBrokerState` now stores a workspace runtime containing both `LoopbackProxyBroker` and an optional `DaemonProxyProcess`, so a live SSH daemon stdio process can stay alive for as long as its local broker is registered.
- Added `RemoteProxyBrokerState::start_ssh_workspace_broker(...)`, which:
  - stops any existing broker for the workspace;
  - launches `cmuxd-remote serve --stdio` over SSH through `spawn_ssh_daemon_proxy_process(...)`;
  - creates a `DaemonProxyConnector`;
  - starts the loopback SOCKS5/HTTP CONNECT broker on the requested local port;
  - stores the broker and process as one runtime.
- Extended `WorkspaceRemoteControlConfig` with optional daemon launch metadata:
  - `remote_daemon_path`
  - `identity_file`
  - `ssh_options`
- `workspace.remote.configure` now accepts:
  - `remote_daemon_path` / `remoteDaemonPath`
  - `identity_file` / `identityFile`
  - `ssh_options` / `sshOptions` as an array or newline-separated string
- `configure_workspace_remote_for_control` now attempts a live SSH daemon proxy start only when all required inputs are present:
  - `transport == "ssh"`
  - `auto_connect == true`
  - `local_proxy_port` is set
  - `remote_daemon_path` is set
- On live-start failure, the workspace remote snapshot is marked `state: "error"`, `connected: false`, proxy state `unavailable`, `error_code: "proxy_unavailable"`, `detail` set to the launch/bind error, and the configured local proxy port is preserved in `conflicted_ports`.

Why this matters:

- This is the first Windows/Tauri session-control path that can start a real local proxy broker backed by an SSH-launched `cmuxd-remote serve --stdio` process.
- Existing metadata-only configure calls still work, preserving current control-socket contract behavior while enabling a production path for callers that provide the daemon path.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.

Still incomplete:

- The TODO row remains unchecked.
- `cmux ssh` / the Windows/Tauri bootstrap flow still needs to pass the actual resolved remote daemon path into `workspace.remote.configure`.
- Live broker traffic observation is still not wired into `record_proxy_http_exchange_observation`.
- Full completion still needs end-to-end verification with real remote browser egress, websocket continuity, reconnect handling, and browser network records.

## Windows Remote Proxy Relay-Map Launch Plumbing Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `SshBatchConfiguration::daemon_transport_arguments_from_relay_map(...)` in `crates/cmux-ssh/src/ssh_batch.rs`.
- The new SSH argv builder reads the bootstrap-installed remote mapping file:
  - `$HOME/.cmux/relay/<relay_port>.daemon_path`
  - then execs that daemon as `serve --stdio`
  - preserves persistent daemon slot arguments when configured
- Added `spawn_ssh_daemon_proxy_process_from_relay_map(...)` in `apps/desktop/src-tauri/src/remote_proxy.rs`.
- Added `RemoteProxyBrokerState::start_ssh_workspace_broker_from_relay_map(...)`, retaining the SSH daemon stdio process and local broker as one workspace runtime.
- Extended `WorkspaceRemoteControlConfig` with optional `remote_daemon_relay_port`.
- `workspace.remote.configure` now accepts:
  - `remote_daemon_relay_port`
  - `remoteDaemonRelayPort`
- `configure_workspace_remote_for_control` now starts live SSH daemon proxy transport by priority:
  - explicit `remote_daemon_path`, if provided;
  - otherwise relay-map lookup via `remote_daemon_relay_port`, if provided;
  - otherwise preserve metadata-only behavior.
- Added parser coverage for `ssh_options` / `sshOptions` array and newline-string forms.

Why this matters:

- The existing remote bootstrap tests already assert the relay-specific daemon path mapping file exists on the remote host.
- This slice lets Windows/Tauri use that established bootstrap artifact without needing the local CLI/control path to know the final remote daemon binary path directly.
- It narrows the remaining `cmux ssh` integration work to passing the known `remote_relay_port` into `workspace.remote.configure` once the Windows CLI bootstrap path is ported.

Verification:

- `cargo fmt -p cmux-ssh -p cmux-desktop` passed.
- `cargo test -p cmux-ssh -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- The Windows `cmux ssh` command path still does not appear to have a first-class `command_forward.rs` mapping in this port, so the new configure metadata is not yet automatically supplied by the CLI.
- Live broker traffic observation is still not wired into `record_proxy_http_exchange_observation`.
- Full completion still needs end-to-end verification with real remote browser egress, websocket continuity, reconnect handling, and browser network records.

## Windows Remote Proxy Broker Traffic Observation Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Added `ProxyTrafficObservation` and `ProxyTrafficObserver` in `apps/desktop/src-tauri/src/remote_proxy.rs`.
- Added bounded upstream/downstream capture in the local broker relay path:
  - captures up to `256 KiB` in each direction;
  - marks truncation separately for upstream and downstream;
  - includes target host/port, SOCKS5 vs HTTP CONNECT protocol, start time, and completion time;
  - preserves SOCKS/CONNECT pipelined payload bytes in the upstream capture.
- Added `LoopbackProxyBroker::start_with_observer(...)`.
- Live SSH daemon broker starts now pass a workspace/browser observer from `session.rs`.
- Added `WorkspaceBrowserProxyObserver`, which:
  - resolves the workspace at observation time;
  - finds browser panes using the broker's proxy URL;
  - records into `browser.network.requests` only when exactly one matching browser pane exists, avoiding false multi-pane attribution;
  - feeds cleartext HTTP request/response bytes into the existing `record_proxy_http_exchange_observation(...)` parser.
- Added tests for:
  - bounded/truncated proxy capture;
  - relay observation of pipelined upstream request bytes and downstream response bytes;
  - workspace browser-panel matching by proxy URL.

Why this matters:

- The broker no longer just tunnels bytes blindly; the live proxy path now has the stream-boundary observation needed by the remaining browser TODO.
- Cleartext proxied HTTP can now become normal browser network records with URL, method, headers, body prefix, status, and timing when attribution is unambiguous.
- TLS-over-CONNECT remains intentionally opaque unless/until MITM interception is explicitly added.

Verification:

- `cargo fmt -p cmux-ssh -p cmux-desktop` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- The Windows `cmux ssh` command path still needs to supply live proxy launch metadata automatically.
- Per-WKWebView attribution is currently conservative: live records are emitted only when exactly one browser pane matches the proxy URL in the workspace.
- Full completion still needs end-to-end verification with real remote browser egress, websocket continuity, reconnect handling, and browser network records.

## Windows Remote Proxy Pane-Scoped Browser Proxy Attribution Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Tightened browser proxy URL assignment in `apps/desktop/src-tauri/src/session.rs`.
- `apply_open_browser_url(...)` now applies the remote proxy URL only to the pane/panel being opened as a browser, instead of writing it across the entire layout root.
- `apply_workspace_browser_proxy_url(...)` now applies configured proxy URLs only to panes that are already browser surfaces; clearing still removes proxy URLs from all panes.
- Added recursive helpers:
  - `set_layout_browser_proxy_url_for_panel(...)`
  - `set_browser_layout_proxy_url_for_browser_panes(...)`
- Added a regression proving that in a split remote workspace, opening `surface-2` as a browser sets the proxy on `surface-2` and leaves `surface-1` unproxied.

Why this matters:

- The live broker observer attributes traffic by matching proxy URL to browser panes. Before this slice, opening one browser could smear the proxy URL across unrelated panes, making future per-WebView attribution ambiguous or wrong.
- This moves the session model closer to real per-WebView proxy identity: only browser panes that actually need the WebView proxy carry the proxy URL.

Verification:

- `cargo fmt -p cmux-ssh -p cmux-desktop` passed.
- `cargo test -p cmux-ssh -p cmux-desktop --lib --tests --no-run` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- Multiple active browser panes can still intentionally share a workspace proxy URL; full per-WebView attribution for that case needs per-pane broker identity or another correlation signal.
- The Windows `cmux ssh` command path still needs to supply live proxy launch metadata automatically.
- Full completion still needs end-to-end verification with real remote browser egress, websocket continuity, reconnect handling, and browser network records.

## Windows Remote Proxy Per-Pane Broker Identity Foundation Slice - 2026-07-09

Advanced the remaining browser proxy observability row again, but did not mark it complete.

What changed:

- Extended `RemoteWorkspaceProxyRuntime` in `apps/desktop/src-tauri/src/remote_proxy.rs` with `panel_brokers`.
- Added `RemoteProxyBrokerState::start_workspace_panel_broker(...)`:
  - requires an already-running workspace daemon process;
  - starts a new loopback broker on port `0` for a specific panel;
  - reuses the workspace daemon RPC connector;
  - stores the per-panel broker under the panel id;
  - returns the unique pane proxy URL.
- Added `RemoteProxyBrokerState::stop_panel_broker(...)`.
- Added `PanelBrowserProxyObserver`, which records proxy observations directly to one panel without workspace/proxy-url matching.
- `open_browser_url_in_panel`, `split_browser_for_control`, and `session_split_browser` now attempt to replace the inherited workspace proxy URL with a unique per-panel broker URL when a live workspace daemon runtime exists.
- `close_panel_for_control` and `session_close` now stop per-panel brokers when their panel is closed.

Why this matters:

- This is the foundation for true per-WebView attribution in multi-browser workspaces. New browser panes can now receive distinct proxy endpoints while sharing the same remote daemon RPC transport.
- Observations from those pane brokers are directly attributed to the owning panel, avoiding the conservative "only record when exactly one pane matches proxy URL" fallback.

Verification:

- `cargo fmt -p cmux-ssh -p cmux-desktop` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-golden --lib --tests --no-run` passed.
- `python -m py_compile tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py tests_v2\test_browser_api_unsupported_matrix.py` passed.

Still incomplete:

- The TODO row remains unchecked.
- Existing browser panes present before live daemon startup still use the workspace proxy URL unless a later migration pass assigns them panel brokers.
- The Windows `cmux ssh` command path still needs to supply live proxy launch metadata automatically.
- Full completion still needs end-to-end verification with real remote browser egress, websocket continuity, reconnect handling, and browser network records.

## Compact Handoff - Browser/Windows Parity State - 2026-07-09

Active user goal: feature/UI/backend parity between `ashlr-mux` and `cmux`; do not mark complete yet.

Current truth:

- The UI is much closer than it was: browser devtools now has a Network tab, command-palette discovery, and CLI access for browser network records.
- Backend support now includes Windows control-socket capability reporting, browser network record storage, bounded request/response body capture, explicit unsupported contracts for WKWebView gaps, and remote-proxy observation foundations.
- The remaining `TODO.md` unchecked row is still:
  - `Browser`
  - `[ ] Per-WKWebView proxy observability/inspection once remote proxy path is shipped (URL, method, headers, body, status, timing)`
- Do not check that row off until there is either a live end-to-end proof or an explicit decision that compile/test-backed broker observation is sufficient.

Recently completed slices:

- Added response-body capture fields to `BrowserNetworkRecord` and observer summaries.
- Added Tauri command/UI for browser network requests in `BrowserSurface`.
- Added command-palette command `palette.browserNetwork`.
- Added CLI `cmux browser network`, `network-requests`, and `requests` aliases.
- Added robust browser CLI ordering for harness-style calls such as `cmux browser --surface surface-1 network requests`.
- Added explicit `not_supported` socket handling for unsupported WKWebView/agent-browser APIs.
- Added `system.capabilities` on the Windows named-pipe control socket.
- Added live remote-proxy broker byte observation and conservative workspace/pane attribution.
- Added per-panel broker identity foundation for new browser panes.

Last intended next slice:

- Add canonical browser v2 socket aliases only where they map to real current behavior:
  - `browser.navigate` -> existing browser open/navigate behavior.
  - `browser.open_split` -> existing split-browser behavior.
  - `browser.url.get` -> pure snapshot handler returning current browser URL for a browser surface.
  - Investigate `browser.reload`, `browser.focus_webview`, and `browser.is_webview_focused`; do not fake these if no real implementation exists.
- Add handled aliases to `CONTROL_SOCKET_METHODS`.
- Add CLI aliases only if they map cleanly to existing behavior.
- Add focused tests for method advertising and CLI mapping.

Useful resume commands:

```powershell
rg -n '"browser\.(navigate|open_split|reload|url\.get|focus_webview|is_webview_focused)"|surface_open_browser|surface_split_browser|browser_set_zoom|browser_webview_command|browser_url' apps/desktop/src-tauri/src/control_socket.rs apps/desktop/src-tauri/src/session.rs crates/cmux-cli/src/command_forward.rs tests_v2 -S
Get-Content apps/desktop/src-tauri/src/control_socket.rs | Select-Object -Skip 720 -First 130
Get-Content apps/desktop/src-tauri/src/control_socket.rs | Select-Object -Skip 1180 -First 135
Get-Content apps/desktop/src-tauri/src/control_socket.rs | Select-Object -Skip 1920 -First 150
Get-Content crates/cmux-cli/src/command_forward.rs | Select-Object -Skip 395 -First 120
```

Verification commands that have recently passed:

```powershell
cargo fmt -p cmux-cli -p cmux-desktop
bun run --cwd apps/desktop/web typecheck
bun test apps/desktop/web/src/components/BrowserSurface.test.tsx
cargo check -p cmux-desktop
cargo test -p cmux-cli --lib -- --nocapture
cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run
python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_browser_cli_agent_port.py scripts\stress-cli-socket-api.py
```

Known limitation on this machine:

- Live socket probes currently fail because no desktop listener is running:
  - `target\debug\cmux.exe rpc system.identify`
  - `target\debug\cmux.exe rpc system.capabilities`
  - Both fail with `could not connect to \\.\pipe\cmux`.
- Runnable Rust desktop test binaries often hit Windows loader/status problems here; `--no-run` compile gates have been the reliable verification path.

Files most relevant to the next slice:

- `apps/desktop/src-tauri/src/control_socket.rs`
- `apps/desktop/src-tauri/src/browser.rs`
- `apps/desktop/src-tauri/src/session.rs`
- `apps/desktop/src-tauri/src/remote_proxy.rs`
- `apps/desktop/web/src/components/BrowserSurface.tsx`
- `apps/desktop/web/src/hooks/useSession.ts`
- `apps/desktop/web/src/styles.css`
- `apps/desktop/web/src/palette/commandCatalog.ts`
- `apps/desktop/web/src/palette/intentPlan.ts`
- `crates/cmux-cli/src/command_forward.rs`
- `crates/cmux-cli/src/dispatch.rs`
- `tests_v2/test_browser_api_unsupported_matrix.py`

Safety reminder:

- The worktree is intentionally very dirty and contains many generated/untracked port files. Do not revert or clean unrelated changes.
- Use `apply_patch` for edits.
- Only advertise socket methods in `system.capabilities` when they are actually handled, either as real behavior or intentional `not_supported`.

## Windows Browser v2 Canonical Alias Slice - 2026-07-09

Advanced the browser API parity layer; the overall goal remains active and the final proxy-observability TODO remains unchecked.

What changed:

- Factored `browser_webview_command_for_control(...)` in `apps/desktop/src-tauri/src/browser.rs`.
  - The existing Tauri UI command still uses the same path.
  - Control-socket callers can now dispatch real native child-webview commands.
  - Missing/not-yet-attached webviews return `attached: false` instead of failing, matching the previous UI command's tolerant behavior.
- Added Windows control-socket methods in `apps/desktop/src-tauri/src/control_socket.rs`:
  - `browser.open_split`
  - `browser.navigate`
  - `browser.reload`
  - `browser.url.get`
  - `browser.focus_webview`
  - `browser.is_webview_focused`
- Added those methods to `CONTROL_SOCKET_METHODS`.
- `browser.open_split` now maps to the real split-browser session behavior and returns an agent-browser-friendly payload with `surface_id`, `panel_id`, `url`, refs, and the full surface snapshot.
- `browser.navigate` now requires a browser surface plus `url`, uses the real session navigation path, and returns the same browser payload.
- `browser.url.get` reads the current browser URL from the authoritative session snapshot and rejects non-browser surfaces.
- `browser.reload` calls the native webview reload helper when attached and returns whether the child webview was attached.
- `browser.focus_webview` selects the surface and calls native child-webview focus when attached.
- `browser.is_webview_focused` reports the current selected/focused browser surface state with `webview_focus_verified: false`, because child `Webview` exposes `set_focus()` but not a direct focus query.
- Added CLI aliases in `crates/cmux-cli/src/command_forward.rs`:
  - `cmux browser navigate ...` -> `browser.navigate`
  - `cmux browser open-split ...` / `open_split` -> `browser.open_split`
  - `cmux browser reload ...` -> `browser.reload`
  - `cmux browser url|get-url|current-url ...` -> `browser.url.get`
  - `cmux browser focus-webview ...` -> `browser.focus_webview`
  - `cmux browser is-webview-focused|webview-focused|is-focused ...` -> `browser.is_webview_focused`
- Added focused tests for:
  - advertised canonical browser v2 methods;
  - browser payload shape;
  - new split browser surface detection;
  - CLI alias mapping.

Verification:

- `cargo fmt -p cmux-desktop -p cmux-cli` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_browser_api_p0.py tests_v2\test_browser_api_comprehensive.py` passed.
- `cargo test -p cmux-cli --lib command_forward::tests::maps_browser_subcommands -- --nocapture` passed.
- `cargo test -p cmux-desktop --lib control_socket::tests::control_socket_methods_advertise_browser_network_and_platform_gaps --no-run` passed.
- `cargo test -p cmux-desktop --lib control_socket::tests::browser_surface_payload_returns_agent_browser_shape --no-run` passed.
- `cargo test -p cmux-desktop --lib control_socket::tests::new_browser_surface_id_prefers_new_browser_surface --no-run` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-cli --lib -- --nocapture` passed, 92 tests.
- `cargo check -p cmux-cli` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run` passed.

Still incomplete:

- The final `TODO.md` browser row is still unchecked.
- Live desktop socket probing is still blocked by the absent `\\.\pipe\cmux` listener in this shell.
- `browser.is_webview_focused` is currently a truthful surface-focus report, not a native child-webview focus query, because Tauri child `Webview` does not expose `is_focused()`.
- Full browser parity still needs live P0/comprehensive agent-browser tests against a running desktop listener, plus end-to-end remote proxy traffic proof.

## Browser Network Proxy Attribution Metadata Slice - 2026-07-09

Advanced the remaining browser/proxy observability work; the final TODO row remains unchecked until live end-to-end proof is available.

What changed:

- Extended `BrowserNetworkRecord` in `apps/desktop/src-tauri/src/browser.rs` with `proxy_attribution`.
  - Non-proxy records use `None`.
  - Workspace fallback proxy observations use `"workspace"`.
  - Per-panel proxy observations use `"panel"`.
- Extended `BrowserNetworkObserverSummary` with `proxy_attribution_mode`.
  - Reports `"none"`, `"workspace"`, `"panel"`, or `"mixed"` based on retained records.
- Added `record_proxy_http_exchange_observation_with_attribution(...)`.
  - Existing no-attribution helper remains for tests/compatibility.
  - `WorkspaceBrowserProxyObserver` now records `"workspace"`.
  - `PanelBrowserProxyObserver` now records `"panel"`.
- Updated Browser Network devtools UI in `BrowserSurface.tsx`.
  - The summary strip now shows proxy attribution mode.
  - Individual proxy records show their attribution, e.g. `panel proxy`.
- Updated `tests_v2/test_browser_api_unsupported_matrix.py`.
  - Live matrix now requires observer `proxyAttributionMode` / `proxy_attribution_mode`.
  - Live matrix now requires each record to include `proxyAttribution` / `proxy_attribution`.

Why this matters:

- The Network panel and CLI/socket payload can now show whether cleartext proxy metadata was attributed by the conservative workspace-shared path or by a true per-panel/per-WebView broker.
- This makes the remaining proxy observability row auditable instead of hidden behind generic `proxy-stream-http` records.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py` passed.
- `cargo test -p cmux-desktop --lib browser::tests::network_requests_records_navigation_metadata --no-run` passed.
- `cargo test -p cmux-desktop --lib browser::tests::proxy_http_exchange_records_cleartext_request_response_metadata --no-run` passed.
- `cargo test -p cmux-desktop --lib browser::tests::proxy_http_exchange_marks_bounded_request_and_response_body_capture --no-run` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run` passed.

Still incomplete:

- The final `TODO.md` browser row is still unchecked.
- Need live desktop listener and remote proxy/browser egress to prove records appear as `"panel"` attribution for real per-WebView traffic.
- TLS-over-CONNECT remains intentionally opaque without MITM interception; cleartext HTTP is the path currently observable.

## Browser Network Detail Inspector UI Slice - 2026-07-09

Advanced the UI parity side of the remaining browser/proxy observability work; the final TODO row remains unchecked until live remote proxy proof exists.

What changed:

- Expanded the Browser Network devtools panel in `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Each network row now includes an `Inspect headers/body` details region.
- The details region renders:
  - sorted request headers;
  - sorted response headers;
  - request body preview;
  - response body preview;
  - empty/unavailable/truncated body states.
- Added compact inspection styling in `apps/desktop/web/src/styles.css`.
- Updated `BrowserSurface.test.tsx` so the static render test proves headers and body previews are actually visible in the Network panel, not merely counted in row metadata.

Why this matters:

- Backend records already contained URL, method, headers, body sizes/snippets, status, timing, source, transport, and proxy attribution. The Network UI was still mostly showing counts and summary chips.
- This closes a concrete UI-behind-backend gap for the remaining browser observability area: developers can now inspect the captured request/response metadata directly from the pane.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 10 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 112 tests.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus remote browser/proxy traffic to prove per-WebView `"panel"` attribution and real cleartext HTTP records end-to-end.

## Remote Browser Proxy Network Live Assertion Slice - 2026-07-09

Added the live proof hook for the final browser/proxy observability row; the row remains unchecked until this runs against a real desktop listener and SSH test host.

What changed:

- Extended `tests_v2/cmux.py` with `browser_network_requests(...)`.
  - Wraps `browser.network.requests`.
  - Supports `url_contains`, `method`, `since_id`, and `limit`.
- Extended `tests_v2/test_ssh_remote_browser_move_rebinds_proxy.py`.
  - After moving a browser surface into the SSH workspace and proving it loads the remote localhost marker page, the test now waits for a matching browser network record.
  - The expected record must include:
    - URL containing the marker file;
    - `source == "proxy-stream-http"`;
    - `proxyAttribution` / `proxy_attribution == "panel"`;
    - `responseStatus` / `response_status == 200`;
    - integer duration timing;
    - response body containing the marker body;
    - non-empty request headers;
    - non-empty response headers.
- Updated the PASS message to make the new network-record assertion explicit.

Why this matters:

- This turns the existing remote browser move/proxy regression into the closest thing to the final acceptance test: real browser navigation, real SSH remote localhost egress, and real panel-attributed `browser.network.requests` metadata.
- It directly verifies URL, method, headers, body, status, timing, and per-panel attribution when run in a live SSH/WebView environment.

Verification:

- `python -m py_compile tests_v2\cmux.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py` passed.
- `python -m py_compile tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_browser_api_p0.py tests_v2\test_browser_api_comprehensive.py` passed.
- `python tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote browser move/proxy regression`.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- The live assertion has not been executed successfully in this shell because `CMUX_SSH_TEST_HOST` is not configured and no live desktop socket/listener is available.

## Remote Browser Favicon/Subresource Network Assertion Slice - 2026-07-09

Strengthened the live proof path for the final browser/proxy observability row; the row remains unchecked until the live SSH/WebView environment can run it.

What changed:

- Extended `tests_v2/test_ssh_remote_browser_favicon_uses_proxy.py`.
- After the test proves a remote localhost page loads and `debug.browser.favicon` receives the favicon image, it now waits for a matching `browser.network.requests` record for `/favicon.ico`.
- The expected favicon/subresource record must include:
  - URL containing `/favicon.ico`;
  - `source == "proxy-stream-http"`;
  - `proxyAttribution` / `proxy_attribution == "panel"`;
  - `responseStatus` / `response_status == 200`;
  - integer duration timing;
  - nonzero response body size;
  - `content-type` response header containing `image/png`;
  - binary body placeholder or captured body text;
  - non-empty request headers.
- Reused the new `cmux.browser_network_requests(...)` helper added in the previous slice.
- Updated the PASS message to mention panel-attributed favicon network metadata.

Why this matters:

- The main-document remote browser assertion proves one cleartext HTTP navigation record.
- This favicon assertion proves subresource traffic is also observable through the per-panel proxy path, which better matches real browser usage and the "URL, method, headers, body, status, timing" TODO language.

Verification:

- `python -m py_compile tests_v2\cmux.py tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` passed.
- `python tests_v2\test_ssh_remote_browser_favicon_uses_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote favicon proxy regression`.
- `python tests_v2\test_ssh_remote_browser_move_rebinds_proxy.py` skipped cleanly here with `SKIP: set CMUX_SSH_TEST_HOST to run remote browser move/proxy regression`.
- `python -m py_compile tests_v2\test_browser_api_unsupported_matrix.py tests_v2\test_browser_api_p0.py tests_v2\test_browser_api_comprehensive.py tests_v2\test_ssh_remote_cli_metadata.py tests_v2\test_ssh_remote_cli_relay.py tests_v2\test_ssh_remote_proxy_bind_conflict.py` passed.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute both remote browser proxy tests successfully.

## Native Browser Proxy Rebind/Recreate Slice - 2026-07-09

Fixed a likely runtime blocker for the final browser/proxy observability row; the row remains unchecked until live remote browser tests pass.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `browserNativeWebviewSyncPlan(...)`.
  - Attached webview + same proxy -> `browser_update_webview`.
  - Not attached -> `browser_attach_webview`.
  - Attached webview + changed proxy -> close first, then `browser_attach_webview`.
- `BrowserSurface` now tracks the proxy URL used by the currently attached native child WebView.
- When the pane proxy changes, e.g. after moving a local browser pane into an SSH workspace or after assigning a per-panel broker, the UI now calls `browser_close_webview` before attaching a fresh child WebView with the new proxy.
- The blank/unmount close paths also clear the tracked native proxy URL.
- Hardened `apps/desktop/src-tauri/src/browser.rs`.
  - If `browser_update_webview` arrives with a changed proxy anyway, the backend now removes/closes the old child WebView and continues by creating a fresh one instead of returning `browser webview proxyUrl can only be changed by recreating the child webview`.
- Added `BrowserSurface.test.tsx` coverage for the proxy-change plan.

Why this matters:

- Tauri/WebView proxy settings are creation-time settings. Before this slice, moving a browser pane into a remote workspace could update session state to a remote/per-panel proxy URL while the already-attached native child WebView continued using its old no-proxy or workspace-proxy configuration.
- That would make the live network assertions flaky or impossible: the UI could show a moved browser pane, but the actual WebView network path would not necessarily be rebound to the per-panel broker.
- This slice makes per-WebView proxy rebinding real at runtime, not just represented in the session snapshot.

Verification:

- `cargo fmt -p cmux-desktop` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 11 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `cargo check -p cmux-desktop` passed.
- `cargo test -p cmux-desktop --lib browser::tests::normalize_proxy_url_accepts_http_and_socks5_loopback_endpoints --no-run` passed.
- `cargo test -p cmux-desktop --lib browser::tests::network_requests_records_navigation_metadata --no-run` passed.
- `cargo test -p cmux-ssh -p cmux-core -p cmux-desktop -p cmux-cli --lib --tests --no-run` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx` passed, 35 tests.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to prove remote browser main-document and favicon/subresource requests now produce panel-attributed network records end-to-end.

## Browser Network Filter UI Slice - 2026-07-09

Closed another UI-behind-backend gap in the browser Network inspector; the final proxy observability row remains live-test gated.

What changed:

- Updated `apps/desktop/web/src/components/BrowserSurface.tsx`.
- Added `browserNetworkRequestParams(...)`.
  - Builds the native `browser_network_requests` payload from UI filter fields.
  - Preserves the existing 50-record default limit.
  - Trims URL filter text and emits it as `urlContains`.
  - Normalizes method filter text to uppercase and emits it as `method`.
- Added Network panel filter controls:
  - `URL contains`;
  - `Method`;
  - `Apply filters`;
  - `Clear`.
- The Network panel now sends `urlContains` and `method` to the backend instead of always requesting only `{ panelId, limit: 50 }`.
- Added styling for the filter row in `apps/desktop/web/src/styles.css`, including mobile stacking.
- Updated `BrowserSurface.test.tsx`.
  - Proves request-param construction.
  - Proves the filter controls render in the Network panel.

Why this matters:

- The backend and CLI already supported URL/method filters for `browser.network.requests`; the UI could only refresh the latest 50 records.
- This makes the Network inspector use the same backend filtering surface, which is especially useful for the final remote proxy proof where users/tests need to find a main document or `/favicon.ico` record quickly.

Verification:

- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx` passed, 12 tests.
- `bun run --cwd apps/desktop/web typecheck` passed.
- `bun test apps/desktop/web/src/components/BrowserSurface.test.tsx apps/desktop/web/src/components/Workspace.test.tsx apps/desktop/web/src/palette/commandCatalog.test.ts apps/desktop/web/src/palette/intentPlan.test.ts` passed, 114 tests.

Still incomplete:

- The final `TODO.md` browser row remains unchecked.
- Need live desktop listener plus SSH test host to execute the remote main-document and favicon/subresource network assertions.
