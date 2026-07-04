//! `SentryNoiseFilter` — classifies Sentry-bound error text so expected,
//! non-actionable CLI socket transport disconnects can be dropped before capture
//! or send.
//!
//! Faithful Rust port of `SentryNoiseFilter.swift`
//! (`Packages/macOS/CmuxFoundation/Sources/CmuxFoundation/SentryNoiseFilter.swift:1-66`).
//! The entire Swift struct is pure (no I/O), so this is a direct behavioral port.

use fancy_regex::Regex as FancyRegex;

/// Classifies Sentry-bound error text so expected, non-actionable transport
/// disconnects can be dropped before capture or send.
///
/// Swift: `SentryNoiseFilter.swift:5` (`public struct SentryNoiseFilter`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SentryNoiseFilter;

impl SentryNoiseFilter {
    /// Creates a noise filter.
    ///
    /// Swift: `SentryNoiseFilter.swift:6` (`public init() {}`).
    #[must_use]
    pub fn new() -> Self {
        SentryNoiseFilter
    }

    /// Returns `true` when a CLI socket transport stage failed because the peer
    /// was absent, refused the connection, or disappeared during a write.
    ///
    /// Swift: `SentryNoiseFilter.swift:10-19`
    /// (`isExpectedCLISocketTransportFailure(stage:message:dataKeys:)`).
    ///
    // DIVERGENCE: Swift's `dataKeys` is a `Set<String>` defaulting to `[]`. Rust
    // has no default arguments and a caller-built `HashSet` would be awkward, so
    // this takes a `&[&str]`; only `.contains` membership is observed, which is
    // identical between a set and a slice for this classifier's purposes.
    #[must_use]
    pub fn is_expected_cli_socket_transport_failure(
        &self,
        stage: &str,
        message: &str,
        data_keys: &[&str],
    ) -> bool {
        if !Self::is_cli_socket_transport_context(stage, data_keys) {
            return false;
        }
        Self::is_expected_cli_socket_transport_message(message)
    }

    /// Returns `true` for expected CLI socket connect/write error messages that
    /// are normal lifecycle races at fleet scale.
    ///
    /// Swift: `SentryNoiseFilter.swift:23-52`
    /// (`isExpectedCLISocketTransportMessage(_:)`).
    #[must_use]
    pub fn is_expected_cli_socket_transport_message(text: &str) -> bool {
        // Swift `text.lowercased()`; ASCII-equivalent to `to_lowercase`.
        let t = text.to_lowercase();

        let is_socket_write_failure =
            t.contains("failed to write to socket") || t.contains("write to socket");
        if is_socket_write_failure {
            return t.contains("broken pipe")
                || contains_errno(32, &t)          // EPIPE
                || t.contains("connection reset")
                || contains_errno(54, &t)          // ECONNRESET
                || t.contains("bad file descriptor")
                || contains_errno(9, &t)           // EBADF after peer/fd teardown
                || t.contains("socket is not connected")
                || contains_errno(57, &t); // ENOTCONN
        }

        let is_socket_connect_failure =
            t.contains("failed to connect to socket") || t.contains("socket not found at");
        if !is_socket_connect_failure {
            return false;
        }

        t.contains("socket not found at")
            || t.contains("no such file or directory")
            || contains_errno(2, &t)               // ENOENT
            || t.contains("connection refused")
            || contains_errno(61, &t) // ECONNREFUSED
    }

    /// Returns `true` when `stage`/`dataKeys` mark this event as belonging to the
    /// CLI socket transport surface.
    ///
    /// Swift: `SentryNoiseFilter.swift:54-59`
    /// (`isCLISocketTransportContext(stage:dataKeys:)`).
    fn is_cli_socket_transport_context(stage: &str, data_keys: &[&str]) -> bool {
        stage == "socket_connect"
            || stage.starts_with("socket_command")
            || data_keys.contains(&"socket_phase")
            || data_keys.contains(&"socket_operation")
    }
}

/// Returns `true` when `text` contains an `errno` annotation for exactly `code`.
///
/// Swift: `SentryNoiseFilter.swift:61-65` (`containsErrno(_:in:)`). The lookbehind
/// `(?<![0-9])` and trailing `(?![0-9])` reject an `errno` code that is a digit
/// substring of a larger number (e.g. `errno 329` must not match `errno 32`).
///
// DIVERGENCE: `regex::escape` stands in for `NSRegularExpression.escapedPattern(for:)`.
// For a purely numeric code string neither escapes anything (digits are regex
// literals), so the compiled pattern is byte-identical to Swift's.
//
// Whitespace class: Swift's `[[:space:]:=]` runs under `NSRegularExpression`
// (ICU), where POSIX `[:space:]` is Unicode-aware (== `\p{White_Space}`, incl.
// U+00A0/U+2028/U+3000). `fancy_regex`/`regex` treat `[:space:]` as ASCII-only,
// so we use Unicode `\s` (== `\p{White_Space}` with default Unicode mode) to
// match ICU exactly; `[\s:=]` therefore mirrors ICU `[[:space:]:=]` semantically,
// not just textually. The lookbehind is why this needs `fancy_regex` (stock
// `regex` has no lookbehind); that dependency is already present for the
// home-path scrubber.
fn contains_errno(code: i64, text: &str) -> bool {
    let escaped_code = regex::escape(&code.to_string());
    let pattern = format!(r"(?<![0-9])errno[\s:=]*{escaped_code}(?![0-9])");
    FancyRegex::new(&pattern)
        .expect("valid built-in errno regex")
        .is_match(text)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Swift: `SentryNoiseFilterTests.swift:7-24`
    /// (`testDropsExpectedCLISocketDisconnectsInSocketStages`).
    #[test]
    fn drops_expected_cli_socket_disconnects_in_socket_stages() {
        let filter = SentryNoiseFilter::new();
        assert!(filter.is_expected_cli_socket_transport_failure(
            "socket_command",
            "CLIError: Failed to write to socket (Broken pipe, errno 32) (Code: 1)",
            &[],
        ));
        assert!(filter.is_expected_cli_socket_transport_failure(
            "socket_command_surface_list",
            "Failed to write to socket (Connection reset by peer, errno 54)",
            &[],
        ));
        assert!(filter.is_expected_cli_socket_transport_failure(
            "socket_connect",
            "Failed to connect to socket at /tmp/cmux.sock (Connection refused, errno 61)",
            &[],
        ));
        assert!(filter.is_expected_cli_socket_transport_failure(
            "socket_connect",
            "Socket not found at /tmp/cmux.sock",
            &[],
        ));
    }

    /// Swift: `SentryNoiseFilterTests.swift:26-35`
    /// (`testKeepsActionableSocketFailures`).
    #[test]
    fn keeps_actionable_socket_failures() {
        let filter = SentryNoiseFilter::new();
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "socket_command",
            "Failed to write to socket (Operation timed out, errno 60)",
            &[],
        ));
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "socket_connect",
            "Failed to connect to socket at /tmp/cmux.sock (Permission denied, errno 13)",
            &[],
        ));
    }

    /// Swift: `SentryNoiseFilterTests.swift:37-50`
    /// (`testErrnoMatchingRequiresExactCode`).
    #[test]
    fn errno_matching_requires_exact_code() {
        let filter = SentryNoiseFilter::new();
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "socket_connect",
            "Failed to connect to socket at /tmp/cmux.sock (Invalid argument, errno 22)",
            &[],
        ));
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "socket_command",
            "Failed to write to socket (Not a socket, errno 329)",
            &[],
        ));
        assert!(filter.is_expected_cli_socket_transport_failure(
            "socket_connect",
            "Failed to connect to socket at /tmp/cmux.sock (errno=2)",
            &[],
        ));
    }

    /// Swift: `SentryNoiseFilterTests.swift:52-58`
    /// (`testKeepsRawSignalAndNonSocketMessages`).
    #[test]
    fn keeps_raw_signal_and_non_socket_messages() {
        let filter = SentryNoiseFilter::new();
        assert!(!SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "SIGPIPE: Signal 13, Code 0"
        ));
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "codex-monitor-start",
            "Failed to write to socket (Broken pipe, errno 32)",
            &[],
        ));
    }

    // --- Edge cases the port notes / parity risks call out -------------------

    /// The transport context also opens via `dataKeys` membership, independent of
    /// `stage`. Swift: `SentryNoiseFilter.swift:57-58`.
    #[test]
    fn data_keys_membership_opens_transport_context() {
        let filter = SentryNoiseFilter::new();
        assert!(filter.is_expected_cli_socket_transport_failure(
            "codex-monitor-start",
            "Failed to write to socket (Broken pipe, errno 32)",
            &["socket_phase"],
        ));
        assert!(filter.is_expected_cli_socket_transport_failure(
            "codex-monitor-start",
            "Failed to write to socket (Broken pipe, errno 32)",
            &["socket_operation"],
        ));
        // An unrelated key does not open the context.
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "codex-monitor-start",
            "Failed to write to socket (Broken pipe, errno 32)",
            &["unrelated_key"],
        ));
    }

    /// `stage.hasPrefix("socket_command")` is a prefix match, not equality.
    /// Swift: `SentryNoiseFilter.swift:56`.
    #[test]
    fn stage_prefix_match_opens_transport_context() {
        let filter = SentryNoiseFilter::new();
        assert!(filter.is_expected_cli_socket_transport_failure(
            "socket_command",
            "Broken pipe write to socket",
            &[],
        ));
        // A stage that merely contains (not prefixes) the token is not a match.
        assert!(!filter.is_expected_cli_socket_transport_failure(
            "x_socket_command",
            "Broken pipe write to socket",
            &[],
        ));
    }

    /// The errno regex's trailing `(?![0-9])` guard rejects a digit
    /// supersequence: `errno 329` must not satisfy the errno-32 check while a
    /// bare `errno 32` does. Swift: `SentryNoiseFilter.swift:61-65`.
    #[test]
    fn errno_lookbehind_rejects_digit_supersequence() {
        // "errno 329" must not satisfy the errno-32 check; the trailing
        // `(?![0-9])` guards it. Message otherwise looks like a write failure.
        assert!(!SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to write to socket (errno 329)"
        ));
        // The bare, exact code does match.
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to write to socket (errno 32)"
        ));
    }

    /// `errno` separators may be whitespace, `:`, `=`, or nothing at all — the
    /// `[[:space:]:=]*` class with `*` quantifier. Swift: `SentryNoiseFilter.swift:63`.
    #[test]
    fn errno_accepts_varied_separators() {
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to connect to socket at /s (errno: 2)"
        ));
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to connect to socket at /s (errno=2)"
        ));
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to connect to socket at /s (errno2)"
        ));
    }

    /// The errno-separator class matches ICU `[:space:]` (== `\p{White_Space}`),
    /// not ASCII-only whitespace: Swift's `NSRegularExpression` `[[:space:]:=]`
    /// matches a Unicode space (e.g. U+00A0 NBSP) between `errno` and the code,
    /// so the Rust `[\s:=]` class must too. Pins the ICU-parity fix.
    #[test]
    fn errno_separator_matches_unicode_whitespace() {
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to connect to socket at /s (errno\u{00A0}2)"
        ));
    }

    /// Named-reason branches match case-insensitively via `lowercased()`.
    /// Swift: `SentryNoiseFilter.swift:24,30-37`.
    #[test]
    fn named_reasons_match_case_insensitively() {
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "FAILED TO WRITE TO SOCKET (BAD FILE DESCRIPTOR)"
        ));
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to write to socket (Socket is not connected)"
        ));
        assert!(SentryNoiseFilter::is_expected_cli_socket_transport_message(
            "Failed to connect to socket at /s (No such file or directory)"
        ));
    }
}
