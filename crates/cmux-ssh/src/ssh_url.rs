//! `cmux-ssh-url` — pure parser for cmux `…://ssh?…` deep links and standard
//! `ssh://` URLs.
//!
//! Port of the canonical macOS Swift source `Sources/CmuxSSHURLRequest.swift`
//! lines 1-522 (the `CmuxSSHURLParseError` enum, the `CmuxSSHURLRequest`
//! struct, its `cliArguments` / `cliPreview` / `displayTarget` accessors, and
//! the `parse` / `parseStandardSSHURL` / `standardSSHURLPort` state machine
//! plus every per-parameter validator).
//!
//! ## Intentionally omitted (out of scope for the pure core)
//! * `activeSupportedSchemes` (Swift 25-27) reads `AuthEnvironment.callbackScheme`
//!   — a host-environment lookup. [`CmuxSSHURLRequest::parse`] takes the
//!   supported schemes as an injected slice instead, matching the Swift
//!   `parse(_:supportedSchemes:)` overload the tests exercise.
//! * `CmuxNavigationURLRequest` / `CmuxTextURLRequest` (Swift 524-905) and the
//!   app-scheme link emitters (`workspaceLink`, …) are separate lanes.
//!
//! ## URL splitting — hand-rolled, not the `url` crate
//! Swift uses `URLComponents`. A conformance probe (see the `url_components_*`
//! tests below, which pull in the `url` crate as a dev-dependency) shows the
//! WHATWG-based `url` crate diverges from Foundation `URLComponents` on the
//! oracle inputs:
//!   1. `query_pairs()` applies `application/x-www-form-urlencoded` decoding, so
//!      a literal `+` becomes a space; `URLComponents.queryItems` keeps `+`.
//!   2. `query_pairs()` collapses the "no `=`" case (`no-focus`) and the "empty
//!      value" case (`no-focus=`) both to `""`; `URLComponents` reports the
//!      former as `value == nil`.
//!   3. `Url::username()` stays percent-encoded (`%20`); `URLComponents.user`
//!      is decoded (`" "`) — which then trims to blank and drops the user.
//!   4. `Url::host_str()` keeps IPv6 brackets (`[2001:db8::1]`);
//!      `URLComponents.host` is unbracketed (`2001:db8::1`).
//!
//! Because it diverges on multiple oracle inputs, the production splitter below
//! ([`ParsedUrl`]) hand-rolls a minimal scheme/authority/port/query parser that
//! matches `URLComponents` exactly. The `url` crate is a dev-dependency only,
//! used solely to pin those divergences as regression tests.

use thiserror::Error;

/// Swift: `enum CmuxSSHURLParseError` (`CmuxSSHURLRequest.swift` 3-19).
///
/// The Swift enum carries no user-facing messages (it is a bare `Error`); the
/// `#[error]` strings here are informational and are not asserted by any test.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CmuxSSHURLParseError {
    /// Swift: `.missingDestination`.
    #[error("missing ssh destination")]
    MissingDestination,
    /// Swift: `.destinationTooLong(maxLength:)`.
    #[error("ssh destination exceeds {max_length} characters")]
    DestinationTooLong { max_length: usize },
    /// Swift: `.destinationContainsUnsafeCharacters`.
    #[error("ssh destination contains unsafe characters")]
    DestinationContainsUnsafeCharacters,
    /// Swift: `.destinationStartsWithDash`.
    #[error("ssh destination starts with '-'")]
    DestinationStartsWithDash,
    /// Swift: `.titleTooLong(maxLength:)`.
    #[error("title exceeds {max_length} characters")]
    TitleTooLong { max_length: usize },
    /// Swift: `.titleContainsUnsafeCharacters`.
    #[error("title contains unsafe characters")]
    TitleContainsUnsafeCharacters,
    /// Swift: `.invalidPort`.
    #[error("invalid ssh port")]
    InvalidPort,
    /// Swift: `.invalidIntegerParameter(String)`.
    #[error("invalid integer parameter '{0}'")]
    InvalidIntegerParameter(String),
    /// Swift: `.invalidHostKeyPolicy(String)`.
    #[error("invalid host-key policy parameter '{0}'")]
    InvalidHostKeyPolicy(String),
    /// Swift: `.invalidBooleanParameter(String)`.
    #[error("invalid boolean parameter '{0}'")]
    InvalidBooleanParameter(String),
    /// Swift: `.conflictingDestinationParameters`.
    #[error("conflicting destination parameters")]
    ConflictingDestinationParameters,
    /// Swift: `.conflictingTitleParameters`.
    #[error("conflicting title parameters")]
    ConflictingTitleParameters,
    /// Swift: `.duplicateParameter(String)`.
    #[error("duplicate parameter '{0}'")]
    DuplicateParameter(String),
    /// Swift: `.unsupportedParameter(String)`.
    #[error("unsupported parameter '{0}'")]
    UnsupportedParameter(String),
    /// Swift: `.multipleLinks`. Declared in the Swift enum but never produced by
    /// the parser; retained for parity.
    #[error("multiple links")]
    MultipleLinks,
}

/// Swift: `struct CmuxSSHURLRequest: Equatable` (`CmuxSSHURLRequest.swift` 21-52).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxSSHURLRequest {
    /// Swift: `originalURL` — the verbatim input URL string. (Swift stores a
    /// `URL`; the pure port keeps the source string it parsed.)
    pub original_url: String,
    /// Swift: `destination` — `[user@]host`.
    pub destination: String,
    /// Swift: `port`.
    pub port: Option<i64>,
    /// Swift: `title`.
    pub title: Option<String>,
    /// Swift: `sshOptions` — the safe, structured `Key=value` options.
    pub ssh_options: Vec<String>,
    /// Swift: `noFocus`.
    pub no_focus: bool,
}

/// Swift: `static let maxDestinationLength = 256`.
pub const MAX_DESTINATION_LENGTH: usize = 256;
/// Swift: `static let maxTitleLength = 160`.
pub const MAX_TITLE_LENGTH: usize = 160;
/// Swift: `static let supportedSchemes` (the stable/nightly/dev product schemes).
pub const SUPPORTED_SCHEMES: [&str; 3] = ["cmux", "cmux-nightly", "cmux-dev"];

/// Allowed character set for an unbracketed IPv6-style host body, shared by
/// [`is_allowed_standard_ssh_host`] and the bracketed-inner branch of
/// [`is_allowed_ssh_host`]. The two must stay in sync — a silent divergence
/// would be a parity bug.
const IPV6_HOST_CHARS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz:.%";

impl CmuxSSHURLRequest {
    /// Swift: `var cliArguments` (36-52).
    pub fn cli_arguments(&self) -> Vec<String> {
        let mut parts = vec!["ssh".to_string()];
        if let Some(port) = self.port {
            parts.push("--port".to_string());
            parts.push(port.to_string());
        }
        if let Some(title) = self.normalized_title() {
            parts.push("--name".to_string());
            parts.push(title);
        }
        for ssh_option in &self.ssh_options {
            parts.push("--ssh-option".to_string());
            parts.push(ssh_option.clone());
        }
        if self.no_focus {
            parts.push("--no-focus".to_string());
        }
        parts.push(self.destination.clone());
        parts
    }

    /// Swift: `var cliPreview` (54-56) — `cliPreview(socketPath: nil)`.
    pub fn cli_preview(&self) -> String {
        self.cli_preview_with_socket(None)
    }

    /// Swift: `func cliPreview(socketPath:)` (58-65).
    pub fn cli_preview_with_socket(&self, socket_path: Option<&str>) -> String {
        let mut parts = vec!["cmux".to_string()];
        if let Some(socket_path) = socket_path {
            if !socket_path.is_empty() {
                parts.push("--socket".to_string());
                parts.push(socket_path.to_string());
            }
        }
        parts.extend(self.cli_arguments());
        parts
            .iter()
            .map(|p| preview_argument(p))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Swift: `var displayTarget` (67-72).
    pub fn display_target(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{}", self.destination, port),
            None => self.destination.clone(),
        }
    }

    /// Swift: `private var normalizedTitle` (74-78).
    fn normalized_title(&self) -> Option<String> {
        let title = self.title.as_ref()?;
        let trimmed = trim_whitespace_and_newlines(title);
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Swift: `static func parse(_:supportedSchemes:)` (80-201).
    ///
    /// `supported_schemes` mirrors the injected `supportedSchemes` set; each
    /// entry is expected pre-lowercased (Swift lowercases the URL scheme and
    /// checks membership).
    pub fn parse(
        url: &str,
        supported_schemes: &[&str],
    ) -> Result<Option<CmuxSSHURLRequest>, CmuxSSHURLParseError> {
        let components = ParsedUrl::parse(url);

        if is_standard_ssh_url_scheme(components.scheme.as_deref()) {
            return parse_standard_ssh_url(url, &components);
        }
        if !is_supported_scheme(components.scheme.as_deref(), supported_schemes) {
            return Ok(None);
        }
        if !ssh_target(&components) {
            return Ok(None);
        }

        // Swift: `URLComponents(url:…)` failing → `.missingDestination`. The
        // hand-rolled parser never fails structurally, so this arm is
        // unreachable for well-formed input; kept implicit.
        let query_items = &components.query_items;
        const ALLOWED_QUERY_NAMES: [&str; 10] = [
            "host",
            "user",
            "port",
            "title",
            "name",
            "connect-timeout",
            "server-alive-interval",
            "server-alive-count-max",
            "host-key-policy",
            "no-focus",
        ];
        validate_query_names(query_items, &ALLOWED_QUERY_NAMES)?;
        if contains_path_destination(&components) {
            return Err(CmuxSSHURLParseError::ConflictingDestinationParameters);
        }

        let host_value = match normalized_query_value(&["host"], query_items) {
            Some(value) => value,
            None => return Err(CmuxSSHURLParseError::MissingDestination),
        };
        if host_value.starts_with('-') {
            return Err(CmuxSSHURLParseError::DestinationStartsWithDash);
        }
        if !is_allowed_ssh_host(&host_value) {
            return Err(CmuxSSHURLParseError::DestinationContainsUnsafeCharacters);
        }

        let user_value = normalized_query_value(&["user"], query_items);
        if let Some(user_value) = &user_value {
            if user_value.starts_with('-') {
                return Err(CmuxSSHURLParseError::DestinationStartsWithDash);
            }
            if !is_allowed_ssh_user(user_value) {
                return Err(CmuxSSHURLParseError::DestinationContainsUnsafeCharacters);
            }
        }
        let destination = match &user_value {
            Some(user) => format!("{user}@{host_value}"),
            None => host_value.clone(),
        };

        if char_count(&destination) > MAX_DESTINATION_LENGTH {
            return Err(CmuxSSHURLParseError::DestinationTooLong {
                max_length: MAX_DESTINATION_LENGTH,
            });
        }

        let parsed_port = match normalized_query_value(&["port"], query_items) {
            Some(port_value) => match port_value.parse::<i64>() {
                Ok(value) if value > 0 && value <= 65535 => Some(value),
                _ => return Err(CmuxSSHURLParseError::InvalidPort),
            },
            None => None,
        };

        let title = resolve_title(query_items)?;

        let ssh_options = structured_ssh_options(query_items)?;
        let no_focus = normalized_boolean_value("no-focus", query_items)?;

        Ok(Some(CmuxSSHURLRequest {
            original_url: url.to_string(),
            destination,
            port: parsed_port,
            title,
            ssh_options,
            no_focus,
        }))
    }
}

// ---------------------------------------------------------------------------
// standard ssh:// parsing (Swift: parseStandardSSHURL / standardSSHURLPort)
// ---------------------------------------------------------------------------

/// Swift: `isStandardSSHURLScheme(_:)` (203-205).
fn is_standard_ssh_url_scheme(scheme: Option<&str>) -> bool {
    scheme.map(str::to_lowercase).as_deref() == Some("ssh")
}

/// Swift: `parseStandardSSHURL(_:)` (207-304).
fn parse_standard_ssh_url(
    url: &str,
    components: &ParsedUrl,
) -> Result<Option<CmuxSSHURLRequest>, CmuxSSHURLParseError> {
    // Swift: `components.percentEncodedPath.trimmingCharacters(in: "/")`.
    let path = trim_chars(&components.percent_encoded_path, '/');
    if !path.is_empty() {
        return Err(CmuxSSHURLParseError::ConflictingDestinationParameters);
    }
    if components.password_present {
        return Err(CmuxSSHURLParseError::UnsupportedParameter(
            "password".to_string(),
        ));
    }

    let query_items = &components.query_items;
    const ALLOWED_QUERY_NAMES: [&str; 3] = ["title", "name", "no-focus"];
    validate_query_names(query_items, &ALLOWED_QUERY_NAMES)?;

    let host_value = match &components.host {
        Some(host) if !host.is_empty() => host.clone(),
        _ => return Err(CmuxSSHURLParseError::MissingDestination),
    };
    let destination_host = unbracketed_standard_ssh_host(&host_value);
    if destination_host.starts_with('-') {
        return Err(CmuxSSHURLParseError::DestinationStartsWithDash);
    }
    if !is_allowed_standard_ssh_host(&host_value) {
        return Err(CmuxSSHURLParseError::DestinationContainsUnsafeCharacters);
    }

    let user_value = components
        .user
        .as_ref()
        .map(|u| trim_whitespace_and_newlines(u).to_string());
    if let Some(user_value) = &user_value {
        if !user_value.is_empty() {
            if user_value.starts_with('-') {
                return Err(CmuxSSHURLParseError::DestinationStartsWithDash);
            }
            if !is_allowed_ssh_user(user_value) {
                return Err(CmuxSSHURLParseError::DestinationContainsUnsafeCharacters);
            }
        }
    }
    let destination = match &user_value {
        Some(user) if !user.is_empty() => format!("{user}@{destination_host}"),
        _ => destination_host.to_string(),
    };
    if char_count(&destination) > MAX_DESTINATION_LENGTH {
        return Err(CmuxSSHURLParseError::DestinationTooLong {
            max_length: MAX_DESTINATION_LENGTH,
        });
    }

    let parsed_port = standard_ssh_url_port(components)?;

    let title = resolve_title(query_items)?;

    let no_focus = normalized_boolean_value("no-focus", query_items)?;

    Ok(Some(CmuxSSHURLRequest {
        original_url: url.to_string(),
        destination,
        port: parsed_port,
        title,
        ssh_options: Vec::new(),
        no_focus,
    }))
}

/// Swift: `standardSSHURLPort(in:)` (306-317) folded with
/// `standardSSHURLHasExplicitPort(in:)` (319-340).
///
/// Swift derives the port two ways — `components.port` for a well-formed value,
/// then a fallback string scan of the authority to detect a present-but-invalid
/// port (a trailing `:` with no/overflowing digits). [`ParsedUrl`] already
/// extracts the raw port substring from the authority, so both Swift helpers
/// collapse into one check here: the fallback exists precisely to catch the
/// `components.port == nil && ':' present` case that this single parse also
/// covers.
///
/// Foundation's `components.port` follows RFC 3986 port grammar (`*DIGIT`), so a
/// port substring containing any non-digit byte (e.g. a leading `+` or `-`)
/// makes `components.port == nil`; Swift then hits `standardSSHURLHasExplicitPort`
/// (which only checks that a `:` is present in the authority) and returns
/// `.invalidPort`. Rust's `i64::from_str` is *more* permissive — it accepts an
/// optional leading sign, so `"+22".parse::<i64>()` is `Ok(22)`. We therefore
/// reject any non-digit port substring up front to match Foundation exactly
/// (Swift: `CmuxSSHURLRequest.swift` 306-340).
fn standard_ssh_url_port(components: &ParsedUrl) -> Result<Option<i64>, CmuxSSHURLParseError> {
    match &components.port_section {
        None => Ok(None),
        // A non-empty port substring with a non-digit byte can never yield a
        // non-nil `components.port` in Foundation, so it is an explicit-but-
        // invalid port. (Empty stays empty here and fails `parse` below, which
        // is also `.invalidPort` — matching the Swift fallback for a bare `:`.)
        Some(port_str) if !port_str.is_empty() && !port_str.bytes().all(|b| b.is_ascii_digit()) => {
            Err(CmuxSSHURLParseError::InvalidPort)
        }
        Some(port_str) => match port_str.parse::<i64>() {
            Ok(value) if value > 0 && value <= 65535 => Ok(Some(value)),
            _ => Err(CmuxSSHURLParseError::InvalidPort),
        },
    }
}

/// Swift: `unbracketedStandardSSHHost(_:)` (342-347).
fn unbracketed_standard_ssh_host(host: &str) -> &str {
    if host.starts_with('[') && host.ends_with(']') && host.len() >= 2 {
        &host[1..host.len() - 1]
    } else {
        host
    }
}

/// Swift: `isAllowedStandardSSHHost(_:)` (349-361).
fn is_allowed_standard_ssh_host(value: &str) -> bool {
    if is_allowed_ssh_host(value) {
        return true;
    }
    if contains_unsafe_hidden_character(value)
        || !value.contains(':')
        || value.starts_with('[')
        || value.ends_with(']')
    {
        return false;
    }
    value.chars().all(|c| IPV6_HOST_CHARS.contains(c))
}

// ---------------------------------------------------------------------------
// scheme / target predicates (Swift: isSupportedScheme / sshTarget / …)
// ---------------------------------------------------------------------------

/// Swift: `isSupportedScheme(_:supportedSchemes:)` (363-366).
fn is_supported_scheme(scheme: Option<&str>, supported_schemes: &[&str]) -> bool {
    match scheme {
        Some(scheme) => {
            let lowered = scheme.to_lowercase();
            supported_schemes.contains(&lowered.as_str())
        }
        None => false,
    }
}

/// Swift: `sshTarget(from:)` (368-379).
fn ssh_target(components: &ParsedUrl) -> bool {
    if let Some(host) = &components.host {
        let host = trim_chars(host, '/').to_lowercase();
        if !host.is_empty() {
            return host == "ssh";
        }
    }
    let path = components.decoded_path();
    let first = path
        .split('/')
        .find(|s| !s.is_empty())
        .map(|s| s.to_lowercase());
    first.as_deref() == Some("ssh")
}

/// Swift: `containsPathDestination(_:)` (381-389).
fn contains_path_destination(components: &ParsedUrl) -> bool {
    if let Some(host) = &components.host {
        if host.to_lowercase() == "ssh" {
            return !trim_chars(&components.decoded_path(), '/').is_empty();
        }
    }
    let path = components.decoded_path();
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    segments
        .next()
        .map(|s| s.to_lowercase() == "ssh")
        .unwrap_or(false)
        && segments.next().is_some()
}

// ---------------------------------------------------------------------------
// query value accessors (Swift: normalizedQueryValue / structuredSSHOptions / …)
// ---------------------------------------------------------------------------

/// Shared query-name allow-list + duplicate check used by both
/// [`CmuxSSHURLRequest::parse`] and [`parse_standard_ssh_url`]. Iterates the
/// query items in order, lowercasing each name, rejecting the first name not in
/// `allowed` with `UnsupportedParameter` and the first repeat with
/// `DuplicateParameter` (both carry `display_parameter_name(item.name)`).
fn validate_query_names(
    query_items: &[QueryItem],
    allowed: &[&str],
) -> Result<(), CmuxSSHURLParseError> {
    let mut seen_query_names: Vec<String> = Vec::new();
    for item in query_items {
        let name = item.name.to_lowercase();
        if !allowed.contains(&name.as_str()) {
            return Err(CmuxSSHURLParseError::UnsupportedParameter(
                display_parameter_name(&item.name),
            ));
        }
        if seen_query_names.contains(&name) {
            return Err(CmuxSSHURLParseError::DuplicateParameter(
                display_parameter_name(&item.name),
            ));
        }
        seen_query_names.push(name);
    }
    Ok(())
}

/// Shared `title`/`name` resolution + validation used by both
/// [`CmuxSSHURLRequest::parse`] and [`parse_standard_ssh_url`]. Rejects both
/// present with `ConflictingTitleParameters`, prefers `title` over `name`, then
/// enforces the length (`TitleTooLong`) and hidden-character
/// (`TitleContainsUnsafeCharacters`) guards.
fn resolve_title(query_items: &[QueryItem]) -> Result<Option<String>, CmuxSSHURLParseError> {
    let title_value = normalized_query_value(&["title"], query_items);
    let name_value = normalized_query_value(&["name"], query_items);
    if title_value.is_some() && name_value.is_some() {
        return Err(CmuxSSHURLParseError::ConflictingTitleParameters);
    }
    let title = title_value.or(name_value);
    if let Some(title) = &title {
        if char_count(title) > MAX_TITLE_LENGTH {
            return Err(CmuxSSHURLParseError::TitleTooLong {
                max_length: MAX_TITLE_LENGTH,
            });
        }
        if contains_unsafe_hidden_character(title) {
            return Err(CmuxSSHURLParseError::TitleContainsUnsafeCharacters);
        }
    }
    Ok(title)
}

/// Swift: `normalizedQueryValue(namedAnyOf:in:)` (391-397).
fn normalized_query_value(names: &[&str], query_items: &[QueryItem]) -> Option<String> {
    let value = query_items
        .iter()
        .find(|item| names.contains(&item.name.to_lowercase().as_str()))?
        .value
        .as_ref()?;
    let normalized = trim_whitespace_and_newlines(value);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

/// Swift: `structuredSSHOptions(from:)` (399-438).
fn structured_ssh_options(query_items: &[QueryItem]) -> Result<Vec<String>, CmuxSSHURLParseError> {
    let mut options: Vec<String> = Vec::new();
    if let Some(value) = normalized_query_value(&["connect-timeout"], query_items) {
        let seconds = bounded_integer(&value, "connect-timeout", 1, 600)?;
        options.push(format!("ConnectTimeout={seconds}"));
    }
    if let Some(value) = normalized_query_value(&["server-alive-interval"], query_items) {
        let seconds = bounded_integer(&value, "server-alive-interval", 1, 3600)?;
        options.push(format!("ServerAliveInterval={seconds}"));
    }
    if let Some(value) = normalized_query_value(&["server-alive-count-max"], query_items) {
        let count = bounded_integer(&value, "server-alive-count-max", 1, 100)?;
        options.push(format!("ServerAliveCountMax={count}"));
    }
    if let Some(value) = normalized_query_value(&["host-key-policy"], query_items) {
        match value.to_lowercase().as_str() {
            "accept-new" => options.push("StrictHostKeyChecking=accept-new".to_string()),
            "ask" => options.push("StrictHostKeyChecking=ask".to_string()),
            "strict" | "yes" => options.push("StrictHostKeyChecking=yes".to_string()),
            _ => {
                return Err(CmuxSSHURLParseError::InvalidHostKeyPolicy(
                    "host-key-policy".to_string(),
                ))
            }
        }
    }
    Ok(options)
}

/// Swift: `boundedInteger(_:parameter:range:)` (440-448).
fn bounded_integer(
    value: &str,
    parameter: &str,
    lower: i64,
    upper: i64,
) -> Result<i64, CmuxSSHURLParseError> {
    // Swift: `!containsUnsafeHiddenCharacter && matches ^[0-9]+$ && Int(value)
    // && range.contains`. The value arrives already trimmed by
    // `normalizedQueryValue`, so an all-ASCII-digit test is equivalent to the
    // `^[0-9]+$` regex.
    let is_all_digits = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    if contains_unsafe_hidden_character(value) || !is_all_digits {
        return Err(CmuxSSHURLParseError::InvalidIntegerParameter(
            parameter.to_string(),
        ));
    }
    match value.parse::<i64>() {
        Ok(integer) if integer >= lower && integer <= upper => Ok(integer),
        _ => Err(CmuxSSHURLParseError::InvalidIntegerParameter(
            parameter.to_string(),
        )),
    }
}

/// Swift: `normalizedBooleanValue(named:in:)` (450-469).
fn normalized_boolean_value(
    name: &str,
    query_items: &[QueryItem],
) -> Result<bool, CmuxSSHURLParseError> {
    let item = match query_items
        .iter()
        .find(|item| item.name.to_lowercase() == name)
    {
        Some(item) => item,
        None => return Ok(false),
    };
    let raw_value = match &item.value {
        Some(value) => value,
        None => return Ok(true),
    };
    let normalized = trim_whitespace_and_newlines(raw_value).to_lowercase();
    if normalized.is_empty() {
        return Ok(true);
    }
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(CmuxSSHURLParseError::InvalidBooleanParameter(
            display_parameter_name(&item.name),
        )),
    }
}

// ---------------------------------------------------------------------------
// charset / unicode validators (Swift: isAllowedSSHHost / … )
// ---------------------------------------------------------------------------

/// Swift: `isAllowedSSHHost(_:)` (471-482).
fn is_allowed_ssh_host(value: &str) -> bool {
    if contains_unsafe_hidden_character(value) {
        return false;
    }
    if value.starts_with('[') || value.ends_with(']') {
        if !(value.starts_with('[') && value.ends_with(']')) {
            return false;
        }
        // '[' and ']' are single-byte ASCII → slice is on a char boundary.
        let inner = &value[1..value.len() - 1];
        if inner.is_empty() {
            return false;
        }
        return inner.chars().all(|c| IPV6_HOST_CHARS.contains(c));
    }
    const ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._%-";
    value.chars().all(|c| ALLOWED.contains(c))
}

/// Swift: `isAllowedSSHUser(_:)` (484-488).
fn is_allowed_ssh_user(value: &str) -> bool {
    if contains_unsafe_hidden_character(value) {
        return false;
    }
    const ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._%+=,:-";
    value.chars().all(|c| ALLOWED.contains(c))
}

/// Swift: `containsUnsafeHiddenCharacter(_:)` (490-499). Any Unicode scalar
/// whose General_Category is control (Cc), format (Cf), line separator (Zl),
/// or paragraph separator (Zp).
fn contains_unsafe_hidden_character(value: &str) -> bool {
    value.chars().any(is_unsafe_hidden_scalar)
}

/// Swift: `previewArgument(_:)` (501-509).
fn preview_argument(value: &str) -> String {
    // Swift regex: `[^A-Za-z0-9_./:=+@%\-\[\]]` — quote iff any char is outside
    // this set.
    if value.chars().all(is_preview_safe_char) {
        return value.to_string();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn is_preview_safe_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_./:=+@%-[]".contains(c)
}

/// Swift: `displayParameterName(_:)` (511-521).
fn display_parameter_name(name: &str) -> String {
    if name.is_empty() || contains_unsafe_hidden_character(name) {
        return "?".to_string();
    }
    const ALLOWED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-";
    if !name.chars().all(|c| ALLOWED.contains(c)) {
        return "?".to_string();
    }
    // Swift: `name.prefix(64)`. DIVERGENCE: scalar prefix, not grapheme prefix
    // (matches this crate's established scalar convention); only differs for
    // names > 64 scalars containing grapheme clusters.
    if char_count(name) <= 64 {
        name.to_string()
    } else {
        let prefix: String = name.chars().take(64).collect();
        format!("{prefix}...")
    }
}

// ---------------------------------------------------------------------------
// Unicode / string helpers
// ---------------------------------------------------------------------------

/// True if `c`'s General_Category is Cc, Cf, Zl, or Zp — the categories Swift's
/// `containsUnsafeHiddenCharacter` rejects.
///
/// Cc, Zl (only U+2028), and Zp (only U+2029) are fixed across all Unicode
/// versions. The Cf set below tracks Unicode 15.1. DIVERGENCE NOTE: the host
/// macOS classifies via its bundled ICU, whose Unicode version may add/remove a
/// handful of Cf code points at the margins; a mismatch can only affect exotic
/// format characters never present in real ssh destinations.
fn is_unsafe_hidden_scalar(c: char) -> bool {
    // Cc — Rust `is_control()` is exactly General_Category=Cc.
    c.is_control()
        // Zl / Zp — singletons.
        || c == '\u{2028}'
        || c == '\u{2029}'
        // Cf — format characters.
        || is_format_scalar(c)
}

/// Unicode General_Category=Cf (format), Unicode 15.1.
fn is_format_scalar(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// Swift `String.count` — DIVERGENCE: scalar count, not grapheme-cluster count
/// (this crate's established convention; see the note in `lib.rs`). Only differs
/// for destinations/titles containing combining marks, which are unreachable
/// through the allowed charsets anyway.
fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// Swift `trimmingCharacters(in: .whitespacesAndNewlines)`. The Swift
/// `whitespacesAndNewlines` set (Zs ∪ {U+0009} ∪ {U+000A–000D, U+0085, U+2028,
/// U+2029}) is exactly Rust's `char::is_whitespace` (the Unicode White_Space
/// property).
fn trim_whitespace_and_newlines(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace())
}

/// Swift `trimmingCharacters(in: CharacterSet(charactersIn: "…"))` for a single
/// trim character.
fn trim_chars(s: &str, ch: char) -> &str {
    s.trim_matches(ch)
}

/// Percent-decode `%XX` escapes (Foundation `.removingPercentEncoding` /
/// `URLComponents` decoded getters). A literal `+` is preserved (NOT converted
/// to space — that is `application/x-www-form-urlencoded`, which
/// `URLComponents` does not apply). Invalid `%` escapes are left verbatim
/// (Swift returns nil on failure; the pure port is lenient — no oracle input
/// exercises invalid escapes).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

// ---------------------------------------------------------------------------
// ParsedUrl — hand-rolled URLComponents-equivalent splitter
// ---------------------------------------------------------------------------

/// One `URLComponents.queryItems` entry: decoded name + decoded value
/// (`value == None` mirrors the Swift `URLQueryItem.value == nil` case, i.e. a
/// bare `name` with no `=`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryItem {
    name: String,
    value: Option<String>,
}

/// Minimal scheme/authority/port/query split matching Foundation
/// `URLComponents` on the oracle inputs. See the module header for why the
/// `url` crate is not used.
#[derive(Debug, Clone)]
struct ParsedUrl {
    /// Raw scheme (case preserved), or `None` when the string has no scheme.
    scheme: Option<String>,
    /// Decoded authority host (`URLComponents.host` / `URL.host`); IPv6 literals
    /// are unbracketed to match Foundation.
    host: Option<String>,
    /// Decoded userinfo user (`URLComponents.user`).
    user: Option<String>,
    /// Whether the userinfo carried a `:` password segment
    /// (`URLComponents.password != nil`).
    password_present: bool,
    /// Raw port substring from the authority (digits between `:` and the
    /// authority end), if a `:port` section was present.
    port_section: Option<String>,
    /// Raw (still percent-encoded) path (`URLComponents.percentEncodedPath`).
    percent_encoded_path: String,
    /// Decoded query items in order (`URLComponents.queryItems ?? []`).
    query_items: Vec<QueryItem>,
}

impl ParsedUrl {
    /// Decoded path (`URL.path`).
    fn decoded_path(&self) -> String {
        percent_decode(&self.percent_encoded_path)
    }

    fn parse(input: &str) -> ParsedUrl {
        // scheme = chars up to the first ':' (RFC 3986 scheme: ALPHA first, but
        // the tests only feed well-formed schemes; be lenient and split on ':').
        let (scheme, rest) = match split_scheme(input) {
            Some((scheme, rest)) => (Some(scheme.to_string()), rest),
            None => (None, input),
        };

        // Authority is present iff the remainder begins with "//".
        let (authority, after_authority) = if let Some(after) = rest.strip_prefix("//") {
            let end = after.find(['/', '?', '#']).unwrap_or(after.len());
            (Some(&after[..end]), &after[end..])
        } else {
            (None, rest)
        };

        // Split path / query / fragment out of the post-authority remainder.
        let without_fragment = match after_authority.find('#') {
            Some(idx) => &after_authority[..idx],
            None => after_authority,
        };
        let (path_part, query_part) = match without_fragment.find('?') {
            Some(idx) => (&without_fragment[..idx], Some(&without_fragment[idx + 1..])),
            None => (without_fragment, None),
        };

        let mut parsed = ParsedUrl {
            scheme,
            host: None,
            user: None,
            password_present: false,
            port_section: None,
            percent_encoded_path: path_part.to_string(),
            query_items: parse_query_items(query_part),
        };
        if let Some(authority) = authority {
            parsed.apply_authority(authority);
        }
        parsed
    }

    fn apply_authority(&mut self, authority: &str) {
        // userinfo is terminated by the first '@' (RFC 3986). The oracle inputs
        // carry at most one '@'.
        let (userinfo, hostport) = match authority.find('@') {
            Some(idx) => (Some(&authority[..idx]), &authority[idx + 1..]),
            None => (None, authority),
        };

        if let Some(userinfo) = userinfo {
            match userinfo.find(':') {
                Some(idx) => {
                    self.user = Some(percent_decode(&userinfo[..idx]));
                    self.password_present = true;
                }
                None => {
                    self.user = Some(percent_decode(userinfo));
                }
            }
        }

        // host [":" port]; bracketed IPv6 literal keeps its inner colons.
        let (host_raw, port_section) = if let Some(rest) = hostport.strip_prefix('[') {
            match rest.find(']') {
                Some(close) => {
                    let inner = &rest[..close];
                    let after = &rest[close + 1..];
                    let port = after.strip_prefix(':').map(|p| p.to_string());
                    (inner.to_string(), port)
                }
                None => (hostport.to_string(), None),
            }
        } else {
            match hostport.rfind(':') {
                Some(idx) => (
                    hostport[..idx].to_string(),
                    Some(hostport[idx + 1..].to_string()),
                ),
                None => (hostport.to_string(), None),
            }
        };

        self.host = Some(percent_decode(&host_raw));
        self.port_section = port_section;
    }
}

/// Split the leading `scheme:` (scheme = up to the first ':', requiring an ALPHA
/// first char so an authority-less path with a ':' is not misread as a scheme).
fn split_scheme(input: &str) -> Option<(&str, &str)> {
    let idx = input.find(':')?;
    let scheme = &input[..idx];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return None;
    }
    Some((scheme, &input[idx + 1..]))
}

/// Swift `URLComponents.queryItems`: split on `&`, then on the first `=`; a
/// component with no `=` yields `value == None`; both name and value are
/// percent-decoded (with `+` preserved).
fn parse_query_items(query: Option<&str>) -> Vec<QueryItem> {
    let query = match query {
        Some(q) => q,
        None => return Vec::new(),
    };
    // Swift returns `queryItems == nil` (→ `[]`) only when there is no `?` at
    // all. A present-but-empty query (`?`) yields a single empty item in
    // Foundation, but no oracle input reaches that shape; an empty string here
    // produces one `{ name: "", value: None }` item, matching Foundation.
    query
        .split('&')
        .map(|pair| match pair.find('=') {
            Some(idx) => QueryItem {
                name: percent_decode(&pair[..idx]),
                value: Some(percent_decode(&pair[idx + 1..])),
            },
            None => QueryItem {
                name: percent_decode(pair),
                value: None,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests;
