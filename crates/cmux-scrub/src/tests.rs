//! Ported test oracles: `SentryScrubberTests.swift` and
//! `ScrubberDenylistsTests.swift`, ported case-for-case.

use crate::{ScrubValue, SentryScrubber, REDACTED_SECRET};

/// A scrubber with a fixed home directory so path redaction is deterministic.
fn scrubber() -> SentryScrubber {
    SentryScrubber::new("/Users/lawrence")
}

/// Builds an ordered object from `(key, value)` pairs.
fn obj(pairs: Vec<(&str, ScrubValue)>) -> Vec<(String, ScrubValue)> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Looks a key up in an ordered object.
fn get<'a>(object: &'a [(String, ScrubValue)], key: &str) -> Option<&'a ScrubValue> {
    object.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn s(text: &str) -> ScrubValue {
    ScrubValue::Str(text.to_string())
}

// =============================================================================
// SentryScrubberTests
// =============================================================================

mod scrubber_tests {
    use super::*;

    // MARK: - Paths

    #[test]
    fn redacts_injected_home_directory() {
        assert_eq!(
            scrubber().scrub("loaded /Users/lawrence/.config/cmux/cmux.json"),
            "loaded /Users/<redacted>/.config/cmux/cmux.json"
        );
    }

    #[test]
    fn redacts_any_users_path_even_when_not_the_runtime_home() {
        assert_eq!(
            scrubber().scrub("/Users/buildbot/work/cmux/Sources/AppDelegate.swift"),
            "/Users/<redacted>/work/cmux/Sources/AppDelegate.swift"
        );
    }

    #[test]
    fn redacts_linux_home_paths() {
        assert_eq!(
            scrubber().scrub("at /home/runner/cmux/main.swift line 12"),
            "at /home/<redacted>/cmux/main.swift line 12"
        );
    }

    #[test]
    fn redacts_multiple_distinct_usernames_in_one_string() {
        let input = "/Users/alice/a.txt and /Users/bob/b.txt";
        assert_eq!(
            scrubber().scrub(input),
            "/Users/<redacted>/a.txt and /Users/<redacted>/b.txt"
        );
    }

    #[test]
    fn leaves_system_paths_untouched() {
        let input = "/usr/lib/foo /System/Library/bar /Applications/cmux.app";
        assert_eq!(scrubber().scrub(input), input);
    }

    // MARK: - Emails

    #[test]
    fn redacts_email_addresses() {
        assert_eq!(
            scrubber().scrub("signed in as lawrence@cmux.com today"),
            "signed in as <redacted-email> today"
        );
    }

    #[test]
    fn redacts_email_with_plus_and_subdomain() {
        assert_eq!(
            scrubber().scrub("to a.b+tag@mail.example.co.uk failed"),
            "to <redacted-email> failed"
        );
    }

    // MARK: - Secrets

    #[test]
    fn redacts_bearer_token() {
        assert_eq!(
            scrubber().scrub("Authorization header Bearer abc123DEF456ghi789xyz"),
            "Authorization header Bearer <redacted-secret>"
        );
    }

    #[test]
    fn redacts_token_query_parameter_but_keeps_key() {
        assert_eq!(
            scrubber().scrub("GET https://api.example.com/v1?token=supersecretvalue123&page=2"),
            "GET https://api.example.com/v1?token=<redacted-secret>&page=2"
        );
    }

    #[test]
    fn redacts_password_assignment() {
        assert_eq!(
            scrubber().scrub(r#"{"password":"hunter2hunter2hunter2"}"#),
            r#"{"password":"<redacted-secret>"}"#
        );
    }

    #[test]
    fn redacts_provider_api_key() {
        assert_eq!(
            scrubber().scrub("using sk-proj-abcdef0123456789ABCDEF to call"),
            "using <redacted-secret> to call"
        );
    }

    #[test]
    fn redacts_github_token() {
        assert_eq!(
            scrubber().scrub("clone with ghp_0123456789abcdefABCDEF0123456789abcd"),
            "clone with <redacted-secret>"
        );
    }

    #[test]
    fn redacts_json_web_token() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        assert_eq!(
            scrubber().scrub(&format!("session {jwt} expired")),
            "session <redacted-secret> expired"
        );
    }

    #[test]
    fn redacts_aws_access_key_id() {
        assert_eq!(
            scrubber().scrub("creds AKIAIOSFODNN7EXAMPLE rejected"),
            "creds <redacted-secret> rejected"
        );
    }

    #[test]
    fn redacts_broader_credential_markers_in_raw_query_strings() {
        assert_eq!(
            scrubber().scrub("GET /x?auth=opaquesessionvalue&page=1"),
            "GET /x?auth=<redacted-secret>&page=1"
        );
        assert_eq!(
            scrubber().scrub("session_id=abc123def456ghi has expired"),
            "session_id=<redacted-secret> has expired"
        );
        assert_eq!(
            scrubber().scrub("cookie=sid%3Dabcdef0123 set"),
            "cookie=<redacted-secret> set"
        );
    }

    #[test]
    fn redacts_env_style_secret_assignment_with_longer_key_name() {
        assert_eq!(
            scrubber().scrub("AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY done"),
            "AWS_SECRET_ACCESS_KEY=<redacted-secret> done"
        );
        assert_eq!(
            scrubber().scrub("export MY_API_KEY=plainlettersvalue123"),
            "export MY_API_KEY=<redacted-secret>"
        );
    }

    #[test]
    fn redacts_bare_session_and_sid_aliases_but_not_substrings() {
        assert_eq!(
            scrubber().scrub("GET /x?session=abcdef1234567890&page=2"),
            "GET /x?session=<redacted-secret>&page=2"
        );
        assert_eq!(
            scrubber().scrub("redirect ?sid=abcdef0123456789 done"),
            "redirect ?sid=<redacted-secret> done"
        );
        assert_eq!(
            scrubber().scrub("usersession=plainvalue123"),
            "usersession=<redacted-secret>"
        );
        assert_eq!(scrubber().scrub("inside=hallway"), "inside=hallway");
        assert_eq!(scrubber().scrub("aside=note"), "aside=note");
    }

    #[test]
    fn session_and_sid_dictionary_keys_are_sensitive_without_overmatching() {
        let input = obj(vec![
            ("session", s("abc")),
            ("sid", s("def")),
            ("inside", s("hallway")),
            ("presidency", s("term")),
            ("count", ScrubValue::Int(3)),
        ]);
        let output = scrubber().scrub_dictionary(&input);
        assert_eq!(get(&output, "session"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "sid"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "inside"), Some(&s("hallway")));
        assert_eq!(get(&output, "presidency"), Some(&s("term")));
        assert_eq!(get(&output, "count"), Some(&ScrubValue::Int(3)));
    }

    #[test]
    fn redacts_raw_data_values_sentry_would_hex_encode() {
        let token_data = ScrubValue::Data(b"token=secretvalue123".to_vec());
        assert_eq!(scrubber().scrub_value(&token_data), s("<redacted-data>"));
        let dict = obj(vec![
            ("payload", ScrubValue::Data(b"token=secretvalue123".to_vec())),
            ("count", ScrubValue::Int(2)),
        ]);
        let output = scrubber().scrub_dictionary(&dict);
        assert_eq!(get(&output, "payload"), Some(&s("<redacted-data>")));
        assert_eq!(get(&output, "count"), Some(&ScrubValue::Int(2)));
    }

    #[test]
    fn redacts_url_userinfo_credentials_keeping_host() {
        assert_eq!(
            scrubber().scrub("connecting to http://alice:secret@localhost/path"),
            "connecting to http://<redacted-secret>@localhost/path"
        );
        assert_eq!(
            scrubber().scrub("redis://default:p4ss@cache.internal:6379"),
            "redis://<redacted-secret>@cache.internal:6379"
        );
        assert_eq!(
            scrubber().scrub("GET http://localhost/health"),
            "GET http://localhost/health"
        );
    }

    #[test]
    fn redacts_url_userinfo_with_unencoded_at_in_password() {
        assert_eq!(
            scrubber().scrub("redis://user:p@ss@host/db"),
            "redis://<redacted-secret>@host/db"
        );
        assert_eq!(
            scrubber().scrub("mongodb://u:p@w0rd@db.example.com:27017/x"),
            "mongodb://<redacted-secret>@db.example.com:27017/x"
        );
        assert_eq!(
            scrubber().scrub("see http://a.com/x and http://b:c@d.com/y"),
            "see http://a.com/x and http://<redacted-secret>@d.com/y"
        );
    }

    #[test]
    fn redacts_exact_home_paths_without_trailing_slash() {
        assert_eq!(
            scrubber().scrub("build dir /Users/buildbot"),
            "build dir /Users/<redacted>"
        );
        assert_eq!(scrubber().scrub("file:///Users/alice"), "file:///Users/<redacted>");
        assert_eq!(
            scrubber().scrub("at /Users/bob in frame"),
            "at /Users/<redacted> in frame"
        );
        assert_eq!(
            scrubber().scrub("/Users/carol/dev/app.swift"),
            "/Users/<redacted>/dev/app.swift"
        );
    }

    #[test]
    fn exact_home_directory_is_bounded_to_a_path_component() {
        let prefix_scrubber = SentryScrubber::new("/Users/al");
        assert_eq!(prefix_scrubber.scrub("/Users/alice/x"), "/Users/<redacted>/x");
        assert_eq!(prefix_scrubber.scrub("/Users/al/cfg"), "/Users/<redacted>/cfg");
        assert_eq!(
            prefix_scrubber.scrub("at /Users/al done"),
            "at /Users/<redacted> done"
        );
    }

    // MARK: - Structured query-string redaction

    #[test]
    fn scrubs_sensitive_query_params_by_key_keeping_non_sensitive() {
        assert_eq!(
            scrubber().scrub_query_string("_csrf=abc123&_vercel_jwt=xyz789&page=2"),
            "_csrf=<redacted-secret>&_vercel_jwt=<redacted-secret>&page=2"
        );
        assert_eq!(
            scrubber().scrub_query_string("su=rootcookie&phpsessid=deadbeef&sid=sessionval"),
            "su=<redacted-secret>&phpsessid=<redacted-secret>&sid=<redacted-secret>"
        );
        assert_eq!(scrubber().scrub_query_string("page=2&sort=asc"), "page=2&sort=asc");
    }

    #[test]
    fn scrub_query_string_does_not_mis_split_url_values() {
        assert_eq!(
            scrubber().scrub_query_string("next=https://host/p?token=x&page=2"),
            "next=https://host/p?token=x&page=2"
        );
        assert_eq!(
            scrubber().scrub_query_string("token=a=b=c&page=2"),
            "token=<redacted-secret>&page=2"
        );
    }

    #[test]
    fn scrub_query_string_handles_semicolons_bare_keys_and_empty_segments() {
        assert_eq!(
            scrubber().scrub_query_string("sid=abc;page=2"),
            "sid=<redacted-secret>;page=2"
        );
        assert_eq!(scrubber().scrub_query_string("flag&page=2"), "flag&page=2");
        assert_eq!(
            scrubber().scrub_query_string("page=1&&sid=abc"),
            "page=1&&sid=<redacted-secret>"
        );
        assert_eq!(scrubber().scrub_query_string(""), "");
    }

    // MARK: - Grouping fields preserved

    #[test]
    fn preserves_normal_error_text() {
        let input = "Fatal error: Index out of range while reading buffer";
        assert_eq!(scrubber().scrub(input), input);
    }

    #[test]
    fn preserves_exception_type_shape() {
        let input = "NSInvalidArgumentException in -[NSArray objectAtIndex:]";
        assert_eq!(scrubber().scrub(input), input);
    }

    #[test]
    fn preserves_short_identifiers_that_are_not_secrets() {
        let input = "code=42 status=ok retry=true id=ABC123";
        assert_eq!(scrubber().scrub(input), input);
    }

    #[test]
    fn empty_string_is_unchanged() {
        assert_eq!(scrubber().scrub(""), "");
    }

    // MARK: - Recursive value scrubbing

    #[test]
    fn scrubs_nested_dictionary_values() {
        let input = obj(vec![
            ("cwd", s("/Users/lawrence/dev/cmux")),
            ("email", s("lawrence@cmux.com")),
            ("count", ScrubValue::Int(7)),
            (
                "nested",
                ScrubValue::Object(obj(vec![(
                    "url",
                    s("https://x.com/?token=abcdef0123456789secret"),
                )])),
            ),
        ]);
        let output = scrubber().scrub_dictionary(&input);
        assert_eq!(get(&output, "cwd"), Some(&s("/Users/<redacted>/dev/cmux")));
        assert_eq!(get(&output, "email"), Some(&s("<redacted-email>")));
        assert_eq!(get(&output, "count"), Some(&ScrubValue::Int(7)));
        let ScrubValue::Object(nested) = get(&output, "nested").unwrap() else {
            panic!("nested should be an object");
        };
        assert_eq!(
            get(nested, "url"),
            Some(&s("https://x.com/?token=<redacted-secret>"))
        );
    }

    #[test]
    fn redacts_values_under_sensitive_keys_regardless_of_value_shape() {
        let input = obj(vec![
            ("token", s("abcdef0123456789plainvalue")),
            ("password", s("p4ssw0rd")),
            ("api_key", s("justletters")),
            ("Authorization", s("Basic dXNlcjpwYXNz")),
            ("note", s("/Users/alice/readme.txt")),
            ("count", ScrubValue::Int(5)),
        ]);
        let output = scrubber().scrub_dictionary(&input);
        assert_eq!(get(&output, "token"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "password"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "api_key"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "Authorization"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "note"), Some(&s("/Users/<redacted>/readme.txt")));
        assert_eq!(get(&output, "count"), Some(&ScrubValue::Int(5)));
    }

    #[test]
    fn redacts_structured_values_under_sensitive_keys() {
        let input = obj(vec![
            (
                "cookie",
                ScrubValue::Array(vec![s("session=abc"), s("csrf=def")]),
            ),
            (
                "credentials",
                ScrubValue::Object(obj(vec![("user", s("alice")), ("pass", s("secret"))])),
            ),
            ("note", s("plain")),
        ]);
        let output = scrubber().scrub_dictionary(&input);
        assert_eq!(get(&output, "cookie"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "credentials"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&output, "note"), Some(&s("plain")));
    }

    #[test]
    fn scrubs_context_with_sensitive_outer_name_as_boundary() {
        let input: Vec<(String, Vec<(String, ScrubValue)>)> = vec![
            (
                "credentials".to_string(),
                obj(vec![("raw", s("plainsecretvalue")), ("user", s("alice"))]),
            ),
            ("auth".to_string(), obj(vec![("bearer", s("opaquetoken"))])),
            (
                "device".to_string(),
                obj(vec![("cwd", s("/Users/alice/dev")), ("model", s("MacBookPro"))]),
            ),
        ];
        let output = scrubber().scrub_context(&input);
        let find = |name: &str| -> Vec<(String, ScrubValue)> {
            output.iter().find(|(n, _)| n == name).unwrap().1.clone()
        };
        let credentials = find("credentials");
        assert_eq!(get(&credentials, "raw"), Some(&s("<redacted-secret>")));
        assert_eq!(get(&credentials, "user"), Some(&s("<redacted-secret>")));
        let auth = find("auth");
        assert_eq!(get(&auth, "bearer"), Some(&s("<redacted-secret>")));
        let device = find("device");
        assert_eq!(get(&device, "cwd"), Some(&s("/Users/<redacted>/dev")));
        assert_eq!(get(&device, "model"), Some(&s("MacBookPro")));
    }

    #[test]
    fn sensitive_key_matching_ignores_case_and_separators() {
        assert!(SentryScrubber::is_sensitive_key("Access-Token"));
        assert!(SentryScrubber::is_sensitive_key("X_API_KEY"));
        assert!(SentryScrubber::is_sensitive_key("Cookie"));
        assert!(SentryScrubber::is_sensitive_key("session_id"));
        assert!(SentryScrubber::is_sensitive_key("authorization"));
        assert!(!SentryScrubber::is_sensitive_key("username"));
        assert!(!SentryScrubber::is_sensitive_key("count"));
        assert!(!SentryScrubber::is_sensitive_key("path"));
    }

    #[test]
    fn scrubs_url_values_which_sentry_stringifies() {
        // URL(fileURLWithPath: "/Users/alice/secret.txt").absoluteString.
        let file_url = ScrubValue::Url("file:///Users/alice/secret.txt".to_string());
        assert_eq!(
            scrubber().scrub_value(&file_url),
            s("file:///Users/<redacted>/secret.txt")
        );
        let web_url = ScrubValue::Url("https://x.com/?token=abcdef0123456789zz".to_string());
        assert_eq!(
            scrubber().scrub_value(&web_url),
            s("https://x.com/?token=<redacted-secret>")
        );
    }

    #[test]
    fn scrubs_url_nested_in_dictionary_value() {
        let input = obj(vec![(
            "where",
            ScrubValue::Url("file:///Users/bob/x".to_string()),
        )]);
        let output = scrubber().scrub_dictionary(&input);
        assert_eq!(get(&output, "where"), Some(&s("file:///Users/<redacted>/x")));
    }

    #[test]
    fn preserves_numeric_and_bool_scalars() {
        assert_eq!(scrubber().scrub_value(&ScrubValue::Int(7)), ScrubValue::Int(7));
        assert_eq!(
            scrubber().scrub_value(&ScrubValue::Double(3.5)),
            ScrubValue::Double(3.5)
        );
        assert_eq!(
            scrubber().scrub_value(&ScrubValue::Bool(true)),
            ScrubValue::Bool(true)
        );
    }

    #[test]
    fn scrubs_arrays_of_strings() {
        let value = ScrubValue::Array(vec![
            s("/Users/alice/x"),
            s("plain"),
            s("tok=secretsecretsecret123"),
        ]);
        let ScrubValue::Array(output) = scrubber().scrub_value(&value) else {
            panic!("expected array");
        };
        assert_eq!(output[0], s("/Users/<redacted>/x"));
        assert_eq!(output[1], s("plain"));
        // "tok" is not in the secret key set; token=/secret=/password= are.
        assert_eq!(output[2], s("tok=secretsecretsecret123"));
    }

    #[test]
    fn scrub_optional_nil_passes_through() {
        assert_eq!(scrubber().scrub_optional(None), None);
        assert_eq!(
            scrubber().scrub_optional(Some("/Users/lawrence/x")),
            Some("/Users/<redacted>/x".to_string())
        );
    }

    #[test]
    fn combined_secret_email_and_path_in_one_string() {
        let input =
            "user lawrence@cmux.com opened /Users/lawrence/secret.txt with token=abcdef0123456789zz";
        assert_eq!(
            scrubber().scrub(input),
            "user <redacted-email> opened /Users/<redacted>/secret.txt with token=<redacted-secret>"
        );
    }
}

// =============================================================================
// ScrubberDenylistsTests
// =============================================================================

mod denylists_tests {
    use super::*;

    /// Keys from sentry-python's denylists (and relay's sensitive-cookie list)
    /// that must be treated as sensitive dictionary keys.
    const SENSITIVE_KEYS: &[&str] = &[
        // core
        "password", "passwd", "secret", "api_key", "apikey", "auth",
        "credentials", "mysql_pwd", "privatekey", "private_key", "token",
        "session",
        // django / framework
        "csrftoken", "sessionid", "x_csrftoken", "set_cookie", "cookie",
        "authorization", "proxy-authorization", "x_api_key",
        // in the wild
        "aiohttp_session", "connect.sid", "csrf_token", "csrf", "_csrf",
        "_csrf_token", "PHPSESSID", "_session", "symfony", "user_session",
        "_xsrf", "XSRF-TOKEN",
        // PII (cmux runs sendDefaultPii = false)
        "x_forwarded_for", "x_real_ip", "ip_address", "remote_addr",
        // relay SENSITIVE_COOKIES aliases
        "sentrysid", "su", "fasthttpsessionid", "irissessionid", "_vercel_jwt",
        "fastcsrf", "_iris_csrf", "__session", "phpsessid",
    ];

    #[test]
    fn denylisted_key_is_treated_as_sensitive() {
        for &key in SENSITIVE_KEYS {
            assert!(
                SentryScrubber::is_sensitive_key(key),
                "expected '{key}' to be a sensitive key"
            );
            let output = scrubber().scrub_dictionary(&obj(vec![(key, s("plainvalue123notapattern"))]));
            assert_eq!(
                get(&output, key),
                Some(&s(REDACTED_SECRET)),
                "expected value under '{key}' to be redacted"
            );
        }
    }

    const NON_SENSITIVE_KEYS: &[&str] = &[
        "username", "count", "path", "inside", "aside", "presidency", "issue",
        "consumer", "describe",
    ];

    #[test]
    fn non_credential_key_is_not_sensitive() {
        for &key in NON_SENSITIVE_KEYS {
            assert!(
                !SentryScrubber::is_sensitive_key(key),
                "expected '{key}' to NOT be sensitive"
            );
        }
    }

    /// `(input, expected)` pairs for the value-regex layer.
    const VALUE_REDACTIONS: &[(&str, &str)] = &[
        // @pemkey — the whole PEM block is gone.
        (
            "key -----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEAsecretbodyabc/def+ghi==\n-----END RSA PRIVATE KEY----- done",
            "key <redacted-secret> done",
        ),
        (
            "-----BEGIN PRIVATE KEY-----\nMIIBVwIBADANBgkqhkiG9w0BAQEFAAS\n-----END PRIVATE KEY-----",
            "<redacted-secret>",
        ),
        (
            "-----BEGIN EC PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQ\n-----END EC PUBLIC KEY-----",
            "<redacted-secret>",
        ),
        // @creditcard.
        ("paid with 4111 1111 1111 1111 today", "paid with <redacted-secret> today"),
        ("amex 378282246310005 charged", "amex <redacted-secret> charged"),
        ("mc 5555-5555-5555-4444 ok", "mc <redacted-secret> ok"),
        // @iban.
        ("transfer to DE89370400440532013000 now", "transfer to <redacted-secret> now"),
        ("iban GB82WEST12345698765432 fine", "iban <redacted-secret> fine"),
        // @usssn.
        ("ssn 123-45-6789 on file", "ssn <redacted-secret> on file"),
        // @urlauth equivalent (cmux redactURLCredentials).
        (
            "git remote https://user:pass@github.com/x.git",
            "git remote https://<redacted-secret>@github.com/x.git",
        ),
        // @bearer.
        ("hdr Bearer abc123DEF456ghi789xyz end", "hdr Bearer <redacted-secret> end"),
        // @password key=value family.
        ("the_password=hunter2hunter2 set", "the_password=<redacted-secret> set"),
        ("api_key=plainlettersvalue123 used", "api_key=<redacted-secret> used"),
        (
            "connecting with mysql_pwd=rootpassword123",
            "connecting with mysql_pwd=<redacted-secret>",
        ),
        // cmux-original provider key / JWT / AWS.
        ("call sk-proj-abcdef0123456789ABCDEF now", "call <redacted-secret> now"),
        (
            "clone ghp_0123456789abcdefABCDEF0123456789abcd here",
            "clone <redacted-secret> here",
        ),
        ("creds AKIAIOSFODNN7EXAMPLE rejected", "creds <redacted-secret> rejected"),
        // Quoted JSON values whose secret contains a delimiter.
        (r#"{"password":"abc&def"}"#, r#"{"password":"<redacted-secret>"}"#),
        (r#"{"token":"a,b,c"}"#, r#"{"token":"<redacted-secret>"}"#),
        (r#"{"api_key":"k&v}x"}"#, r#"{"api_key":"<redacted-secret>"}"#),
        (r#"cookie="ab&cd""#, r#"cookie="<redacted-secret>""#),
        // Unquoted value: the delimiter bounds the value.
        ("GET /x?token=supersecret123&page=2", "GET /x?token=<redacted-secret>&page=2"),
        ("env TOKEN=plainvalue,KEEP=2", "env TOKEN=<redacted-secret>,KEEP=2"),
    ];

    #[test]
    fn value_pattern_redacts_secret() {
        for &(input, expected) in VALUE_REDACTIONS {
            assert_eq!(scrubber().scrub(input), expected, "input: {input}");
        }
    }

    #[test]
    fn pem_key_body_does_not_survive() {
        let body = "MIIEpAIBAAKCAQEAsecretkeymaterialdoesnotleak";
        let input = format!(
            "leak? -----BEGIN RSA PRIVATE KEY-----\n{body}\n-----END RSA PRIVATE KEY-----"
        );
        let output = scrubber().scrub(&input);
        assert!(!output.contains(body), "PEM body leaked: {output}");
        assert!(
            !output.contains("BEGIN RSA PRIVATE KEY"),
            "PEM header leaked: {output}"
        );
        assert!(output.contains("<redacted-secret>"));
    }

    // MARK: - Negative fixtures: must NOT over-redact

    const PRESERVED_STRINGS: &[&str] = &[
        "workspace 550e8400-e29b-41d4-a716-446655440000 ready",
        "surface 123e4567-e89b-12d3-a456-426614174000 attached",
        "cmux DEV build 1234567890 v2",
        "version 0.64.13 (build 4521)",
        "at GhosttyTerminalView.forceRefresh() line 142 in frame 7",
        "Fatal error: Index out of range while reading buffer",
        "code=42 status=ok retry=true id=ABC123",
    ];

    #[test]
    fn preserves_non_secret_string() {
        for &input in PRESERVED_STRINGS {
            assert_eq!(scrubber().scrub(input), input, "input: {input}");
        }
    }
}
