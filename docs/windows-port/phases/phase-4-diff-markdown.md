# Phase 4 — Diff & markdown surfaces   [M]

**Goal:** reuse cmux's **diff viewer** and **markdown/mermaid renderer** as in-app
panels — two of the highest-reuse assets, near-wholesale.

**Depends on:** Phase 1 (host bridge). Can run in parallel with Phase 2/3.

## Context

Both already run in WKWebView on macOS, so they are WebView2-portable. The work is
the **host contract**, not the UI.

## Tasks

1. **Diff viewer** — build the `@pierre/diffs` + `@pierre/trees` viewer (from
   `webviews/`) into the app; wire the **`worker-pool`** (large-diff performance);
   feed diffs through the host bridge.
2. **Markdown/mermaid viewer** — copy the static `Resources/markdown-viewer/`
   renderer (marked + highlight.js + mermaid + vega); render agent/file markdown.
3. **Diff comments** — port the `DiffCommentsBridge` contract to Tauri (the
   comments channel used by the diff UI).
4. **Data sources** — define where diffs come from (git integration scope TBD;
   start with explicit diff payloads, add a git provider later).

## Reuse
Near-wholesale: `webviews/` diff viewer + worker pool, `Resources/markdown-viewer`.

## New code
Host-bridge adapters for the diff/comments channels; the diff data feed.

## Deliverable
View diffs (incl. large files) and rich markdown (incl. mermaid/vega) in-app.

## Acceptance
- Diff renders large files smoothly via the worker pool.
- Markdown + mermaid + vega render.
- Diff comments round-trip through the bridge.

## Risks
- Worker-pool behavior under Vite/WebView2 (bundling web workers).
- Scope of the git/diff **data source** — keep the first cut minimal.

## Touchpoints
`cmux/webviews/src` (diff, `worker-pool.ts`, `pierre-options.ts`, `comments/`),
`cmux/Resources/markdown-viewer`, `Sources/Panels/MarkdownWebRenderer.swift` +
`DiffCommentsBridge.swift` (contract reference), `apps/desktop/src-tauri`.

---

## Research-derived plan (2026-07-01, ultracode workflow `w5kvotmvq`)

**Contract (verified from source).** Both surfaces reuse their **JS/TS bundles
VERBATIM**; only the native WKWebView handlers are re-implemented in Rust/Tauri.
The Windows host shim (`apps/desktop/web/src/host/host.ts`, already built) already
maps the two `webkit.messageHandlers` channels to the two Rust commands this phase
adds: `MAC_HOST_CHANNELS` has `cmuxLib → cmux_lib_rpc` and
`cmuxDiffComments → diff_comments_rpc`, so `webviews/src/comments/bridge.ts` and
`shell.html`'s `window.webkit.messageHandlers.*` calls flow through unchanged.
- **Markdown** (`MarkdownWebRenderer.swift`): a per-panel WKWebView that
  `loadHTMLString`s `Resources/markdown-viewer/shell.html` (marked / highlight.js /
  github-markdown.css inlined) with a `file://` base URL. `cmuxLib` handles two
  shapes: `{lib:'mermaid'|'vega-lite'}` → native concatenates the bundled lazy JS
  (mermaid.min.js 2.5MB, or vega + vega-lite + vega-embed in order) and
  `evaluateJavaScript`-injects it, then calls `window.__cmuxLibLoaded(name)` (the
  postMessage reply is IGNORED — shell.html awaits the callback); and
  `{action:'resolveMarkdownFile'|'openMarkdownFile',requestId,path}`. Two
  URL-scheme handlers: `cmux-local-image` (path-jailed to the md file's dir, mime
  allowlist) and `cmux-remote-image`.
- **Diff**: the React/`@pierre/diffs` bundle (`webviews/src/**`) built by vite into
  `Resources/markdown-viewer/webviews-app/**` + shiki grammar chunks under
  `diff-viewer/**`. `DiffCommentsBridge.swift` (name `cmuxDiffComments`) dispatches
  `comments.list/save/delete` against `DiffCommentStore`, trust-gated to main-frame
  pages of a registered `cmux-diff-viewer://<token>/` session
  (`CmuxDiffViewerURLSchemeHandler.hasActiveSession(token)`; per-token allowlist
  ≤1024 files, mime + extension validation, 24h expiry).

**Do not rewrite the web seams** (host.ts shim + both channel mappings + the reused
bundles). Phase 4 is the Rust/Tauri side plus two thin host wrappers.

### Build order (concrete)
1. **Bundle assets:** extend `tauri.conf.json` `bundle.resources` to also ship
   `shell.html` + top-level `*.js`/`*.css` libs + `diff-viewer/**` (today only
   `webviews-app/**` is bundled) so shell + lazy libs + shiki grammars reach Windows.
2. **`crates/cmux-diff` → `comment_store.rs`:** port `DiffCommentStore.swift` —
   per-repo JSON at `data_dir/cmux/diff-comments/<repoKey>.json`, ISO8601 dates,
   pretty + sorted-keys, upsert preserves `createdAt`, idempotent delete. Headless
   unit tests (keep in-lib to dodge os-4551).
3. **`cmux-diff` → `diff_scheme.rs`:** port `CmuxDiffViewerURLSchemeHandler`
   session/token model — `register(token, files)` with ≤1024-file allowlist, mime +
   extension match, trusted-root jail, 24h expiry, `hasActiveSession`,
   `registeredFile`. Defer the two git branch-picker routes.
4. **`crates/cmux-markdown` (or module):** port `MarkdownPanelFileLinkResolver`
   (`isMarkdownPathLike` + resolve-relative), shell HTML placeholder substitution
   (`MarkdownViewerAssets.shellHTML(isDark)`), local-image path-jail + mime map +
   remote-image fetch. Mind the `standardizingPath` macOS-vs-Windows gotcha.
5. **Register custom URI-scheme protocols** in `lib.rs` via
   `register_asynchronous_uri_scheme_protocol`: (a) `cmux-diff-viewer` → serve from
   the cmux-diff session allowlist; (b) `cmux-md` (new) → shell.html + static libs +
   css; (c) `cmux-local-image` + `cmux-remote-image` → image handlers. These become
   WebView2 `WebResourceRequested` handlers on Windows.
6. **`markdown.rs`:** `MarkdownState` + `cmux_lib_rpc` (lib injection via
   `webview.eval` + `__cmuxLibLoaded`; resolve/open actions) + host→webview push
   helpers (`__cmuxRenderMarkdown` / `__cmuxApplyTheme` / `__cmuxSetMarkdownZoom` /
   font-family / maxWidth).
7. **`diff.rs`:** `DiffState` + `diff_comments_rpc` (trust-gate via cmux-diff token
   from the calling webview URL, then dispatch to `comment_store`, returning
   `NativeReply`; reject untrusted frames with code `not_allowed`).
8. **Register both commands** in `lib.rs invoke_handler!` alongside
   ping/terminal/session, `.manage(MarkdownState)` / `.manage(DiffState)`; confirm
   names match `host.ts` `MAC_HOST_CHANNELS` (`cmux_lib_rpc` / `diff_comments_rpc`).
9. **Web hosts:** `apps/desktop/web/src/surfaces/MarkdownPanel.tsx` and
   `DiffPanel.tsx` — thin wrappers mounting the reused shell/diff surface in a
   webview under the new schemes, calling `installMacHostShims` so the reused bridges
   bind. No diff/comment logic (all reused).
10. **Tests:** Rust — comment_store round-trip + repoKey stability + JSON
    byte-parity; diff_scheme token/allowlist/mime/expiry; markdown path-jail
    (reject escapes) + resolver. TS — reuse existing `webviews/test`
    comment/anchor tests unchanged to prove the bundle still passes.
11. **Parity audit:** no new user-facing strings expected (bridge errors reuse
    macOS `userMessage`s); confirm codes `not_allowed` / `invalid_request` match
    `DiffCommentsBridge`.

### Host-bridge channels
- **`cmuxLib`** — methods: `lib`, `resolveMarkdownFile`, `openMarkdownFile`;
  host→webview events: `__cmuxRenderMarkdown`, `__cmuxLibLoaded`,
  `__cmuxSetMarkdownZoom`, `__cmuxApplyTheme`, `__cmuxMarkdownFileResolved`.
- **`cmuxDiffComments`** — methods: `comments.list`, `comments.save`,
  `comments.delete`; no push events.

### New Rust commands
- **`cmux_lib_rpc(webview, state: MarkdownState, message) -> NativeReply`** —
  services the markdown `cmuxLib` channel: on `{lib}` reads the bundled lazy library
  source once-per-webview-lifetime and `webview.eval()`s it + `__cmuxLibLoaded(name)`;
  on `resolveMarkdownFile`/`openMarkdownFile` ports the file-link resolver. Mirrors
  `MarkdownWebRenderer.Coordinator.handleLibRequest`.
- **`diff_comments_rpc(webview, state: DiffState, message) -> NativeReply`** —
  services `cmuxDiffComments`: trust-gates the calling webview's `cmux-diff-viewer://
  <token>` session (`not_allowed` otherwise), then dispatches
  list/save/delete against the ported `DiffCommentStore`; `invalid_request` on
  malformed params/repoRoot. Ports `DiffCommentsBridge.swift` + `DiffCommentStore.swift`.

### Data model
Diff comments persist as **one JSON file per git repo** under
`%APPDATA%/cmux/diff-comments/<repoKey>.json` (Tauri `app_data_dir` / `dirs::data_dir`,
mirroring macOS `Application Support/cmux/diff-comments/`). `repoKey` = lowercase
`hex(SHA256(canonicalRepoRoot))[..24]`, where `canonicalRepoRoot` is the standardized +
symlink-resolved absolute path (Windows: canonicalize + lowercase drive letter).
File shape: `{ repoRoot: String, comments: [DiffComment] }`. **DiffComment** fields
(`DiffCommentStore.swift`): `id` (UUID), `filePath`, `side` (`additions`|`deletions`),
`startLine`, `endLine`, `endSide?`, `lineText` (anchor text at save time for
re-anchoring), `message`, `submissionText?`, `consumedAt?` (Date; marks
delivered-to-agent so it never re-enters the pending pool), `createdAt`, `updatedAt`.
Encoding must match byte-for-byte: ISO8601 dates, prettyPrinted + sortedKeys; upsert
preserves the original `createdAt`; delete is idempotent (returns changed bool). JS
wire-shape (`webviews/src/comments/types.ts DiffCommentRecord`): same fields,
`submissionText` defaults to `''`, dates as ISO8601 strings, save input omits
id/createdAt/updatedAt. Deferred: `DiffCommentSubmissionPool` (pending TextBox chips,
gated on `consumedAt==nil && submissionText`) until TextBox surfaces are ported.

### Open decisions
- **Serve shell/libs/bundle via custom URI-scheme or command-returned strings?**
  → **Hybrid.** Custom schemes for `shell.html`, css, images, and the whole
  `cmux-diff-viewer` bundle (injecting 2.5MB mermaid via a command string is slow and
  risks CSP/eval limits, and the diff scheme also needs the token trust-gate). Keep
  `cmux_lib_rpc`'s `{lib}` path as a `webview.eval` injection — shell.html awaits
  `window.__cmuxLibLoaded(name)`, not the postMessage reply, so eval preserves parity.
- **Where/how do comments persist on Windows?** → **Mirror macOS exactly** (per-repo
  JSON, SHA256 repoKey, ISO8601, pretty+sorted-keys) per the canonical-fidelity
  directive. Only open sub-question: canonical-repo-root normalization on Windows
  (drive-letter case, UNC, symlink/junction) must yield a stable repoKey — watch the
  documented `standardizingPath` divergence.
- **Trust-gate without the macOS 127.0.0.1 HTTP fallback / branch-picker routes?**
  → Port the custom `cmux-diff-viewer` scheme + token/allowlist/session model now
  (it is the authority `hasActiveSession` checks), drop the macOS HTTP form entirely,
  and **DEFER** the two git branch-picker routes (`/__cmux_diff_viewer_refs`,
  `/__cmux_diff_viewer_branch`) — they need bundled git CLI plumbing and are
  orthogonal to comments.

### Key risks
- WebView2 custom URI-scheme names with hyphens (`cmux-diff-viewer`,
  `cmux-local-image`) must register correctly via
  `register_asynchronous_uri_scheme_protocol`; behavior differs from
  `WKURLSchemeHandler`.
- Canonical-repo-root normalization (drive case, UNC, junction/symlink) must yield a
  stable SHA256 repoKey or comments silently split across files (the `standardizingPath`
  macOS-vs-26 divergence is a direct warning).
- The `{lib}` contract relies on native `evaluateJavaScript` resolving out-of-band
  (postMessage reply ignored, `__cmuxLibLoaded` awaited); a command that only returns
  a value without eval'ing into the webview would hang the mermaid/vega loader.
- mermaid.min.js is 2.5MB and shiki emits ~300 grammar chunks, pushing the diff
  scheme's 1024-file allowlist cap; bundling + per-token registration must stay under
  it (vite already collapses `@pierre/diffs`+shiki into one diff-vendor chunk).
- Trust-gate parity: `diff_comments_rpc` must resolve the CALLING webview's token from
  its URL and reject non-diff-viewer frames (`not_allowed`); a Tauri command doesn't
  scope to a frame by default, so use the webview handle to read origin/token.
- Markdown per-panel isolation: macOS uses one WKWebView per panel with a per-panel
  file base URL + image jail; the Windows host must key `cmux_lib_rpc`/image-scheme
  state per webview (`filePath`, `requestedLibs`) or cross-panel leakage/misjailing
  occurs.
- The two git branch-picker routes and `DiffCommentSubmissionPool` are intentionally
  deferred; leaving `consumedAt`/`submissionText` plumbing half-wired could regress
  once TextBox surfaces land.
