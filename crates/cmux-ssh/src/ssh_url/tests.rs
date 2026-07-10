//! Oracle ported verbatim from `cmuxTests/CmuxSSHURLRequestTests.swift`
//! lines 17-666 (the `CmuxSSHURLRequestTests` SSH cases; the `CmuxTextURLRequest`
//! and `CmuxNavigationURLRequest` cases are separate lanes).
//!
//! The Swift tests build inputs two ways — `URLComponents{…}.url` and
//! `URL(string:)`. Both round-trip through `URLComponents.queryItems`, so each
//! case is reproduced here as the equivalent URL *string* (spaces / control /
//! non-ASCII bytes percent-encoded exactly as Foundation serializes them;
//! sub-delims like `,` `:` `=` left literal since Foundation's `queryItems`
//! split-on-first-`=` round-trips them unchanged).

use super::*;

const SCHEME: &str = "cmux";

/// Swift default parser: `parse(url)` with `activeSupportedSchemes == [scheme]`.
fn parse(url: &str) -> Result<Option<CmuxSSHURLRequest>, CmuxSSHURLParseError> {
    CmuxSSHURLRequest::parse(url, &[SCHEME])
}

fn expect_request(url: &str) -> CmuxSSHURLRequest {
    match parse(url) {
        Ok(Some(request)) => request,
        other => panic!("expected SSH request for {url:?}, got {other:?}"),
    }
}

fn expect_error(url: &str) -> CmuxSSHURLParseError {
    match parse(url) {
        Err(error) => error,
        other => panic!("expected parse error for {url:?}, got {other:?}"),
    }
}

// --- happy-path cmux deep links -------------------------------------------

// Swift: testParsesSSHURLWithExplicitHostUserPortAndTitle
#[test]
fn parses_ssh_url_with_explicit_host_user_port_and_title() {
    let request =
        expect_request("cmux://ssh?host=dev.example.com&user=alice&port=2222&title=Dev%20SSH");
    assert_eq!(request.destination, "alice@dev.example.com");
    assert_eq!(request.port, Some(2222));
    assert_eq!(request.title.as_deref(), Some("Dev SSH"));
    assert_eq!(
        request.cli_arguments(),
        [
            "ssh",
            "--port",
            "2222",
            "--name",
            "Dev SSH",
            "alice@dev.example.com"
        ]
    );
}

// Swift: testParsesSSHURLWithAllowedConnectionKnobs
#[test]
fn parses_ssh_url_with_allowed_connection_knobs() {
    let request = expect_request(
        "cmux://ssh?host=dev.example.com&user=alice&port=2222&title=Dev%20SSH\
         &connect-timeout=15&server-alive-interval=20&server-alive-count-max=4\
         &host-key-policy=accept-new&no-focus=true",
    );
    assert_eq!(request.destination, "alice@dev.example.com");
    assert_eq!(request.port, Some(2222));
    assert_eq!(request.title.as_deref(), Some("Dev SSH"));
    assert_eq!(
        request.ssh_options,
        [
            "ConnectTimeout=15",
            "ServerAliveInterval=20",
            "ServerAliveCountMax=4",
            "StrictHostKeyChecking=accept-new",
        ]
    );
    assert!(request.no_focus);
    assert_eq!(
        request.cli_arguments(),
        [
            "ssh",
            "--port",
            "2222",
            "--name",
            "Dev SSH",
            "--ssh-option",
            "ConnectTimeout=15",
            "--ssh-option",
            "ServerAliveInterval=20",
            "--ssh-option",
            "ServerAliveCountMax=4",
            "--ssh-option",
            "StrictHostKeyChecking=accept-new",
            "--no-focus",
            "alice@dev.example.com",
        ]
    );
}

// Swift: testParsesSSHURLWithFreestyleUserDelimiters
#[test]
fn parses_ssh_url_with_freestyle_user_delimiters() {
    let host = "workspace123.vm-ssh.freestyle.sh";
    for user in [
        "workspace123,session-token_ABC.2yi9kzY-dysFsVBKh",
        "workspace123:session-token_ABC.2yi9kzY-dysFsVBKh",
    ] {
        let url = format!("cmux://ssh?host={host}&user={user}");
        let request = expect_request(&url);
        assert_eq!(request.destination, format!("{user}@{host}"));
        assert_eq!(request.cli_arguments(), ["ssh", &format!("{user}@{host}")]);
    }
}

// Swift: testCommandPreviewIncludesSocketPathWhenProvided
#[test]
fn command_preview_includes_socket_path_when_provided() {
    let request = expect_request("cmux://ssh?host=dev.example.com&title=Dev%20SSH");
    assert_eq!(
        request.cli_preview_with_socket(Some("/tmp/cmux-urlcmd.sock")),
        "cmux --socket /tmp/cmux-urlcmd.sock ssh --name \"Dev SSH\" dev.example.com"
    );
}

// Swift: testParsesNoFocusFlagWithoutValue
#[test]
fn parses_no_focus_flag_without_value() {
    let request = expect_request("cmux://ssh?host=dev.example.com&no-focus");
    assert!(request.no_focus);
    assert_eq!(
        request.cli_arguments(),
        ["ssh", "--no-focus", "dev.example.com"]
    );
}

// Swift: testParsesNoFocusFalseAsDisabled
#[test]
fn parses_no_focus_false_as_disabled() {
    let request = expect_request("cmux://ssh?host=dev.example.com&no-focus=false");
    assert!(!request.no_focus);
    assert_eq!(request.cli_arguments(), ["ssh", "dev.example.com"]);
}

// Swift: testParsesStableNightlyAndDevSchemes
#[test]
fn parses_stable_nightly_and_dev_schemes() {
    for scheme in SUPPORTED_SCHEMES {
        let url = format!("{scheme}://ssh?host=dev.example.com");
        match CmuxSSHURLRequest::parse(&url, &SUPPORTED_SCHEMES) {
            Ok(Some(request)) => assert_eq!(request.destination, "dev.example.com"),
            other => panic!("expected SSH request for {scheme}, got {other:?}"),
        }
    }
}

// Swift: testDefaultParserIgnoresOtherProductSchemes
#[test]
fn default_parser_ignores_other_product_schemes() {
    // An inactive product scheme (in SUPPORTED_SCHEMES but != the active one).
    let url = "cmux-nightly://ssh?host=dev.example.com";
    assert_eq!(parse(url), Ok(None));
}

// Swift: testIgnoresNonSSHURLs
#[test]
fn ignores_non_ssh_urls() {
    assert_eq!(
        parse("cmux://auth-callback?stack_refresh=abc&stack_access=def"),
        Ok(None)
    );
    assert_eq!(
        parse("https://example.com/ssh?host=dev.example.com"),
        Ok(None)
    );
}

// Swift: testTrimsWhitespaceAroundStructuredHost
#[test]
fn trims_whitespace_around_structured_host() {
    // host value = "\ndev.example.com" (URLComponents encodes the newline).
    let request = expect_request("cmux://ssh?host=%0Adev.example.com");
    assert_eq!(request.destination, "dev.example.com");
}

// Swift: testUsesNameWhenTitleIsBlank
#[test]
fn uses_name_when_title_is_blank() {
    // title = " " (blank → dropped), name = "Dev SSH".
    let request = expect_request("cmux://ssh?host=dev.example.com&title=%20&name=Dev%20SSH");
    assert_eq!(request.title.as_deref(), Some("Dev SSH"));
    assert_eq!(
        request.cli_arguments(),
        ["ssh", "--name", "Dev SSH", "dev.example.com"]
    );
}

// --- standard ssh:// URLs --------------------------------------------------

// Swift: testParsesStandardSSHURL
#[test]
fn parses_standard_ssh_url() {
    let request = expect_request("ssh://alice@dev.example.com:2222?title=Dev%20SSH");
    assert_eq!(request.destination, "alice@dev.example.com");
    assert_eq!(request.port, Some(2222));
    assert_eq!(request.title.as_deref(), Some("Dev SSH"));
    assert_eq!(
        request.cli_arguments(),
        [
            "ssh",
            "--port",
            "2222",
            "--name",
            "Dev SSH",
            "alice@dev.example.com"
        ]
    );
}

// Swift: testParsesStandardSSHURLWithIPv6Host
#[test]
fn parses_standard_ssh_url_with_ipv6_host() {
    let request = expect_request("ssh://alice@[2001:db8::1]:2222");
    assert_eq!(request.destination, "alice@2001:db8::1");
    assert_eq!(request.port, Some(2222));
    assert_eq!(
        request.cli_arguments(),
        ["ssh", "--port", "2222", "alice@2001:db8::1"]
    );
}

// Swift: testParsesStandardSSHURLWithBlankUserAsHostOnly
#[test]
fn parses_standard_ssh_url_with_blank_user_as_host_only() {
    // user = "%20" decodes to " " → trims to blank → host-only.
    let request = expect_request("ssh://%20@dev.example.com");
    assert_eq!(request.destination, "dev.example.com");
    assert_eq!(request.cli_arguments(), ["ssh", "dev.example.com"]);
}

// Swift: testRejectsStandardSSHURLWithPathDestination
#[test]
fn rejects_standard_ssh_url_with_path_destination() {
    assert_eq!(
        expect_error("ssh://dev.example.com/run"),
        CmuxSSHURLParseError::ConflictingDestinationParameters
    );
}

// Swift: testRejectsStandardSSHURLWithInvalidPort
#[test]
fn rejects_standard_ssh_url_with_invalid_port() {
    for url in [
        "ssh://dev.example.com:",
        "ssh://dev.example.com:0",
        "ssh://dev.example.com:65536",
        "ssh://dev.example.com:999999999999999999999999999999",
    ] {
        assert_eq!(
            expect_error(url),
            CmuxSSHURLParseError::InvalidPort,
            "{url}"
        );
    }
}

// Pinning test: Foundation's `components.port` uses RFC 3986 port grammar
// (`*DIGIT`), so a signed port substring makes `components.port == nil`, and the
// `standardSSHURLHasExplicitPort` fallback then returns `.invalidPort`. Rust's
// `i64::from_str` accepts a leading sign (`"+22".parse::<i64>() == Ok(22)`), so
// without the digit-only guard `ssh://host:+22` would parse to port 22 — a
// divergence not covered by the Swift oracle above. See
// `standard_ssh_url_port` (Swift: CmuxSSHURLRequest.swift 306-340).
#[test]
fn rejects_standard_ssh_url_with_signed_port() {
    for url in ["ssh://dev.example.com:+22", "ssh://dev.example.com:-22"] {
        assert_eq!(
            expect_error(url),
            CmuxSSHURLParseError::InvalidPort,
            "{url}"
        );
    }
}

// Swift: testRejectsStandardSSHURLWithEncodedHostWhitespace
#[test]
fn rejects_standard_ssh_url_with_encoded_host_whitespace() {
    for url in ["ssh://%20host", "ssh://host%20", "ssh://ho%0Ast"] {
        assert_eq!(
            expect_error(url),
            CmuxSSHURLParseError::DestinationContainsUnsafeCharacters,
            "{url}"
        );
    }
}

// Swift: testRejectsStandardSSHURLWithPassword
#[test]
fn rejects_standard_ssh_url_with_password() {
    assert_eq!(
        expect_error("ssh://alice:secret@dev.example.com"),
        CmuxSSHURLParseError::UnsupportedParameter("password".to_string())
    );
}

// --- rejections (cmux deep links) -----------------------------------------

// Swift: testRejectsSSHURLWithPathDestination
#[test]
fn rejects_ssh_url_with_path_destination() {
    assert_eq!(
        expect_error("cmux://ssh/alice@dev.example.com"),
        CmuxSSHURLParseError::ConflictingDestinationParameters
    );
}

// Swift: testRejectsMissingDestination
#[test]
fn rejects_missing_destination() {
    assert_eq!(
        expect_error("cmux://ssh?title=Missing"),
        CmuxSSHURLParseError::MissingDestination
    );
}

// Swift: testRejectsHiddenControlCharacters
#[test]
fn rejects_hidden_control_characters() {
    // host = "dev.example.com\nbad".
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com%0Abad"),
        CmuxSSHURLParseError::DestinationContainsUnsafeCharacters
    );
}

// Swift: testRejectsStructuredHostPortInHostParameter
#[test]
fn rejects_structured_host_port_in_host_parameter() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com:2222"),
        CmuxSSHURLParseError::DestinationContainsUnsafeCharacters
    );
}

// Swift: testRejectsConflictingTitleAliases
#[test]
fn rejects_conflicting_title_aliases() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&title=Title&name=Name"),
        CmuxSSHURLParseError::ConflictingTitleParameters
    );
}

// Swift: testRejectsDashPrefixedDestination
#[test]
fn rejects_dash_prefixed_destination() {
    assert_eq!(
        expect_error("cmux://ssh?host=-oProxyCommand=bad"),
        CmuxSSHURLParseError::DestinationStartsWithDash
    );
}

// Swift: testRejectsUnicodeFormatCharacters
#[test]
fn rejects_unicode_format_characters() {
    // host = "safe\u{202E}bad.example.com" (RIGHT-TO-LEFT OVERRIDE, gc=Cf).
    assert_eq!(
        expect_error("cmux://ssh?host=safe%E2%80%AEbad.example.com"),
        CmuxSSHURLParseError::DestinationContainsUnsafeCharacters
    );
}

// Swift: testRejectsUnicodeSeparatorsInTitle
#[test]
fn rejects_unicode_separators_in_title() {
    // title = "safe\u{2028}hidden" (LINE SEPARATOR, gc=Zl).
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&title=safe%E2%80%A8hidden"),
        CmuxSSHURLParseError::TitleContainsUnsafeCharacters
    );
}

// Swift: testRejectsIdentityParameterFromExternalLinks
#[test]
fn rejects_identity_parameter_from_external_links() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&identity=~/.ssh/id_ed25519"),
        CmuxSSHURLParseError::UnsupportedParameter("identity".to_string())
    );
}

// Swift: testRejectsRawSSHOptionParameterFromExternalLinks
#[test]
fn rejects_raw_ssh_option_parameter_from_external_links() {
    for option in [
        "HostName=evil.example.com",
        "ProxyJump=evil.example.com",
        "ProxyCommand=/bin/sh%20-c%20id",
        "SendEnv=*",
        "ControlMaster=auto",
        "StrictHostKeyChecking%20=%20no",
        "UserKnownHostsFile=/tmp/link-known-hosts",
    ] {
        let url = format!("cmux://ssh?host=dev.example.com&ssh-option={option}");
        assert_eq!(
            expect_error(&url),
            CmuxSSHURLParseError::UnsupportedParameter("ssh-option".to_string()),
            "{option}"
        );
    }
}

// Swift: testParsesAllowedHostKeyPolicies
#[test]
fn parses_allowed_host_key_policies() {
    for (value, option) in [
        ("accept-new", "StrictHostKeyChecking=accept-new"),
        ("ask", "StrictHostKeyChecking=ask"),
        ("strict", "StrictHostKeyChecking=yes"),
        ("yes", "StrictHostKeyChecking=yes"),
    ] {
        let url = format!("cmux://ssh?host=dev.example.com&host-key-policy={value}");
        let request = expect_request(&url);
        assert_eq!(request.ssh_options, [option], "{value}");
    }
}

// Swift: testRejectsHostKeyPolicyThatDisablesChecking
#[test]
fn rejects_host_key_policy_that_disables_checking() {
    for value in ["no", "off", "false", "0"] {
        let url = format!("cmux://ssh?host=dev.example.com&host-key-policy={value}");
        assert_eq!(
            expect_error(&url),
            CmuxSSHURLParseError::InvalidHostKeyPolicy("host-key-policy".to_string()),
            "{value}"
        );
    }
}

// Swift: testRejectsInvalidStructuredIntegerKnobs
#[test]
fn rejects_invalid_structured_integer_knobs() {
    for (parameter, value) in [
        ("connect-timeout", "0"),
        ("connect-timeout", "601"),
        ("server-alive-interval", "0"),
        ("server-alive-interval", "3601"),
        ("server-alive-count-max", "0"),
        ("server-alive-count-max", "101"),
        ("server-alive-count-max", "1%0A2"), // "1\n2"
        ("server-alive-count-max", "1.5"),
    ] {
        let url = format!("cmux://ssh?host=dev.example.com&{parameter}={value}");
        assert_eq!(
            expect_error(&url),
            CmuxSSHURLParseError::InvalidIntegerParameter(parameter.to_string()),
            "{parameter}={value}"
        );
    }
}

// Swift: testRejectsInvalidNoFocusValue
#[test]
fn rejects_invalid_no_focus_value() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&no-focus=maybe"),
        CmuxSSHURLParseError::InvalidBooleanParameter("no-focus".to_string())
    );
}

// Swift: testRejectsDuplicateStructuredKnobs
#[test]
fn rejects_duplicate_structured_knobs() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&connect-timeout=10&connect-timeout=20"),
        CmuxSSHURLParseError::DuplicateParameter("connect-timeout".to_string())
    );
}

// Swift: testRejectsUnsupportedCommandParameter
#[test]
fn rejects_unsupported_command_parameter() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&command=whoami"),
        CmuxSSHURLParseError::UnsupportedParameter("command".to_string())
    );
}

// Swift: testRejectsOpaqueDestinationParameter
#[test]
fn rejects_opaque_destination_parameter() {
    assert_eq!(
        expect_error("cmux://ssh?destination=alice@dev.example.com"),
        CmuxSSHURLParseError::UnsupportedParameter("destination".to_string())
    );
}

// Swift: testRejectsDuplicateParameters
#[test]
fn rejects_duplicate_parameters() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&host=prod.example.com"),
        CmuxSSHURLParseError::DuplicateParameter("host".to_string())
    );
}

// Swift: testRejectsUnsafeUser
#[test]
fn rejects_unsafe_user() {
    assert_eq!(
        expect_error("cmux://ssh?host=dev.example.com&user=alice;bad"),
        CmuxSSHURLParseError::DestinationContainsUnsafeCharacters
    );
}

// --- displayTarget accessor (Swift 67-72) ---------------------------------

#[test]
fn display_target_includes_port_when_present() {
    let request = expect_request("cmux://ssh?host=dev.example.com&port=2222");
    assert_eq!(request.display_target(), "dev.example.com:2222");
    let request = expect_request("cmux://ssh?host=dev.example.com");
    assert_eq!(request.display_target(), "dev.example.com");
}

// ==========================================================================
// URLComponents-conformance probe (dev-dependency `url`): pins the divergences
// documented in the module header that motivate the hand-rolled splitter.
// ==========================================================================

#[test]
fn url_components_conformance_plus_is_literal_not_space() {
    // `url` crate: query_pairs() form-decodes '+' → space.
    let u = url::Url::parse("cmux://ssh?text=a+b").unwrap();
    let (name, value) = u.query_pairs().next().unwrap();
    assert_eq!(name, "text");
    assert_eq!(value, "a b", "url crate diverges: '+' became a space");

    // Our splitter (matching URLComponents) keeps '+' literal.
    let items = parse_query_items(Some("text=a+b"));
    assert_eq!(items[0].value.as_deref(), Some("a+b"));
}

#[test]
fn url_components_conformance_nil_vs_empty_value() {
    // `url` crate collapses "no =" and "empty value" both to "".
    let u = url::Url::parse("cmux://ssh?no-focus").unwrap();
    assert_eq!(u.query_pairs().next().unwrap().1, "");

    // Our splitter (matching URLComponents) distinguishes them: None vs Some("").
    assert_eq!(parse_query_items(Some("no-focus"))[0].value, None);
    assert_eq!(
        parse_query_items(Some("no-focus="))[0].value.as_deref(),
        Some("")
    );
}

#[test]
fn url_components_conformance_username_decoding() {
    // `url` crate keeps the userinfo percent-encoded.
    let u = url::Url::parse("ssh://%20@dev.example.com").unwrap();
    assert_eq!(
        u.username(),
        "%20",
        "url crate diverges: username not decoded"
    );

    // Our splitter (matching URLComponents.user) decodes it → blank → host-only.
    let parsed = ParsedUrl::parse("ssh://%20@dev.example.com");
    assert_eq!(parsed.user.as_deref(), Some(" "));
}

#[test]
fn url_components_conformance_ipv6_brackets() {
    // `url` crate keeps IPv6 brackets in host_str().
    let u = url::Url::parse("ssh://alice@[2001:db8::1]:2222").unwrap();
    assert_eq!(u.host_str(), Some("[2001:db8::1]"));

    // Our splitter (matching URLComponents.host) unbrackets it.
    let parsed = ParsedUrl::parse("ssh://alice@[2001:db8::1]:2222");
    assert_eq!(parsed.host.as_deref(), Some("2001:db8::1"));
    assert_eq!(parsed.port_section.as_deref(), Some("2222"));
}
