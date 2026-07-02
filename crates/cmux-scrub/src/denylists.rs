//! The maintained denylists `SentryScrubber` consumes: sensitive dictionary key
//! markers (ported from sentry-python) and free-text value regexes (ported from
//! Sentry's relay `@common` scrubbers, plus cmux's own provider-key / JWT / AWS
//! additions that exceed relay's set).
//!
//! Faithful port of `ScrubberDenylists.swift`. The lists are data-only; their
//! provenance is pinned in the Swift source's per-entry comments and preserved
//! here in spirit. Ordering is load-bearing for `VALUE_PATTERNS` (they are
//! applied sequentially in `SentryScrubber::redact_secrets`).

/// Substrings that mark a dictionary/header key as secret-bearing.
///
/// `SentryScrubber::is_sensitive_key` normalizes a key (lowercase, strip
/// `-`/`_`/space) and redacts wholesale if it *contains* any marker here, so a
/// marker also catches longer identifiers (`AWS_SECRET_ACCESS_KEY` → `secret`,
/// `MY_API_KEY` → `apikey`, `sessionid` → `session`, `csrftoken` → `token`,
/// `set_cookie` → `cookie`).
///
/// Ported from sentry-python `DEFAULT_DENYLIST`
/// (getsentry/sentry-python @ 9e54e149a095d15e90f664b9e2ef35796f37e83b,
/// sentry_sdk/scrubber.py:15-53) plus cmux's own `accesskey` / `bearer`.
pub(crate) const SENSITIVE_KEY_MARKERS: &[&str] = &[
    // High-value credential markers (sentry-python DEFAULT_DENYLIST core).
    "password",
    "passwd",
    "secret",
    "apikey",
    "auth",
    "credential",
    "privatekey",
    "token",
    "session",
    "cookie",
    "authorization",
    // cmux-original markers retained.
    "accesskey",
    "bearer",
];

/// Short or marker-free credential key aliases matched WHOLE (not as
/// substrings), so they don't redact innocuous keys that merely contain them
/// (e.g. `sid` must not match `inside`/`aside`; `su` must not match
/// `issue`/`consumer`).
///
/// These are stored **already normalized** the same way
/// `SentryScrubber::is_sensitive_key` normalizes (lowercase, strip `-`/`_`/space)
/// because the set is consulted *after* normalization.
///
/// Ported from sentry-python `DEFAULT_DENYLIST` + `DEFAULT_PII_DENYLIST`
/// (scrubber.py:39-60) and relay `SENSITIVE_COOKIES`
/// (getsentry/relay @ 99c91d92845fe436713b51018a7f8d2b7b469be5,
/// relay-pii/src/convert.rs:30-56).
///
// DIVERGENCE: Swift stores this as a `Set<String>`; Rust uses a slice queried by
// linear scan in `is_sensitive_key`. The set is tiny (19 entries) so membership
// cost is irrelevant and behavior is identical (whole-string equality).
pub(crate) const SENSITIVE_KEY_EXACT: &[&str] = &[
    // sentry-python framework/session/CSRF aliases (normalized).
    "sid",
    "connect.sid",
    "csrf",
    "csrftoken",
    "xsrf",
    "phpsessid",
    "symfony",
    "mysqlpwd",
    // sentry-python DEFAULT_PII_DENYLIST (cmux sends sendDefaultPii = false).
    "xforwardedfor",
    "xrealip",
    "ipaddress",
    "remoteaddr",
    // relay SENSITIVE_COOKIES aliases not implied by a substring marker.
    "sentrysid",
    "su",
    "fasthttpsessionid",
    "irissessionid",
    "verceljwt",
    "fastcsrf",
    "iriscsrf",
];

/// A free-text value regex plus whether it is applied case-insensitively.
///
/// The Swift `SentryRegexPattern` defaults to `.caseInsensitive`; the JWT / AWS /
/// credit-card / IBAN / SSN rules opt out with `options: []` (case-sensitive on
/// purpose). `case_insensitive` mirrors that per-pattern choice.
pub(crate) struct ValuePattern {
    pub source: &'static str,
    pub case_insensitive: bool,
}

/// The ordered secret / token / key / PEM / financial value patterns applied to
/// every free-text string, in priority order. Ported verbatim from
/// `ScrubberDenylists.valuePatterns`; the ORDER is load-bearing.
pub(crate) const VALUE_PATTERNS: &[ValuePattern] = &[
    // Bearer <token>. Keeps the `Bearer ` prefix legible (group 1).
    ValuePattern {
        source: r"(Bearer\s+)[A-Za-z0-9\-._~+/]+=*",
        case_insensitive: true,
    },
    // Authorization: <scheme> <token>.
    ValuePattern {
        source: r"(Authorization:\s*\w+\s+)\S+",
        case_insensitive: true,
    },
    // `<sensitive-key> = value` — QUOTED-value form: redact through the closing
    // quote (so a delimiter inside a quoted JSON value doesn't leak the tail).
    ValuePattern {
        source: r#"([A-Za-z0-9.\-]*(?:access[_\-]?token|api[_\-]?key|access[_\-]?key|private[_\-]?key|session[_\-]?id|session|secret|token|password|passwd|pwd|credentials?|cookie|bearer|auth)[A-Za-z0-9.\-]*["']?\s*[:=]\s*["'])[^"']*"#,
        case_insensitive: true,
    },
    // Bare `sid` session alias — quoted-value form.
    ValuePattern {
        source: r#"(\bsid["']?\s*[:=]\s*["'])[^"']*"#,
        case_insensitive: true,
    },
    // `<sensitive-key> = value` — UNQUOTED-value form: delimiter-bounded value.
    ValuePattern {
        source: r#"([A-Za-z0-9.\-]*(?:access[_\-]?token|api[_\-]?key|access[_\-]?key|private[_\-]?key|session[_\-]?id|session|secret|token|password|passwd|pwd|credentials?|cookie|bearer|auth)[A-Za-z0-9.\-]*["']?\s*[:=]\s*)[^\s"'&,}]+"#,
        case_insensitive: true,
    },
    // Bare `sid` alias — unquoted-value form.
    ValuePattern {
        source: r#"(\bsid["']?\s*[:=]\s*)[^\s"'&,}]+"#,
        case_insensitive: true,
    },
    // Provider-style keys: sk-..., pk-..., ghp_..., xoxb-..., etc.
    ValuePattern {
        source: r"\b(?:sk|pk|rk|ghp|gho|ghu|ghs|ghr|xox[baprs])[_\-][A-Za-z0-9_\-]{16,}",
        case_insensitive: true,
    },
    // JSON Web Tokens. Case-sensitive (`eyJ` header is fixed-case base64).
    ValuePattern {
        source: r"\beyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+",
        case_insensitive: false,
    },
    // AWS access key IDs. Case-sensitive (AKIA/ASIA + uppercase).
    ValuePattern {
        source: r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
        case_insensitive: false,
    },
    // @pemkey — whole PEM block (header, body, footer) redacted (no capture group).
    ValuePattern {
        source: r"-----BEGIN[A-Z ]+(?:PRIVATE|PUBLIC) KEY-----[\s\S]+?-----END[A-Z ]+(?:PRIVATE|PUBLIC) KEY-----",
        case_insensitive: true,
    },
    // @creditcard — variable-length card number. Case-sensitive.
    ValuePattern {
        source: r"\b(?:3[47]\d|4\d{3}|5[1-5]\d\d|65\d\d|6011)(?:[-\s]?\d){12}\b",
        case_insensitive: false,
    },
    // @iban — country-prefix IBAN. Case-sensitive.
    ValuePattern {
        source: r"\b(?:AT|AD|AE|AL|AZ|BA|BE|BG|BH|BR|BY|CH|CR|CY|CZ|DE|DK|DO|EE|EG|ES|FI|FO|FR|GB|GE|GI|GL|GR|GT|HR|HU|IE|IL|IQ|IS|IT|JO|KW|KZ|LB|LC|LI|LT|LU|LV|LY|MC|MD|ME|MK|MR|MT|MU|NL|NO|PK|PL|PS|PT|QA|RO|RU|RS|SA|SC|SE|SI|SK|SM|ST|SV|TL|TN|TR|UA|VA|VG|XK|DZ|AO|BJ|BF|BI|CV|CM|CF|TD|KM|CG|CI|DJ|GQ|GA|GW|HN|IR|MG|ML|MA|MZ|NI|NE|SN|TG)\d{2}[A-Za-z0-9]{11,29}\b",
        case_insensitive: false,
    },
    // @usssn — US Social Security number `NNN-NN-NNNN`. Case-sensitive.
    ValuePattern {
        source: r"\b[0-9]{3}-[0-9]{2}-[0-9]{4}\b",
        case_insensitive: false,
    },
];
