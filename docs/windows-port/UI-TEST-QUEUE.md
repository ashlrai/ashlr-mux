# UI-test queue — items needing the running app (WebView2 / live agents)

Everything below is **built + headless-verified** (compiles, unit-tested) but its
final acceptance needs a live `tauri dev` run in WebView2 or an installed agent CLI.
Accumulated by the overnight milestone-loop so the morning review is one file.

Run the app: `cd apps/desktop/src-tauri && npx @tauri-apps/cli dev` (there is NO
`cargo tauri`; the npm CLI is the one that works). If port 1420 is held by a stale
orphan, kill the `node` PID on 1420 first (`Get-NetTCPConnection -LocalPort 1420`).

## Phase 4 — diff + markdown mount layer (NEW, 2026-07-03, commit `92b69f1de`)

The Tauri commands + custom URI schemes are wired (`diff.rs`/`markdown.rs`/
`schemes.rs`/`lib.rs`); 56 lib tests + clippy green. Live checks needed:

1. **Diff-viewer token gate** — does `webview.url()` inside `diff_comments_rpc`
   return the calling **iframe** URL carrying the `cmux-diff-viewer://<token>/…`
   token, or the main-frame URL? The frozen `bridge.ts` sends NO token, so the
   token source is deliberately a single swappable line in `diff.rs`. If the iframe
   URL isn't visible, decide iframe-embeds-token vs a bridge change.
2. **`webview.eval` delivery** — confirm `cmux_lib_rpc` lazy-lib injection and
   `render_markdown_js` / `apply_theme_js` actually reach the markdown webview.
3. **Custom hyphenated schemes through WebView2** — confirm `cmux-diff-viewer://`,
   `cmux-md://`, `cmux-local-image://`, `cmux-remote-image://` serve bytes (WebView2
   custom-scheme registration can differ from macOS WKWebView).
4. **`bundle.resources` at runtime** — `MarkdownViewerAssets::load` + the diff
   scheme read from the resource dir; the `../../../Resources/markdown-viewer` glob
   likely lands off the `resource_dir/markdown-viewer` subpath Tauri v2 expects.
   Verify the assets resolve at runtime (dev + bundled), fix the glob/base if not.
5. Wire the web surfaces (`MarkdownPanel.tsx` / `DiffPanel.tsx` / `host.ts`) to these
   commands — OUT of scope for the headless lane, needs the running UI.

## Phase 3 — agent sessions (carried from prior sessions, still pending live-verify)

6. **Codex converse** — token streaming + approvals in a live session (Codex CLI
   resolves from `%LOCALAPPDATA%\OpenAI\Codex\bin`).
7. **OpenCode converse** — reply rendering on the fixed binary (session-id + npm
   shim resolver fixes landed `f0d46cce9`/`e3acf88e6`; unit-proven, not yet
   user-confirmed live).
8. Agent-surface **styling** re-confirm (guarded-import approach; should render
   styled, not black).

## How to close an item

When a UI item passes, tick it here and note the commit; when one fails, capture
the repro + fix on `windows-port`. These do NOT block continued headless work.
