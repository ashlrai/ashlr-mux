//! Shared fixture I/O for the golden-file parity tests.
//!
//! Fixtures live under `crates/cmux-golden/fixtures/<domain>/<name>.json` and
//! hold the **canonical** JSON rendering (sorted keys, uppercase UUIDs,
//! pretty-printed) for one representative input.
//!
//! ## How parity is asserted
//!
//! Each test produces a `serde_json::Value` from a Rust port, canonicalizes it,
//! and compares the pretty bytes against the committed fixture. The comparison
//! is **byte-for-byte** on the canonical rendering, which is the parity contract
//! (see `cmux_golden::canonicalize`).
//!
//! ## Blessing / regenerating fixtures
//!
//! Set `CMUX_GOLDEN_BLESS=1` to (re)write fixture files from the current Rust
//! output instead of asserting. This is how the **initial Rust-seeded corpus**
//! was produced. It is also the seam the macOS Swift exporter plugs into: the
//! Swift exporter writes the SAME canonical JSON to these SAME paths, and CI
//! runs the Rust tests WITHOUT `CMUX_GOLDEN_BLESS` so any drift fails the build.
//! Swapping authoritative Swift fixtures in requires NO code change here.
//!
//! IMPORTANT: the fixtures currently committed are **Rust-seeded placeholders**.
//! The macOS CI Swift exporter MUST regenerate them and is the source of truth.

#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::Value;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture_path(domain: &str, name: &str) -> PathBuf {
    fixtures_root().join(domain).join(format!("{name}.json"))
}

fn bless_enabled() -> bool {
    matches!(
        std::env::var("CMUX_GOLDEN_BLESS").ok().as_deref(),
        Some("1") | Some("true")
    )
}

/// Assert that the canonical rendering of `value` matches the committed fixture
/// at `fixtures/<domain>/<name>.json`. Under `CMUX_GOLDEN_BLESS=1`, writes the
/// fixture instead.
pub fn assert_canonical_fixture(domain: &str, name: &str, value: &Value) {
    let actual = cmux_golden::canonical_json_string(value);
    let path = fixture_path(domain, name);

    if bless_enabled() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture dir");
        }
        // Trailing newline keeps the files git-friendly.
        std::fs::write(&path, format!("{actual}\n")).expect("write fixture");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing golden fixture {}: {err}\n\
             Run `CMUX_GOLDEN_BLESS=1 cargo test -p cmux-golden` to seed it \
             (Rust placeholder) — the macOS Swift exporter is authoritative.",
            path.display()
        )
    });
    let expected = expected.trim_end_matches('\n');

    assert_eq!(
        actual,
        expected,
        "\ngolden mismatch for {}/{}\nfixture: {}\n\
         If the Rust port intentionally changed, re-bless with CMUX_GOLDEN_BLESS=1, \
         then have the macOS Swift exporter confirm parity.",
        domain,
        name,
        path.display(),
    );
}

/// Convenience for tests that already hold a JSON string (e.g. an encoder that
/// returns a `String`): parse, then assert canonical equality.
pub fn assert_canonical_fixture_from_json_str(domain: &str, name: &str, json: &str) {
    let value: Value = serde_json::from_str(json)
        .unwrap_or_else(|err| panic!("test produced invalid JSON for {domain}/{name}: {err}"));
    assert_canonical_fixture(domain, name, &value);
}
