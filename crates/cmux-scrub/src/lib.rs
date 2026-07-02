//! `cmux-scrub` — PII / secret scrubber.
//!
//! Faithful Rust port of `SentryScrubber.swift` (+ `ScrubberDenylists.swift`,
//! `SentryRegexPattern.swift`). Redacts privacy-sensitive content out of strings
//! and nested values before they leave the device in a crash/error report.
//!
//! [`SentryScrubber`] is a pure value transformer with no Sentry dependency: it
//! scrubs plain strings and recursively walks JSON-like [`ScrubValue`] trees.
//!
//! What it redacts, in priority order on every string (order is load-bearing):
//! URL credentials → tokens/secrets → emails → home/user paths.
//!
//! ```
//! use cmux_scrub::SentryScrubber;
//! let scrubber = SentryScrubber::new("/Users/alice");
//! assert_eq!(
//!     scrubber.scrub("opening /Users/alice/dev/secret with token=sk-abc123def456ghij"),
//!     "opening /Users/<redacted>/dev/secret with token=<redacted-secret>"
//! );
//! ```

mod denylists;
mod pattern;

#[cfg(test)]
mod tests;

use std::sync::OnceLock;

use denylists::{SENSITIVE_KEY_EXACT, SENSITIVE_KEY_MARKERS, VALUE_PATTERNS};
use pattern::Pattern;

/// The placeholder substituted for the redacted home directory leaf.
pub const REDACTED_USER: &str = "<redacted>";
/// The placeholder substituted for an email address.
pub const REDACTED_EMAIL: &str = "<redacted-email>";
/// The placeholder substituted for a token / secret / key / bearer / password.
pub const REDACTED_SECRET: &str = "<redacted-secret>";
/// The placeholder substituted for a raw binary (`Data`) value.
pub const REDACTED_DATA: &str = "<redacted-data>";

/// A JSON-like value tree the scrubber can walk.
///
// DIVERGENCE: Swift models the dynamic tree as `Any` (`String` / `[String: Any]`
// / `[Any]` / scalars, plus `Data`, `URL`, `Date`, `NSNull`). Rust has no
// `Any`-shaped payload, so this owned enum represents the same cases explicitly.
// [`ScrubValue::Object`] uses an ordered `Vec<(String, ScrubValue)>` rather than
// a map so key order round-trips (Swift dictionaries are unordered; tests query
// by key, so ordering is not observable in the ported oracles).
#[derive(Debug, Clone, PartialEq)]
pub enum ScrubValue {
    /// A string; scrubbed by content.
    Str(String),
    /// An integer scalar; passes through untouched (Sentry serializes faithfully).
    Int(i64),
    /// A floating-point scalar; passes through untouched.
    Double(f64),
    /// A boolean scalar; passes through untouched.
    Bool(bool),
    /// Raw bytes; dropped wholesale to [`REDACTED_DATA`] (Sentry would hex-encode).
    Data(Vec<u8>),
    /// A URL, carried as its absolute string; scrubbed as a path-bearing string.
    Url(String),
    /// An array; each element is walked.
    Array(Vec<ScrubValue>),
    /// A `[key: value]` map; walked with keys as redaction boundaries.
    Object(Vec<(String, ScrubValue)>),
    /// A date scalar; passes through untouched.
    Date,
    /// A null / absent value; passes through untouched.
    Null,
}

/// The set of compiled patterns shared by every scrubber instance.
struct Compiled {
    url_userinfo: Pattern,
    email: Pattern,
    user_home: Pattern,
    linux_home: Pattern,
    query_segment: Pattern,
    secret_patterns: Vec<Pattern>,
}

impl Compiled {
    fn new() -> Self {
        Compiled {
            // scheme://user[:pass]@host — group 1 keeps the scheme.
            url_userinfo: Pattern::stock_ci(r"([A-Za-z][A-Za-z0-9+.\-]*://)[^/?#\s]+@"),
            email: Pattern::stock_ci(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}"),
            user_home: Pattern::stock_ci(r#"/Users/[^/\s"']+"#),
            linux_home: Pattern::stock_ci(r#"/home/[^/\s"']+"#),
            query_segment: Pattern::stock_ci(r"[^&;]+"),
            secret_patterns: VALUE_PATTERNS
                .iter()
                .map(|vp| {
                    if vp.case_insensitive {
                        Pattern::stock_ci(vp.source)
                    } else {
                        Pattern::stock(vp.source)
                    }
                })
                .collect(),
        }
    }
}

fn compiled() -> &'static Compiled {
    static COMPILED: OnceLock<Compiled> = OnceLock::new();
    COMPILED.get_or_init(Compiled::new)
}

/// Redacts privacy-sensitive content out of strings and nested values.
///
/// Bound to a home directory whose prefix is redacted wherever it appears.
#[derive(Debug, Clone)]
pub struct SentryScrubber {
    /// The absolute home directory whose prefix is replaced wherever it appears.
    home_directory: String,
    /// A compiled, path-component-bounded pattern for the exact `home_directory`.
    ///
    /// `None` when `home_directory` is empty or `/` (nothing meaningful to
    /// redact). Otherwise it matches the literal home path only when followed by a
    /// path delimiter, quote, whitespace, or end of string, via the lookahead.
    home_pattern: Option<Pattern>,
}

impl SentryScrubber {
    /// Creates a scrubber bound to a home directory.
    ///
    // DIVERGENCE: Swift's `init(homeDirectory:)` defaults the argument to
    // `NSHomeDirectory()`. There is no cross-platform Foundation equivalent (and
    // reading the OS home would make the type impure/untestable), so the home
    // directory is always injected by the caller — no default is provided.
    pub fn new(home_directory: impl Into<String>) -> Self {
        let home_directory = home_directory.into();
        let home_pattern = if home_directory.is_empty() || home_directory == "/" {
            None
        } else {
            // Escape the home path so any regex metacharacter in a username is
            // literal, then bound the match to a full path component. `(?i)` mirrors
            // the Swift `.caseInsensitive` default the pattern is created with.
            //
            // DIVERGENCE: `regex::escape` stands in for
            // `NSRegularExpression.escapedPattern(for:)`; both escape regex
            // metacharacters and leave `/` literal, so the escaped output matches.
            let escaped = regex::escape(&home_directory);
            Some(Pattern::fancy(&format!(r#"(?i){escaped}(?=[/\s"']|$)"#)))
        };
        SentryScrubber {
            home_directory,
            home_pattern,
        }
    }

    /// Returns a copy of `text` with secrets, emails, and home/user paths redacted.
    ///
    /// Redaction order is URL-credentials → secrets → emails → paths.
    pub fn scrub(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let result = self.redact_url_credentials(text);
        let result = self.redact_secrets(&result);
        let result = self.redact_emails(&result);
        self.redact_paths(&result)
    }

    /// Returns `text` scrubbed, or `None` when the input is `None`.
    pub fn scrub_optional(&self, text: Option<&str>) -> Option<String> {
        text.map(|t| self.scrub(t))
    }

    /// Recursively scrubs every string found inside a JSON-like value tree.
    pub fn scrub_value(&self, value: &ScrubValue) -> ScrubValue {
        match value {
            ScrubValue::Str(s) => ScrubValue::Str(self.scrub(s)),
            ScrubValue::Object(dict) => ScrubValue::Object(self.scrub_dictionary(dict)),
            ScrubValue::Array(items) => {
                ScrubValue::Array(items.iter().map(|v| self.scrub_value(v)).collect())
            }
            // Safe scalars Sentry serializes faithfully; no string content.
            ScrubValue::Int(_)
            | ScrubValue::Double(_)
            | ScrubValue::Bool(_)
            | ScrubValue::Date
            | ScrubValue::Null => value.clone(),
            // Sentry stringifies raw bytes to a hex description after beforeSend,
            // which would leak the bytes (e.g. a UTF-8 token). Drop it wholesale.
            ScrubValue::Data(_) => ScrubValue::Str(REDACTED_DATA.to_string()),
            // A URL carries a path Sentry serializes after beforeSend; scrub it.
            ScrubValue::Url(absolute) => ScrubValue::Str(self.scrub(absolute)),
        }
    }

    /// Recursively scrubs every value inside a dictionary, treating sensitive keys
    /// as a redaction boundary.
    pub fn scrub_dictionary(&self, dictionary: &[(String, ScrubValue)]) -> Vec<(String, ScrubValue)> {
        dictionary
            .iter()
            .map(|(key, value)| {
                if Self::is_sensitive_key(key) {
                    (key.clone(), ScrubValue::Str(REDACTED_SECRET.to_string()))
                } else {
                    (key.clone(), self.scrub_value(value))
                }
            })
            .collect()
    }

    /// Recursively scrubs Sentry's two-level `context` map, treating the outer
    /// context name as a redaction boundary.
    pub fn scrub_context(
        &self,
        context: &[(String, Vec<(String, ScrubValue)>)],
    ) -> Vec<(String, Vec<(String, ScrubValue)>)> {
        context
            .iter()
            .map(|(name, inner)| {
                if Self::is_sensitive_key(name) {
                    // The outer context name is the trust boundary: redact every
                    // inner value wholesale rather than recursing.
                    let redacted = inner
                        .iter()
                        .map(|(k, _)| (k.clone(), ScrubValue::Str(REDACTED_SECRET.to_string())))
                        .collect();
                    (name.clone(), redacted)
                } else {
                    (name.clone(), self.scrub_dictionary(inner))
                }
            })
            .collect()
    }

    /// Redacts the values of sensitive parameters in a URL query string,
    /// structurally, keyed off the single maintained denylist.
    pub fn scrub_query_string(&self, query: &str) -> String {
        if query.is_empty() {
            return String::new();
        }
        compiled()
            .query_segment
            .replace_all(query, |m| Self::scrub_query_pair(m.full))
    }

    /// Redacts the value of a single `key=value` query segment when its key is
    /// sensitive, preserving the original key text and `=`.
    fn scrub_query_pair(segment: &str) -> String {
        let Some(equals) = segment.find('=') else {
            // Bare key with no value (`?flag`); nothing to redact.
            return segment.to_string();
        };
        let key = &segment[..equals];
        // DIVERGENCE: hand-rolled percent decoder stands in for Foundation's
        // `removingPercentEncoding` (decodes `%XX`, leaves `+` untouched, and
        // falls back to the raw key on any invalid sequence — same as Swift's
        // `?? key`). The original, still-encoded key text is emitted unchanged.
        let decoded = remove_percent_encoding(key).unwrap_or_else(|| key.to_string());
        if Self::is_sensitive_key(&decoded) {
            format!("{key}={REDACTED_SECRET}")
        } else {
            segment.to_string()
        }
    }

    /// Returns whether a dictionary/header key names a secret-bearing value.
    ///
    /// Case-insensitive, ignoring `-`/`_`/space separators.
    pub fn is_sensitive_key(key: &str) -> bool {
        let normalized: String = key
            .to_lowercase()
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .collect();
        if SENSITIVE_KEY_EXACT.contains(&normalized.as_str()) {
            return true;
        }
        SENSITIVE_KEY_MARKERS
            .iter()
            .any(|marker| normalized.contains(marker))
    }

    // MARK: - URL credentials

    /// Replaces `user:password@` URL credentials with [`REDACTED_SECRET`],
    /// preserving the `scheme://` and the host. Runs FIRST.
    fn redact_url_credentials(&self, text: &str) -> String {
        compiled().url_userinfo.replace_all(text, |m| match m.group1 {
            Some(scheme) => format!("{scheme}{REDACTED_SECRET}@"),
            None => format!("{REDACTED_SECRET}@"),
        })
    }

    // MARK: - Secrets

    /// Replaces token / secret / key / bearer / password patterns, in order.
    fn redact_secrets(&self, text: &str) -> String {
        let mut result = text.to_string();
        for pattern in &compiled().secret_patterns {
            result = pattern.replace_all(&result, |m| match m.group1 {
                // A captured prefix group (e.g. "token=") is kept; only the value
                // is redacted, so the field stays legible.
                Some(prefix) => format!("{prefix}{REDACTED_SECRET}"),
                None => REDACTED_SECRET.to_string(),
            });
        }
        result
    }

    // MARK: - Emails

    /// Replaces email addresses with [`REDACTED_EMAIL`].
    fn redact_emails(&self, text: &str) -> String {
        compiled()
            .email
            .replace_all(text, |_| REDACTED_EMAIL.to_string())
    }

    // MARK: - Paths

    /// Replaces the injected home directory and any `/Users/<name>` or
    /// `/home/<name>` prefix with a redacted-user equivalent.
    fn redact_paths(&self, text: &str) -> String {
        let mut result = text.to_string();
        if let Some(home_pattern) = &self.home_pattern {
            // Component-bounded replace (not a raw substring replace), so a home
            // dir that is a prefix of a longer username never corrupts the longer
            // path and leaks its tail.
            let redacted = Self::redacted_home_path(&self.home_directory);
            result = home_pattern.replace_all(&result, |_| redacted.clone());
        }
        let c = compiled();
        let users = format!("/Users/{REDACTED_USER}");
        result = c.user_home.replace_all(&result, |_| users.clone());
        let home = format!("/home/{REDACTED_USER}");
        result = c.linux_home.replace_all(&result, |_| home.clone());
        result
    }

    /// Returns the redacted form of an absolute home directory.
    fn redacted_home_path(home_directory: &str) -> String {
        // DIVERGENCE: Rust's `str::split('/')` keeps empty subsequences, matching
        // Swift's `split(separator:"/", omittingEmptySubsequences: false)`, so
        // "/Users/alice" → ["", "Users", "alice"] identically.
        let components: Vec<&str> = home_directory.split('/').collect();
        if components.len() >= 3 && (components[1] == "Users" || components[1] == "home") {
            format!("/{}/{REDACTED_USER}", components[1])
        } else {
            format!("/{REDACTED_USER}")
        }
    }
}

/// Decodes `%XX` percent-escapes, leaving `+` untouched. Returns `None` on any
/// malformed escape or non-UTF-8 result (caller falls back to the raw text),
/// mirroring `String.removingPercentEncoding`.
fn remove_percent_encoding(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_value(bytes[i + 1])?;
            let lo = hex_value(bytes[i + 2])?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Returns the numeric value of a single ASCII hex digit, or `None`.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
