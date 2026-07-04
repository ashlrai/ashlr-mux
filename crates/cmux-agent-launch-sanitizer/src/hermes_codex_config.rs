//! Config / base-URL half of the canonical macOS Swift enum
//! `HermesAgentCodexEnvironment` (`HermesAgentCodexEnvironment.swift`).
//!
//! This is the pure value core that the Rust
//! `WorkspaceHermesCodexEnvironment` (in `cmux-workspaces`,
//! `session_restore_policy.rs:127-139`) deliberately holds as injected
//! closures whose concrete bodies were left in the app target. It supplies
//! those bodies:
//!
//! - [`applying_default_codex_base_url`] backs the
//!   `applying_default_codex_base_url` closure seam,
//! - [`codex_model_from_codex_config_content`] backs the
//!   `resolving_default_codex_model` closure seam.
//!
//! The provider-argument rewrite half of the Swift enum
//! (`argumentsByReplacingOpenAICodexProvider`) already lives at the crate
//! root ([`crate::arguments_by_replacing_openai_codex_provider`], via
//! `preserved_arguments("hermes-agent", …)`).
//!
//! # Injected I/O seam
//!
//! The single non-pure Swift member — `codexConfigContent(environment:
//! ambientEnvironment:)` reading `~/.codex/config.toml` (via
//! `codexConfigPath`, `NSString.expandingTildeInPath`, and
//! `String(contentsOfFile:)`) — is **not** ported here. Instead
//! [`applying_default_codex_base_url`] is reshaped to take the already-read
//! config *content* (`Option<&str>`, `None` == the file was absent/unreadable,
//! mirroring Swift's `guard let configContent … else { return environment }`).
//! The caller keeps the `codexConfigPath` / `codexConfigContent` filesystem
//! shell.
//!
//! # Sanctioned divergences from the Swift source
//!
//! 1. **URL parsing.** Swift's `normalizedHTTPComponents` uses Foundation
//!    `URLComponents(string:)` and its `.string` reassembly. Rust std has no
//!    URL type, so [`normalized_http_components`] hand-rolls a focused
//!    RFC-3986 `scheme "://" [userinfo "@"] host [":" port] path` parser plus
//!    query/fragment split. It reproduces every field the Swift guards inspect
//!    (`scheme`, `host`, `query == nil`, `fragment == nil`, `path`) and the
//!    `.string` reassembly for `http`/`https` base URLs — the only shape this
//!    code ever sees. It approximates (does not exhaustively replicate)
//!    `URLComponents`' rejection of every malformed-character input: a space or
//!    ASCII control char anywhere is rejected (the common `URLComponents` nil
//!    case), but exotic RFC-3986 delimiter validation is not chased because
//!    Codex config base URLs never carry it. Ports of the two trailing-slash
//!    `replacingOccurrences(of: "/+$", options: .regularExpression)` calls use
//!    [`str::trim_end_matches`] (`/+$` == "strip all trailing '/'").
//! 2. **`.whitespaces` vs `.whitespacesAndNewlines`.** Swift trims TOML lines
//!    and key/value spans with `CharacterSet.whitespaces` (horizontal
//!    whitespace only — a trailing `\r` from a CRLF file is intentionally
//!    *kept*), and trims URL/env values and the host with
//!    `.whitespacesAndNewlines`. [`trim_toml_ws`] mirrors the former exactly
//!    (`char::is_whitespace` minus the newline scalars `\n \u{0B} \u{0C} \r
//!    \u{85} \u{2028} \u{2029}`); [`normalized_value`] mirrors the latter via
//!    [`str::trim`] (Unicode `White_Space`).

use std::collections::HashMap;

/// Swift `HermesAgentCodexEnvironment.defaultProvider`
/// (`HermesAgentCodexEnvironment.swift:6`).
pub const DEFAULT_PROVIDER: &str = "custom";

/// Swift `HermesAgentCodexEnvironment.codexResponsesAPIMode`
/// (`HermesAgentCodexEnvironment.swift:8`).
pub const CODEX_RESPONSES_API_MODE: &str = "codex_responses";

/// Swift `HermesAgentCodexEnvironment.codexBaseURLEnvironmentKey`
/// (`HermesAgentCodexEnvironment.swift:10`).
pub const CODEX_BASE_URL_ENVIRONMENT_KEY: &str = "HERMES_CODEX_BASE_URL";

/// Swift `HermesAgentCodexEnvironment.customBaseURLEnvironmentKey`
/// (`HermesAgentCodexEnvironment.swift:12`).
pub const CUSTOM_BASE_URL_ENVIRONMENT_KEY: &str = "CUSTOM_BASE_URL";

// ---------------------------------------------------------------------------
// Environment application (the injected-closure bodies).
// ---------------------------------------------------------------------------

/// `HermesAgentCodexEnvironment.applyingDefaultCodexBaseURL(to:ambientEnvironment:)`
/// (`HermesAgentCodexEnvironment.swift:63-83`), reshaped to take the already-read
/// Codex config `content` in place of the private `codexConfigContent` file read.
///
/// `config_content == None` mirrors Swift's `guard let configContent … else {
/// return environment }` (config absent/unreadable ⇒ environment returned
/// untouched). Each default key is only filled when the incoming value is
/// absent-or-blank ([`normalized_value`] `== None`), so an explicit Hermes URL
/// is never overridden.
#[must_use]
pub fn applying_default_codex_base_url(
    environment: HashMap<String, String>,
    config_content: Option<&str>,
) -> HashMap<String, String> {
    let Some(content) = config_content else {
        return environment;
    };
    let mut result = environment;
    if normalized_value(result.get(CODEX_BASE_URL_ENVIRONMENT_KEY).map(String::as_str)).is_none() {
        if let Some(codex_base_url) = codex_base_url_from_codex_config_content(content) {
            result.insert(CODEX_BASE_URL_ENVIRONMENT_KEY.to_owned(), codex_base_url);
        }
    }
    if normalized_value(result.get(CUSTOM_BASE_URL_ENVIRONMENT_KEY).map(String::as_str)).is_none() {
        if let Some(custom_base_url) = custom_base_url_from_codex_config_content(content) {
            result.insert(CUSTOM_BASE_URL_ENVIRONMENT_KEY.to_owned(), custom_base_url);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Config-content parsers.
// ---------------------------------------------------------------------------

/// `HermesAgentCodexEnvironment.codexBaseURL(fromCodexConfigContent:)`
/// (`HermesAgentCodexEnvironment.swift:122-134`).
///
/// Scans the top-level (pre-`[table]`) TOML lines for `chatgpt_base_url` and
/// maps it through [`codex_base_url_from_chatgpt_base_url`]. A `[` line ends the
/// scan with `None`.
#[must_use]
pub fn codex_base_url_from_codex_config_content(content: &str) -> Option<String> {
    for raw_line in content.split('\n') {
        let line = trim_toml_ws(raw_line);
        if line.starts_with('[') {
            return None;
        }
        let Some(value) = toml_string_value("chatgpt_base_url", line) else {
            continue;
        };
        return codex_base_url_from_chatgpt_base_url(&value);
    }
    None
}

/// `HermesAgentCodexEnvironment.customBaseURL(fromCodexConfigContent:)`
/// (`HermesAgentCodexEnvironment.swift:137-154`).
///
/// A top-level `openai_base_url` wins immediately; a top-level
/// `chatgpt_base_url` is retained only as a fallback. A `[` line breaks the
/// scan and yields the retained fallback (if any).
#[must_use]
pub fn custom_base_url_from_codex_config_content(content: &str) -> Option<String> {
    let mut fallback_chatgpt_base_url: Option<String> = None;
    for raw_line in content.split('\n') {
        let line = trim_toml_ws(raw_line);
        if line.starts_with('[') {
            break;
        }
        if let Some(value) = toml_string_value("openai_base_url", line) {
            if let Some(base_url) = custom_base_url_from_openai_base_url(&value) {
                return Some(base_url);
            }
        }
        if let Some(value) = toml_string_value("chatgpt_base_url", line) {
            if let Some(base_url) = custom_base_url_from_chatgpt_base_url(&value) {
                fallback_chatgpt_base_url = Some(base_url);
            }
        }
    }
    fallback_chatgpt_base_url
}

/// `HermesAgentCodexEnvironment.codexModel(fromCodexConfigContent:)`
/// (`HermesAgentCodexEnvironment.swift:157-169`).
///
/// Reads the top-level `model` key. A `[` line ends the scan with `None`.
#[must_use]
pub fn codex_model_from_codex_config_content(content: &str) -> Option<String> {
    for raw_line in content.split('\n') {
        let line = trim_toml_ws(raw_line);
        if line.starts_with('[') {
            return None;
        }
        let Some(value) = toml_string_value("model", line) else {
            continue;
        };
        return normalized_value(Some(&value));
    }
    None
}

// ---------------------------------------------------------------------------
// Base-URL derivations.
// ---------------------------------------------------------------------------

/// `HermesAgentCodexEnvironment.codexBaseURL(fromChatGPTBaseURL:)`
/// (`HermesAgentCodexEnvironment.swift:172-181`).
///
/// Appends `/codex` to the (trailing-slash-normalized) path unless it already
/// ends in `/codex` (case-insensitive).
#[must_use]
pub fn codex_base_url_from_chatgpt_base_url(raw_value: &str) -> Option<String> {
    let mut components = normalized_http_components(raw_value)?;
    if components.path.to_lowercase().ends_with("/codex") {
        return normalized_url_string(&components);
    }
    components.path = if components.path.is_empty() {
        "/codex".to_owned()
    } else {
        format!("{}/codex", components.path)
    };
    normalized_url_string(&components)
}

/// `HermesAgentCodexEnvironment.customBaseURL(fromOpenAIBaseURL:)`
/// (`HermesAgentCodexEnvironment.swift:184-190`).
///
/// Passes through any non-`api.openai.com` HTTP(S) URL; OpenAI's own host is
/// rejected (`None`).
#[must_use]
pub fn custom_base_url_from_openai_base_url(raw_value: &str) -> Option<String> {
    let components = normalized_http_components(raw_value)?;
    if host_matches(&components.host, "api.openai.com") {
        return None;
    }
    normalized_url_string(&components)
}

/// `HermesAgentCodexEnvironment.customBaseURL(fromChatGPTBaseURL:)`
/// (`HermesAgentCodexEnvironment.swift:193-205`).
///
/// For a non-ChatGPT host whose path is (under) `/backend-api`, rewrites the
/// path to `/v1`; everything else yields `None`.
#[must_use]
pub fn custom_base_url_from_chatgpt_base_url(raw_value: &str) -> Option<String> {
    let mut components = normalized_http_components(raw_value)?;
    if host_matches(&components.host, "chatgpt.com")
        || host_matches(&components.host, "chat.openai.com")
    {
        return None;
    }
    if components.path == "/backend-api" || components.path.starts_with("/backend-api/") {
        components.path = "/v1".to_owned();
        return normalized_url_string(&components);
    }
    None
}

// ---------------------------------------------------------------------------
// TOML line parsing (`tomlStringValue` / `parseTomlQuotedString` /
// `stripTomlComment`).
// ---------------------------------------------------------------------------

/// `HermesAgentCodexEnvironment.tomlStringValue(forKey:in:)`
/// (`HermesAgentCodexEnvironment.swift:234-247`).
///
/// A bare, `"`-quoted, or `'`-quoted `key = "value"` on a single line; the
/// value is decoded by [`parse_toml_quoted_string`]. Trailing `# comment`s are
/// stripped first (outside strings).
fn toml_string_value(key: &str, line: &str) -> Option<String> {
    let stripped = strip_toml_comment(line);
    let without_comment = trim_toml_ws(&stripped);
    if without_comment.is_empty() {
        return None;
    }
    let equals_index = without_comment.find('=')?;
    let key_part = trim_toml_ws(&without_comment[..equals_index]);
    if key_part != key
        && key_part != format!("\"{key}\"")
        && key_part != format!("'{key}'")
    {
        return None;
    }
    let value_part = trim_toml_ws(&without_comment[equals_index + 1..]);
    parse_toml_quoted_string(value_part)
}

/// `HermesAgentCodexEnvironment.parseTomlQuotedString(_:)`
/// (`HermesAgentCodexEnvironment.swift:249-280`).
///
/// A literal (`'…'`, no escapes) or basic (`"…"`, with `\" \\ \n \r \t` and
/// passthrough-any-other-escape) TOML string; `None` when unterminated or not
/// quoted.
fn parse_toml_quoted_string(value: &str) -> Option<String> {
    let first = value.chars().next()?;
    if first == '\'' {
        let rest = &value['\''.len_utf8()..];
        let end = rest.find('\'')?;
        return Some(rest[..end].to_owned());
    }
    if first != '"' {
        return None;
    }
    let mut result = String::new();
    let mut is_escaped = false;
    for character in value['"'.len_utf8()..].chars() {
        if is_escaped {
            match character {
                '"' | '\\' => result.push(character),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                other => result.push(other),
            }
            is_escaped = false;
        } else if character == '\\' {
            is_escaped = true;
        } else if character == '"' {
            return Some(result);
        } else {
            result.push(character);
        }
    }
    None
}

/// `HermesAgentCodexEnvironment.stripTomlComment(from:)`
/// (`HermesAgentCodexEnvironment.swift:282-307`).
///
/// Drops a `#` comment and everything after it, honoring `'`/`"` string
/// quoting (and `\`-escapes inside `"` strings) so a `#` within a value is
/// preserved.
fn strip_toml_comment(line: &str) -> String {
    let mut result = String::new();
    let mut quote: Option<char> = None;
    let mut is_escaped = false;
    for character in line.chars() {
        if let Some(active_quote) = quote {
            result.push(character);
            if is_escaped {
                is_escaped = false;
            } else if character == '\\' && active_quote == '"' {
                is_escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if character == '#' {
            break;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
        }
        result.push(character);
    }
    result
}

// ---------------------------------------------------------------------------
// Whitespace / normalization helpers.
// ---------------------------------------------------------------------------

/// Port of `String.trimmingCharacters(in: .whitespacesAndNewlines)` returning
/// `nil` for empty — Swift `HermesAgentCodexEnvironment.normalized(_:)`
/// (`HermesAgentCodexEnvironment.swift:309-312`). `.whitespacesAndNewlines`
/// is the Unicode `White_Space` set, so [`str::trim`] matches exactly.
fn normalized_value(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// `true` for `CharacterSet.whitespaces` — horizontal whitespace only. That is
/// Unicode `White_Space` minus the newline scalars, so a trailing `\r` from a
/// CRLF-terminated config line is intentionally *not* trimmed (matching Swift's
/// `.split(separator: "\n")` + `.trimmingCharacters(in: .whitespaces)`).
fn is_toml_horizontal_whitespace(character: char) -> bool {
    character.is_whitespace()
        && !matches!(
            character,
            '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
        )
}

/// `String.trimmingCharacters(in: .whitespaces)`.
fn trim_toml_ws(value: &str) -> &str {
    value.trim_matches(is_toml_horizontal_whitespace)
}

// ---------------------------------------------------------------------------
// URL host/path normalization (`normalizedHTTPComponents` / `hostMatches` /
// `normalizedURLString`). See crate sanctioned divergence 1.
// ---------------------------------------------------------------------------

/// The subset of `URLComponents` fields this port inspects and reassembles.
struct UrlComponents {
    scheme: String,
    userinfo: Option<String>,
    host: String,
    port: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

/// `HermesAgentCodexEnvironment.normalizedHTTPComponents(from:)`
/// (`HermesAgentCodexEnvironment.swift:314-333`).
///
/// Requires an `http`/`https` URL with a non-empty host and no query/fragment;
/// lowercases the scheme (not the host), trims the host, and strips trailing
/// `/` from the path.
fn normalized_http_components(raw_value: &str) -> Option<UrlComponents> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut components = parse_url_components(trimmed)?;
    let scheme = components.scheme.to_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = normalized_value(Some(&components.host))?;
    if components.query.is_some() || components.fragment.is_some() {
        return None;
    }
    components.scheme = scheme;
    components.host = host;
    components.path = components.path.trim_end_matches('/').to_owned();
    Some(components)
}

/// `HermesAgentCodexEnvironment.normalizedURLString(from:)`
/// (`HermesAgentCodexEnvironment.swift:335-337`): reassemble `URLComponents.string`
/// then strip trailing `/`. Query/fragment are guaranteed absent by
/// [`normalized_http_components`], so they are never emitted.
fn normalized_url_string(components: &UrlComponents) -> Option<String> {
    let mut string = String::new();
    string.push_str(&components.scheme);
    string.push_str("://");
    if let Some(userinfo) = &components.userinfo {
        string.push_str(userinfo);
        string.push('@');
    }
    string.push_str(&components.host);
    if let Some(port) = &components.port {
        string.push(':');
        string.push_str(port);
    }
    string.push_str(&components.path);
    Some(string.trim_end_matches('/').to_owned())
}

/// `HermesAgentCodexEnvironment.hostMatches(_:hostSuffix:)`
/// (`HermesAgentCodexEnvironment.swift:339-345`): exact host, or a dotted
/// subdomain suffix, both compared case-insensitively.
fn host_matches(raw_host: &str, host_suffix: &str) -> bool {
    let Some(host) = normalized_value(Some(raw_host)) else {
        return false;
    };
    let host = host.to_lowercase();
    let suffix = host_suffix.to_lowercase();
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// Focused RFC-3986 parser standing in for `URLComponents(string:)` (see crate
/// sanctioned divergence 1). Splits `scheme "://" [userinfo "@"] host [":"
/// port] path [ "?" query ] [ "#" fragment ]`; rejects a space/control char
/// anywhere and a non-numeric port (both `URLComponents` nil cases).
fn parse_url_components(value: &str) -> Option<UrlComponents> {
    if value.chars().any(|character| character.is_control() || character == ' ') {
        return None;
    }
    let (before_fragment, fragment) = match value.find('#') {
        Some(index) => (&value[..index], Some(value[index + 1..].to_owned())),
        None => (value, None),
    };
    let (before_query, query) = match before_fragment.find('?') {
        Some(index) => (
            &before_fragment[..index],
            Some(before_fragment[index + 1..].to_owned()),
        ),
        None => (before_fragment, None),
    };
    let scheme_end = before_query.find(':')?;
    let scheme = &before_query[..scheme_end];
    let mut scheme_chars = scheme.chars();
    let first_scheme_char = scheme_chars.next()?;
    if !first_scheme_char.is_ascii_alphabetic() {
        return None;
    }
    if !scheme_chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    let after_scheme = &before_query[scheme_end + 1..];
    let (userinfo, host, port, path) = if let Some(rest) = after_scheme.strip_prefix("//") {
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], rest[index..].to_owned()),
            None => (rest, String::new()),
        };
        let (userinfo, host_port) = match authority.rfind('@') {
            Some(index) => (Some(authority[..index].to_owned()), &authority[index + 1..]),
            None => (None, authority),
        };
        let (host, port) = split_host_port(host_port)?;
        (userinfo, host, port, path)
    } else {
        (None, String::new(), None, after_scheme.to_owned())
    };
    Some(UrlComponents {
        scheme: scheme.to_owned(),
        userinfo,
        host,
        port,
        path,
        query,
        fragment,
    })
}

/// Splits an authority's `host[:port]` (supporting an `[IPv6]` literal),
/// requiring a numeric port when present. Mirrors `URLComponents` rejecting a
/// non-numeric port with `nil`.
fn split_host_port(host_port: &str) -> Option<(String, Option<String>)> {
    if let Some(rest) = host_port.strip_prefix('[') {
        let close = rest.find(']')?;
        let host = format!("[{}]", &rest[..close]);
        let after = &rest[close + 1..];
        let Some(port) = after.strip_prefix(':') else {
            return Some((host, None));
        };
        return numeric_port(port).map(|port| (host, port));
    }
    match host_port.rfind(':') {
        Some(index) => {
            numeric_port(&host_port[index + 1..]).map(|port| (host_port[..index].to_owned(), port))
        }
        None => Some((host_port.to_owned(), None)),
    }
}

/// An empty port is dropped (`None`); a non-empty port must be all ASCII digits
/// or the URL is rejected.
fn numeric_port(port: &str) -> Option<Option<String>> {
    if port.is_empty() {
        Some(None)
    } else if port.chars().all(|c| c.is_ascii_digit()) {
        Some(Some(port.to_owned()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `HashMap<String, String>` from `(key, value)` literal pairs.
    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Swift `normalizesCodexChatGPTBaseURLForHermes` (12 assertions).
    // -----------------------------------------------------------------------

    #[test]
    fn normalizes_codex_chatgpt_base_url_for_hermes() {
        assert_eq!(
            codex_base_url_from_chatgpt_base_url("http://subrouter-team:31415/backend-api")
                .as_deref(),
            Some("http://subrouter-team:31415/backend-api/codex")
        );
        assert_eq!(
            codex_base_url_from_chatgpt_base_url("http://subrouter-team:31415/backend-api/codex/")
                .as_deref(),
            Some("http://subrouter-team:31415/backend-api/codex")
        );
        assert_eq!(
            custom_base_url_from_chatgpt_base_url("http://subrouter-team:31415/backend-api")
                .as_deref(),
            Some("http://subrouter-team:31415/v1")
        );
        assert_eq!(
            custom_base_url_from_openai_base_url("http://subrouter-team:31415/v1/").as_deref(),
            Some("http://subrouter-team:31415/v1")
        );
        assert_eq!(
            custom_base_url_from_openai_base_url("https://api.openai.com/v1"),
            None
        );
        assert_eq!(
            custom_base_url_from_chatgpt_base_url("https://chatgpt.com/backend-api"),
            None
        );
        assert_eq!(codex_base_url_from_chatgpt_base_url("https://"), None);
        assert_eq!(codex_base_url_from_chatgpt_base_url("https://host?x=1"), None);
        assert_eq!(custom_base_url_from_openai_base_url("https://"), None);
        assert_eq!(custom_base_url_from_openai_base_url("https://host?x=1"), None);
        assert_eq!(custom_base_url_from_chatgpt_base_url("https://"), None);
        assert_eq!(custom_base_url_from_chatgpt_base_url("https://host?x=1"), None);
    }

    // -----------------------------------------------------------------------
    // Swift `readsTopLevelCodexBaseURLs` (3 assertions).
    // -----------------------------------------------------------------------

    #[test]
    fn reads_top_level_codex_base_urls() {
        let content = concat!(
            "model = \"gpt-5.5\"\n",
            "openai_base_url = \"http://subrouter-team:31415/v1\"\n",
            "chatgpt_base_url = \"http://subrouter-team:31415/backend-api\" # route Codex backend\n",
            "\n",
            "[profiles.work]\n",
            "openai_base_url = \"http://ignored.example/v1\"\n",
            "chatgpt_base_url = \"http://ignored.example/backend-api\"",
        );

        assert_eq!(
            codex_base_url_from_codex_config_content(content).as_deref(),
            Some("http://subrouter-team:31415/backend-api/codex")
        );
        assert_eq!(
            custom_base_url_from_codex_config_content(content).as_deref(),
            Some("http://subrouter-team:31415/v1")
        );
        assert_eq!(
            codex_model_from_codex_config_content(content).as_deref(),
            Some("gpt-5.5")
        );
    }

    // -----------------------------------------------------------------------
    // Swift `appliesCodexBaseURLFromCodexHome` (4 assertions), reshaped to pass
    // the config content directly rather than reading a temp `config.toml`.
    // -----------------------------------------------------------------------

    #[test]
    fn applies_codex_base_url_without_overriding_explicit_hermes_url() {
        let content = concat!(
            "openai_base_url = \"http://subrouter-team:31415/v1\"\n",
            "chatgpt_base_url = \"http://subrouter-team:31415/backend-api\"",
        );

        let applied = applying_default_codex_base_url(env(&[]), Some(content));
        assert_eq!(
            applied.get("HERMES_CODEX_BASE_URL").map(String::as_str),
            Some("http://subrouter-team:31415/backend-api/codex")
        );
        assert_eq!(
            applied.get("CUSTOM_BASE_URL").map(String::as_str),
            Some("http://subrouter-team:31415/v1")
        );

        let explicit = applying_default_codex_base_url(
            env(&[
                ("CUSTOM_BASE_URL", "http://custom.example/v1"),
                (
                    "HERMES_CODEX_BASE_URL",
                    "http://custom.example/backend-api/codex",
                ),
            ]),
            Some(content),
        );
        assert_eq!(
            explicit.get("HERMES_CODEX_BASE_URL").map(String::as_str),
            Some("http://custom.example/backend-api/codex")
        );
        assert_eq!(
            explicit.get("CUSTOM_BASE_URL").map(String::as_str),
            Some("http://custom.example/v1")
        );
    }

    // -----------------------------------------------------------------------
    // Parity-risk edges called out in the port notes.
    // -----------------------------------------------------------------------

    /// `None` config content (Swift's absent/unreadable `config.toml`) leaves
    /// the environment untouched.
    #[test]
    fn absent_config_content_returns_environment_unchanged() {
        let environment = env(&[("EXISTING", "value")]);
        assert_eq!(
            applying_default_codex_base_url(environment.clone(), None),
            environment
        );
    }

    /// A blank (whitespace-only) explicit value is treated as absent and gets
    /// overridden — mirrors Swift's `normalized(...) == nil` gate.
    #[test]
    fn blank_explicit_value_is_overridden() {
        let content = "chatgpt_base_url = \"http://subrouter-team:31415/backend-api\"";
        let applied =
            applying_default_codex_base_url(env(&[("HERMES_CODEX_BASE_URL", "   ")]), Some(content));
        assert_eq!(
            applied.get("HERMES_CODEX_BASE_URL").map(String::as_str),
            Some("http://subrouter-team:31415/backend-api/codex")
        );
    }

    /// A `[table]` header stops the top-level scan: keys under a profile table
    /// never leak into the derived URLs/model.
    #[test]
    fn table_header_stops_top_level_scan() {
        let content = concat!(
            "[profiles.work]\n",
            "model = \"gpt-5.5\"\n",
            "chatgpt_base_url = \"http://subrouter-team:31415/backend-api\"\n",
            "openai_base_url = \"http://subrouter-team:31415/v1\"",
        );
        assert_eq!(codex_base_url_from_codex_config_content(content), None);
        assert_eq!(custom_base_url_from_codex_config_content(content), None);
        assert_eq!(codex_model_from_codex_config_content(content), None);
    }

    /// A `#` inside a quoted value is preserved; a trailing `# comment` is
    /// stripped. Also exercises the single-quote (literal) TOML form.
    #[test]
    fn strips_trailing_comment_but_keeps_hash_in_value() {
        let content = "model = 'gpt#5' # trailing comment";
        assert_eq!(
            codex_model_from_codex_config_content(content).as_deref(),
            Some("gpt#5")
        );
    }

    /// A CRLF-terminated line keeps its `\r` after `.whitespaces` trimming, but
    /// the double-quoted value decodes cleanly regardless.
    #[test]
    fn crlf_line_value_parses() {
        let content = "chatgpt_base_url = \"http://subrouter-team:31415/backend-api\"\r\nmodel = \"gpt-5.5\"";
        assert_eq!(
            codex_base_url_from_codex_config_content(content).as_deref(),
            Some("http://subrouter-team:31415/backend-api/codex")
        );
    }

    /// Empty-path ChatGPT base URL gains a `/codex` path.
    #[test]
    fn empty_path_gains_codex_segment() {
        assert_eq!(
            codex_base_url_from_chatgpt_base_url("https://sub.chatgpt-proxy.example").as_deref(),
            Some("https://sub.chatgpt-proxy.example/codex")
        );
    }

    /// Subdomain of a rejected host is also rejected (`hostMatches` dotted
    /// suffix), and scheme case is normalized.
    #[test]
    fn subdomain_of_openai_host_is_rejected() {
        assert_eq!(
            custom_base_url_from_openai_base_url("HTTPS://EU.API.OPENAI.COM/v1"),
            None
        );
    }
}
