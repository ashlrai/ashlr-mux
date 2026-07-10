# Browser Automation

This document is the practical browser automation guide for cmux agents and CLI users.

## Core Workflow

1. Identify the current context:

```bash
cmux identify --json
```

Use `focused.surface_id` or a short `surface:N` ref when the focused pane is already a browser.

2. Open or target a browser surface:

```bash
cmux browser open https://example.com --json
cmux browser surface:1 goto https://example.com --snapshot-after --json
```

3. Inspect page state before acting:

```bash
cmux browser surface:1 snapshot
cmux browser surface:1 get title --json
cmux browser surface:1 wait --load-state complete --timeout-ms 10000 --json
```

4. Act and verify:

```bash
cmux browser surface:1 fill "#search" --text "cmux" --snapshot-after --json
cmux browser surface:1 click "button[type=submit]" --snapshot-after --json
cmux browser surface:1 get text "#results" --json
```

## Targets

- `surface`: a tab inside a pane. Browser automation targets browser surfaces.
- `pane`: a split region that can contain one or more surfaces.
- `workspace`: a sidebar tab containing panes and surfaces.
- `window`: a native desktop window containing workspaces.

Prefer `surface:N` refs in CLI examples because they are stable within a daemon run and friendlier than UUIDs. The CLI also accepts opaque surface IDs:

```bash
cmux browser surface:1 click "#submit"
cmux browser 018f-cmux-surface click "#submit"
cmux browser --surface 018f-cmux-surface click "#submit"
```

## Supported Automation Families

The Windows/Tauri backend supports the agent-browser-style surface below through the control socket and `cmux browser <surface> ...` CLI grammar:

| Family | CLI examples |
| --- | --- |
| Navigation | `browser open`, `browser goto`, `browser back`, `browser forward`, `browser reload`, `browser url` |
| Page state | `browser snapshot`, `browser eval`, `browser wait`, `browser screenshot` |
| Actions | `browser click`, `dblclick`, `hover`, `focus`, `type`, `fill`, `press`, `keydown`, `keyup`, `check`, `uncheck`, `select`, `scroll`, `scroll-into-view` |
| Getters | `browser get text`, `html`, `value`, `attr`, `url`, `title`, `count`, `box`, `styles` |
| Predicates | `browser is visible`, `enabled`, `checked` |
| Locators | `browser find role`, `text`, `label`, `placeholder`, `alt`, `title`, `testid`, `first`, `last`, `nth` |
| Context/session | `browser frame`, `dialog`, `download`, `cookies`, `storage`, `tab`, `state` |
| Diagnostics | `browser console`, `errors`, `highlight`, `addinitscript`, `addscript`, `addstyle` |
| Network inspection | `browser network`, `browser network clear` |

## Network Inspection

Use Network inspection for request/response observability:

```bash
cmux browser surface:1 network --limit 100 --url-contains api --method POST --json
cmux browser surface:1 network clear --json
```

Records include URL, method, request/response headers, body preview metadata, response status, timing, proxy attribution, and notes. Cleartext proxy traffic includes parsed request/response metadata. Encrypted or otherwise opaque proxy tunnels are represented as tunnel records with byte-preview metadata and explanatory notes.

## WKWebView Hard Gaps

Some agent-browser/CDP-style features do not have correct WKWebView equivalents in the Windows/Tauri runtime. These commands are intentionally exposed as explicit `not_supported` socket errors rather than silently pretending to work:

| Command family | Current behavior |
| --- | --- |
| `browser.viewport.set` | `not_supported` |
| `browser.geolocation.set` | `not_supported` |
| `browser.offline.set` | `not_supported` |
| `browser.trace.start`, `browser.trace.stop` | `not_supported` |
| `browser.network.route`, `browser.network.unroute` | `not_supported` |
| `browser.screencast.start`, `browser.screencast.stop` | `not_supported` |
| `browser.input_mouse`, `browser.input_keyboard`, `browser.input_touch` | `not_supported` |

This keeps automation honest: unsupported platform semantics fail loudly, while supported DOM/WebView automation and Network inspection remain available.
