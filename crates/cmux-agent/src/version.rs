//! Semantic-version parsing for the OpenCode fork-capability probe (M3 WS3).
//!
//! Port of `Sources/SemanticVersion.swift` + `AgentForkSupport`'s
//! `minimumOpenCodeForkVersion` / `openCodeVersionSupportsFork`
//! (`AgentForkSupport.swift:51`,`:347-352`). OpenCode session forking requires
//! `opencode --version` ≥ 1.14.50; the supervisor runs that probe as a
//! short-lived child (handled in `cmux-process`) and feeds its stdout here.
//!
//! The Swift original extracts the first `(\d+)\.(\d+)(?:\.(\d+))?` in the
//! output (patch defaulting to 0). We reproduce that first-match semantics with
//! a small hand-rolled scanner to avoid a regex dependency.

/// Minimum `opencode --version` that supports session forking.
pub const MINIMUM_OPENCODE_FORK_VERSION: SemanticVersion = SemanticVersion {
    major: 1,
    minor: 14,
    patch: 50,
};

/// A `major.minor.patch` version. Ordering is field-lexicographic, matching the
/// Swift `Comparable` conformance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemanticVersion {
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Patch component (0 when the source omitted it).
    pub patch: u64,
}

impl SemanticVersion {
    /// Extract the first `MAJOR.MINOR` (optionally `.PATCH`) run in `text`,
    /// mirroring `SemanticVersion.first(in:)`. Returns `None` if no such run
    /// exists. Patch defaults to 0 when absent.
    pub fn first_in(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            if !bytes[start].is_ascii_digit() {
                start += 1;
                continue;
            }
            // Greedily read the major digit run.
            let major_end = digit_run_end(bytes, start);
            // Require `.` then at least one digit for the minor component.
            if bytes.get(major_end) == Some(&b'.')
                && bytes.get(major_end + 1).is_some_and(u8::is_ascii_digit)
            {
                let minor_start = major_end + 1;
                let minor_end = digit_run_end(bytes, minor_start);

                let (patch, _patch_end) = if bytes.get(minor_end) == Some(&b'.')
                    && bytes.get(minor_end + 1).is_some_and(u8::is_ascii_digit)
                {
                    let patch_start = minor_end + 1;
                    let patch_end = digit_run_end(bytes, patch_start);
                    (parse_run(bytes, patch_start, patch_end), patch_end)
                } else {
                    (Some(0), minor_end)
                };

                if let (Some(major), Some(minor), Some(patch)) = (
                    parse_run(bytes, start, major_end),
                    parse_run(bytes, minor_start, minor_end),
                    patch,
                ) {
                    return Some(Self {
                        major,
                        minor,
                        patch,
                    });
                }
            }
            // No match starting in this digit run; skip past it.
            start = major_end.max(start + 1);
        }
        None
    }
}

/// Return the index just past the maximal ASCII-digit run beginning at `start`.
fn digit_run_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    end
}

/// Parse the ASCII-digit slice `bytes[start..end]` as a `u64` (None on overflow
/// or empty).
fn parse_run(bytes: &[u8], start: usize, end: usize) -> Option<u64> {
    if start >= end {
        return None;
    }
    std::str::from_utf8(&bytes[start..end])
        .ok()
        .and_then(|digits| digits.parse::<u64>().ok())
}

/// Whether `opencode --version` output reports a fork-capable build
/// (≥ [`MINIMUM_OPENCODE_FORK_VERSION`]). Mirrors
/// `AgentForkSupport.openCodeVersionSupportsFork`.
pub fn opencode_version_supports_fork(version_output: &str) -> bool {
    SemanticVersion::first_in(version_output)
        .is_some_and(|version| version >= MINIMUM_OPENCODE_FORK_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_triple() {
        assert_eq!(
            SemanticVersion::first_in("opencode 1.14.50"),
            Some(SemanticVersion {
                major: 1,
                minor: 14,
                patch: 50
            })
        );
    }

    #[test]
    fn patch_defaults_to_zero() {
        assert_eq!(
            SemanticVersion::first_in("v2.3 build"),
            Some(SemanticVersion {
                major: 2,
                minor: 3,
                patch: 0
            })
        );
    }

    #[test]
    fn takes_first_match_only() {
        // "1.2.3.4" → 1.2.3 (the 4th component is a separate run).
        assert_eq!(
            SemanticVersion::first_in("1.2.3.4"),
            Some(SemanticVersion {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
    }

    #[test]
    fn ignores_leading_non_version_digits() {
        // A lone integer with no `.MINOR` is not a version; the real one wins.
        assert_eq!(
            SemanticVersion::first_in("build 12 — opencode 1.15.0"),
            Some(SemanticVersion {
                major: 1,
                minor: 15,
                patch: 0
            })
        );
    }

    #[test]
    fn no_version_returns_none() {
        assert_eq!(SemanticVersion::first_in("no version here"), None);
        assert_eq!(SemanticVersion::first_in("12.y"), None);
        assert_eq!(SemanticVersion::first_in(""), None);
    }

    #[test]
    fn ordering_is_field_lexicographic() {
        let a = SemanticVersion { major: 1, minor: 14, patch: 50 };
        let b = SemanticVersion { major: 1, minor: 14, patch: 49 };
        let c = SemanticVersion { major: 1, minor: 15, patch: 0 };
        let d = SemanticVersion { major: 2, minor: 0, patch: 0 };
        assert!(b < a);
        assert!(a < c);
        assert!(c < d);
    }

    #[test]
    fn supports_fork_at_and_above_minimum() {
        assert!(opencode_version_supports_fork("opencode 1.14.50"));
        assert!(opencode_version_supports_fork("opencode 1.14.51"));
        assert!(opencode_version_supports_fork("opencode 1.15.0"));
        assert!(opencode_version_supports_fork("opencode 2.0.0"));
    }

    #[test]
    fn does_not_support_fork_below_minimum() {
        assert!(!opencode_version_supports_fork("opencode 1.14.49"));
        assert!(!opencode_version_supports_fork("opencode 1.13.99"));
        assert!(!opencode_version_supports_fork("opencode 0.99.99"));
        assert!(!opencode_version_supports_fork("opencode (unknown)"));
    }
}
