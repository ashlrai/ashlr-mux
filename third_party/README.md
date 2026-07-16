# Terminal dependency patches

This directory contains the smallest reproducible local patch needed to match cmux's canonical Ghostty terminal-style behavior on Windows.

## Provenance

| Package | Version | crates.io package checksum |
| --- | --- | --- |
| `alacritty_terminal` | `0.26.0` | `bda177466b9524d59f1b12f0dd30b68696788e9992a7e959021c4a0ed96fcf59` |
| `vte` | `0.15.0` | `a5924018406ce0063cd67f8e008104968b74b563ee1b85dde3ed1f7cb87d3dbd` |

The normalized `Cargo.toml`, original `Cargo.toml.orig`, `src` tree, README, and applicable licenses are copied byte-for-byte from those crates.io packages before applying the changes below. Package extraction markers, package-local lockfiles, documentation, examples, build-service configuration, and upstream integration-test fixtures are intentionally omitted because they are not inputs to these library builds. The workspace lockfile retains the exact resolved transitive dependency graph.

## Intentional semantic delta

- `vte/src/ansi.rs`: parse SGR 53 and 55 as overline and cancel-overline.
- `alacritty_terminal/src/term/cell.rs`: retain blink and overline in cell flags and treat styled blank cells as content during reflow.
- `alacritty_terminal/src/term/mod.rs`: map slow/fast blink, cancel-blink, overline, and cancel-overline into cell flags.
- The tests beside those implementations exercise parsing, retention, cancellation, reset, double underline, and blank-cell reflow.

No broad formatting pass is applied to vendored code. All other copied files should remain byte-identical to their crates.io package source.

## Refresh procedure

1. Download and unpack the exact package versions recorded above with Cargo.
2. In a clean branch, replace each vendored normalized/original manifest, `src` tree, README, and license files from the unpacked package.
3. Verify all copied files are byte-identical before applying the four semantic deltas listed above.
4. Reapply the focused behavioral tests and confirm they fail against the pristine packages for the missing behavior.
5. Reapply only the semantic delta, then run the complete VTE library suite with `ansi`, the complete Alacritty library suite against the local VTE patch, `cargo test -p cmux-terminal`, and `cargo check --workspace --locked`.
6. Update this file and `Cargo.lock` if either package version or package checksum changes.
