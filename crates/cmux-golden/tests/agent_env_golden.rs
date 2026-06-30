//! Golden-file parity for the `cmux-agent` launch-environment policy (M3 WS2).
//!
//! Feeds representative process environments into the Rust port of
//! `AgentLaunchEnvironmentPolicy.selectedEnvironment(from:kind:)` and asserts
//! the curated result matches the committed canonical-JSON fixture. The macOS
//! Swift exporter regenerates these SAME fixtures from the Swift original
//! (`Packages/macOS/CMUXAgentLaunch/.../AgentLaunchEnvironmentPolicy.swift`),
//! so any Swift→Rust drift in the allowlist, the `NODE_OPTIONS` sanitizer, or
//! the hermes-key gating fails the build (see the harness in `support/mod.rs`).
//!
//! The selected-environment map is a flat `{ String: String }`, so the canonical
//! rendering is just sorted keys — the policy's contract is exactly which keys
//! survive and with what values.
//!
//! NOTE: fixtures committed here are Rust-seeded placeholders until the Swift
//! exporter blesses them; the Swift original is authoritative.

mod support;

use std::collections::BTreeMap;

use cmux_agent::{selected_environment, ClaudeConfigContext};
use serde_json::Value;
use support::assert_canonical_fixture;

const DOMAIN: &str = "agent_env";

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Render a selected-environment map to a `serde_json::Value` object.
fn to_value(selected: &BTreeMap<String, String>) -> Value {
    Value::Object(
        selected
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

fn assert_selected(name: &str, env: &BTreeMap<String, String>, kind: Option<&str>) {
    // `inert` Claude context: no CLAUDE_CONFIG_DIR rewrite, so the fixtures are
    // filesystem-independent and reproducible on any CI host. The rewrite logic
    // itself is covered by the unit tests in `cmux-agent`.
    let selected = selected_environment(env, kind, &ClaudeConfigContext::inert());
    assert_canonical_fixture(DOMAIN, name, &to_value(&selected));
}

#[test]
fn allowlist_drops_secrets_keeps_safe_keys() {
    // A realistic mixed environment: secrets, unrelated noise, and several
    // allowlisted provider-config keys. Only the allowlisted keys survive.
    let input = env(&[
        ("AMP_API_KEY", "sk-amp-secret"),
        ("OPENAI_API_KEY", "sk-openai-secret"),
        ("ANTHROPIC_API_KEY", "sk-anthropic-secret"),
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/home/u"),
        ("AMP_URL", "https://amp.example"),
        ("ANTHROPIC_BASE_URL", "https://anthropic.example"),
        ("ANTHROPIC_MODEL", "claude-opus-4-8"),
        ("CODEX_HOME", "/home/u/.codex"),
        ("GH_HOST", "github.example"),
        ("USE_BUILTIN_RIPGREP", "1"),
    ]);
    assert_selected("allowlist_drops_secrets_keeps_safe_keys", &input, None);
}

#[test]
fn node_options_sanitized_injected_require_and_heap_cap() {
    // cmux's injected bootstrap require + heap cap are stripped; the user's own
    // flags survive in order.
    let input = env(&[
        ("OPENAI_API_KEY", "secret"),
        (
            "NODE_OPTIONS",
            "--require /var/folders/cmux-abc/restore-node-options.cjs \
             --max-old-space-size=4096 --enable-source-maps --trace-warnings",
        ),
    ]);
    assert_selected(
        "node_options_sanitized_injected_require_and_heap_cap",
        &input,
        None,
    );
}

#[test]
fn node_options_back_channel_present_zero_drops_node_options() {
    // The parent had no NODE_OPTIONS (PRESENT=0): nothing is forwarded even if a
    // live NODE_OPTIONS exists in the spawning shell.
    let input = env(&[
        ("CMUX_ORIGINAL_NODE_OPTIONS_PRESENT", "0"),
        ("NODE_OPTIONS", "--enable-source-maps"),
        ("ANTHROPIC_MODEL", "claude-opus-4-8"),
    ]);
    assert_selected(
        "node_options_back_channel_present_zero_drops_node_options",
        &input,
        None,
    );
}

#[test]
fn hermes_keys_present_only_for_hermes_kind() {
    let input = env(&[
        ("CUSTOM_BASE_URL", "https://hermes.example"),
        ("HERMES_CODEX_BASE_URL", "https://codex.example"),
        ("HERMES_HOME", "/home/u/.hermes"),
        ("GH_HOST", "github.example"),
    ]);
    assert_selected("hermes_keys_excluded_non_hermes_kind", &input, None);
    assert_selected(
        "hermes_keys_included_hermes_kind",
        &input,
        Some("hermes-agent"),
    );
}

#[test]
fn empty_environment_yields_empty_selection() {
    assert_selected("empty_environment", &env(&[]), None);
}
