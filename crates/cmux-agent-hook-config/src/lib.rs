//! Faithful 1:1 Rust ports of the macOS Swift agent hook-config writers.
//!
//! SWIFT SOURCES OF TRUTH:
//! - [`hermes`] ← `enum HermesAgentHookConfig`
//!   (`Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/HermesAgentHookConfig.swift`).
//! - [`rovodev`] ← `enum RovoDevHookConfig`
//!   (`Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/RovoDevHookConfig.swift`).
//!
//! Both Swift types are `enum`s used purely as namespaces of `static` functions
//! that install / uninstall a cmux-owned, marker-delimited hook block inside an
//! agent's YAML config, idempotently and reversibly. Each Swift `enum` becomes a
//! Rust module (`static func` → free `fn`); the nested `Event` structs become
//! [`hermes::Event`] and [`rovodev::Event`]. The string helpers that the Swift
//! files duplicate verbatim (`leadingWhitespace`, `serialized`, `yamlDoubleQuoted`,
//! and the Foundation `CharacterSet` trims) are hoisted into the private
//! [`common`] module and shared.
//!
//! DIVERGENCES (all sanctioned platform swaps; behavior otherwise identical):
//! - The namespace `enum` → a Rust module; `static func` → free `fn`.
//! - Foundation `Data(_:).base64EncodedString()` / `Data(base64Encoded:)` →
//!   the `base64` crate's `STANDARD` engine (canonical padding, strict decode),
//!   matching Foundation's default strict behavior.
//! - Foundation `CharacterSet.whitespaces` / `.whitespacesAndNewlines` trims are
//!   approximated by their ASCII members only (see the [`common`] module note);
//!   the config files these transform are ASCII YAML.
//! - `NSRegularExpression` (ICU) → the `regex` crate. The patterns are anchored
//!   ASCII line patterns, so `^`/`$`/`\s`/`\S` semantics coincide.
//!
//! OUT OF SCOPE: the Swift `enum HermesAgentHookAllowlist` (a JSON approvals
//! transform living in the same Swift file) is not ported by this crate.

mod common;

pub mod hermes;
pub mod rovodev;
