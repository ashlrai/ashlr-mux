# 91 — Parity map (archived M0–M16 → new phases)

The archived `windows-port-plan/` milestones (M0–M16) are retained as a
**feature-parity reference** — a checklist of what a complete Windows cmux
includes. They are **not** the build plan. Mapping to the new phases:

| Old milestone | New home | Notes |
|---|---|---|
| M0 Bootstrap & Windows CI | Phase 0 (CI green) + Phase 6 (CI hardening) | Windows CI already green |
| M1 Core extraction | **Phase 0 — done** | `cmux-core` + `core-types` |
| **M2 Terminal engine v1 (`wgpu`/alacritty)** | **Dropped** → xterm.js (Phase 0/1) | native GPU renderer abandoned |
| M3 Process & agent lifecycle | **Phase 0 — done** + Phase 3 | `cmux-process`, `cmux-agent` |
| M4 Daemon/socket/CLI IPC | Phase 0 (core done); CLI per-command → optional track | `cmux-ipc` done; per-command blocked on app/server contract |
| M5 App shell + windowing | Phase 2 | geometry done in Phase 0 (`cmux-windowing`) |
| M6 Tabs, splits & sidebar (web) | Phase 2 (+ sidebar in Phase 5) | |
| M7 Browser pane + webview hosts | Phase 4 (webview hosts) + browser pane later | |
| M8 Agent integration | Phase 3 | canonical transport + reused chat |
| M9 Settings, config, shortcuts, i18n | Phase 5 | |
| M10 Notifications | Phase 6 | store done in `cmux-core` |
| M11 Backend/cloud control plane | Deferred (optional track) | |
| M12 Packaging, signing, update | Phase 6 | |
| M13 Testing & CI hardening | Cross-cutting + Phase 6 | |
| M14 Performance & parity | Phase 6 | perf/latency pass |
| M15 Beta, docs & release | Phase 6 | |
| **M16 Terminal engine v2 (libghostty)** | **Dropped/deferred** | xterm.js is the renderer; revisit only for perf |

## Still-useful docs in the archived plan

- `windows-port-plan/01-subsystem-map.md` — macOS-subsystem → Windows-equivalent
  teardown (good reference when rebuilding a shell area).
- `windows-port-plan/90-cross-cutting-and-risks.md` — the original program-wide
  rules (C1–C10) and risk register (R1–R12).
- `windows-port-plan/milestones/` — per-milestone feature detail = the parity
  checklist.
