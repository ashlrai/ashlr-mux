# cmux Rust crates (M1 — cross-platform core)

Platform-agnostic domain logic extracted from the macOS Swift target into shared,
OS-neutral Rust crates, validated against the Swift originals (see
`windows-port-plan/milestones/M1-core-extraction.md`). Every later Windows
milestone builds on these portable contracts instead of AppKit/Foundation.

| Crate | Purpose | Swift parity source | Status |
|-------|---------|---------------------|--------|
| `cmux-core` | Session/layout snapshot models, keyboard-shortcut `Action` enum + `StoredShortcut`/`when`-clause engine, notification reducer | `Sources/SessionPersistence.swift`, `Sources/KeyboardShortcutSettings.swift`, `Packages/macOS/CmuxSettings/.../ShortcutWhenClause.swift` | Ported (simplified); `Action` enum complete (109 variants) |
| `cmux-ipc` | Socket v2 RPC codec: `{id,method,params}` envelope, lenient/strict parsers, response encoder, NDJSON framing | `Packages/macOS/CmuxControlSocket/.../Wire/*.swift` | Ported |
| `cmux-terminal` | OSC 133 shell-integration command segmentation (`Osc133Parser`) | `Packages/Shared/CmuxAgentChat/.../OSC133CommandParser.swift` | Ported |
| `cmux-agent` | Pure (env + fs injected) agent resolver + `AgentSessionLaunchPlan`; Windows package-manager roots | `Sources/AgentExecutableResolver.swift`, `Sources/AgentSessionLaunchPlan.swift` | Ported (Windows roots) |
| `cmux-golden` | Golden-file parity harness: canonical-JSON assertions vs committed fixtures (session/shortcuts/osc133/ipc) | — (test crate) | Fixtures are Rust-seeded placeholders; macOS Swift exporter is authoritative — see `crates/cmux-golden/swift-exporter/` |
| `cmux-cli` | CLI entrypoint stub | — | Placeholder |

## TS bindings

`@cmux/core-types` (`apps/desktop/packages/core-types`) generates TypeScript types
from the `cmux-core` session models via ts-rs (feature-gated, inert in the default
build), so the web chrome and Rust never drift. Regenerate with
`bun run core-types:generate`; CI guards drift via `bun run core-types:check-drift`.

## Build & test

```bash
cargo test -p cmux-core -p cmux-ipc -p cmux-terminal -p cmux-agent -p cmux-cli -p cmux-golden
cargo clippy -p cmux-core -p cmux-ipc -p cmux-terminal -p cmux-agent -p cmux-cli -p cmux-golden --all-targets -- -D warnings
```

> The `cmux-desktop` Tauri crate's native build requires a host without an
> Application Control policy blocking unsigned build-scripts (`os error 4551`);
> CI runs it on unlocked windows-latest + macos-latest runners.
