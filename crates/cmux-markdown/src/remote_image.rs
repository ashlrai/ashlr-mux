//! Port of `MarkdownRemoteImageSecurity` (`Sources/Panels/MarkdownRemoteImageLoader.swift`)
//! — the pure, headless security gate for loading remote images in the markdown
//! viewer. The markdown viewer only fetches a remote image after this gate
//! admits its URL, so this is an SSRF boundary: it enforces HTTPS-only, no URL
//! userinfo, the default port, a hostname/literal allowlist that rejects
//! loopback / private / link-local / CGNAT / reserved address ranges, and an
//! image MIME allowlist. It also builds the (deterministic) HTTP request line
//! and guards header values against injection.
//!
//! What is intentionally NOT ported (network / platform I/O — lives in the host
//! layer): DNS resolution (`resolvedAllowedEndpoints` / `getaddrinfo`), the
//! `Network.framework` endpoint construction, and the async TLS fetch. The pure
//! per-address classifier the resolver needs is exposed as
//! [`is_allowed_resolved_ip`] so the host layer applies the identical jail to
//! every address a hostname resolves to.
//!
//! ## Documented divergences from the canonical Swift
//! - **URL parsing uses the WHATWG `url` crate**, not Foundation
//!   `URL`/`URLComponents`. The security predicates are ported byte-identically;
//!   only the string→components parser differs. This is *stricter* against SSRF:
//!   the WHATWG parser classifies decimal/hex/octal IPv4 forms (`http://2130706433/`,
//!   `http://0x7f.1/`) as IP literals, which Swift's `inet_pton`-only check treats
//!   as opaque hostnames and would wrongly allow. Here they are classified and
//!   the private/reserved ones are rejected.
//! - IP literals are classified via [`url::Host`] (which yields
//!   `std::net::Ipv4Addr`/`Ipv6Addr` directly) instead of re-running `inet_pton`
//!   over a normalized host string — same admit/reject outcome for genuine
//!   literals, plus the strictness above.

use std::net::IpAddr;
use url::{Host, Url};

/// The custom URL scheme the viewer uses for remote-image requests, matching
/// `MarkdownWebRenderer.remoteImageURLScheme`.
pub const REMOTE_IMAGE_URL_SCHEME: &str = "cmux-remote-image";

/// The maximum remote image size the viewer will accept, matching
/// `MarkdownRemoteImageSecurity.maximumRemoteImageBytes` (8 MiB).
pub const MAXIMUM_REMOTE_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// Extract and validate the remote image URL from a `cmux-remote-image://…?url=<https-url>`
/// request, returning the admitted URL string or `None` (port of
/// `remoteImageURL(from:)`). The gate is the non-resolving
/// [`is_potentially_safe_remote_image_url`].
pub fn remote_image_url(request_url: &str) -> Option<String> {
    let request = Url::parse(request_url).ok()?;
    if request.scheme() != REMOTE_IMAGE_URL_SCHEME {
        return None;
    }
    let raw_remote_url = request
        .query_pairs()
        .find(|(name, _)| name == "url")
        .map(|(_, value)| value.into_owned())?;
    let remote = Url::parse(&raw_remote_url).ok()?;
    if is_potentially_safe_remote_image_url(&remote) {
        Some(remote.to_string())
    } else {
        None
    }
}

/// The non-resolving safety gate (port of `isPotentiallySafeRemoteImageURL` =
/// `isSafeRemoteImageURL(resolveHost: false)`): HTTPS-only, no userinfo, default
/// port, and an allowed hostname or non-private IP literal. The resolving branch
/// (`resolveHost: true`) is network I/O and lives in the host layer, which must
/// additionally screen every resolved address with [`is_allowed_resolved_ip`].
pub fn is_potentially_safe_remote_image_url(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    // Swift allows `port == nil || port == 443`. The WHATWG parser normalizes the
    // default 443 away (returns None), so a present port is always non-default.
    if url.port().is_some_and(|p| p != 443) {
        return false;
    }
    match url.host() {
        Some(Host::Ipv4(addr)) => is_allowed_ipv4(addr.octets()),
        Some(Host::Ipv6(addr)) => is_allowed_ipv6(addr.octets()),
        Some(Host::Domain(domain)) => is_allowed_host_name(domain),
        None => false,
    }
}

/// Whether a hostname (not an IP literal) is allowed: non-empty and not a
/// loopback-ish name (`localhost`, `*.localhost`, `local`, `*.local`). Port of
/// the domain arm of `isAllowedHostNameOrLiteral`.
fn is_allowed_host_name(domain: &str) -> bool {
    let host = normalized_remote_image_host(domain);
    if host.is_empty() {
        return false;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    if host == "local" || host.ends_with(".local") {
        return false;
    }
    true
}

/// Classify a resolved IP address with the same jail applied to IP literals, so
/// the DNS-resolving host layer can reject a hostname that resolves into a
/// private/reserved range. Port of the per-address checks inside
/// `resolvedAllowedEndpoints`.
pub fn is_allowed_resolved_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => is_allowed_ipv4(addr.octets()),
        IpAddr::V6(addr) => is_allowed_ipv6(addr.octets()),
    }
}

/// Port of `isAllowedIPv4Address`: reject `0/8`, `10/8`, `100.64/10` (CGNAT),
/// `127/8`, `169.254/16` (link-local), `172.16/12`, `192.0/16`, `192.168/16`,
/// `198.18/15` (benchmarking), and `>= 224` (multicast + reserved).
fn is_allowed_ipv4(octets: [u8; 4]) -> bool {
    let (first, second) = (octets[0], octets[1]);
    if first == 0 {
        return false;
    }
    if first == 10 {
        return false;
    }
    if first == 100 && (64..=127).contains(&second) {
        return false;
    }
    if first == 127 {
        return false;
    }
    if first == 169 && second == 254 {
        return false;
    }
    if first == 172 && (16..=31).contains(&second) {
        return false;
    }
    if first == 192 && second == 0 {
        return false;
    }
    if first == 192 && second == 168 {
        return false;
    }
    if first == 198 && (18..=19).contains(&second) {
        return false;
    }
    if first >= 224 {
        return false;
    }
    true
}

/// Port of `isAllowedIPv6Address`: reject `::`, `::1`, `fc00::/7` (ULA),
/// `fe80::/10` (link-local), `fec0::/10`, `ff00::/8` (multicast), `::/96`, and
/// delegate `::ffff:0:0/96` (v4-mapped) to the IPv4 rule. Check order matters and
/// mirrors Swift exactly.
fn is_allowed_ipv6(bytes: [u8; 16]) -> bool {
    if bytes.iter().all(|&b| b == 0) {
        return false; // ::
    }
    if bytes[..15].iter().all(|&b| b == 0) && bytes[15] == 1 {
        return false; // ::1
    }
    if bytes[0] & 0xfe == 0xfc {
        return false; // fc00::/7 unique-local
    }
    if bytes[0] == 0xfe && (bytes[1] & 0xc0) == 0x80 {
        return false; // fe80::/10 link-local
    }
    if bytes[0] == 0xfe && (bytes[1] & 0xc0) == 0xc0 {
        return false; // fec0::/10 site-local (deprecated)
    }
    if bytes[0] == 0xff {
        return false; // ff00::/8 multicast
    }
    if bytes[..12].iter().all(|&b| b == 0) {
        return false; // ::/96 (incl. IPv4-compatible)
    }
    if bytes[..10].iter().all(|&b| b == 0) && bytes[10] == 0xff && bytes[11] == 0xff {
        // ::ffff:0:0/96 IPv4-mapped — classify the embedded IPv4 address.
        return is_allowed_ipv4([bytes[12], bytes[13], bytes[14], bytes[15]]);
    }
    true
}

/// The host to show in the remote-image consent prompt (and to compare a
/// redirect target against), or `None` if the URL is not admissible. Port of
/// `remoteImageConsentHost`.
pub fn remote_image_consent_host(url: &Url) -> Option<String> {
    if !is_potentially_safe_remote_image_url(url) {
        return None;
    }
    let host = normalized_remote_image_host(&host_string(url)?);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// Canonicalize a response `Content-Type` to an allowed image MIME, or `None`
/// (port of `canonicalImageMIMEType`). Unlike the local-image jail, the remote
/// allowlist includes `image/svg+xml`.
pub fn canonical_image_mime_type(raw: Option<&str>) -> Option<String> {
    let mime = raw
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => Some("image/png".to_string()),
        "image/jpeg" | "image/jpg" => Some("image/jpeg".to_string()),
        "image/gif" => Some("image/gif".to_string()),
        "image/webp" => Some("image/webp".to_string()),
        "image/avif" => Some("image/avif".to_string()),
        "image/svg+xml" => Some("image/svg+xml".to_string()),
        _ => None,
    }
}

/// The `path?query` request target for the URL, defaulting the path to `/`
/// (port of `pathAndQuery`).
pub fn path_and_query(url: &Url) -> String {
    let path = url.path();
    let mut value = if path.is_empty() { "/" } else { path }.to_string();
    if let Some(query) = url.query() {
        if !query.is_empty() {
            value.push('?');
            value.push_str(query);
        }
    }
    value
}

/// Build the raw HTTP/1.1 GET request bytes for `url` with the given `host`, or
/// `None` if the host is not a safe header value (port of `requestBytes`).
pub fn request_bytes(url: &Url, host: &str) -> Option<Vec<u8>> {
    let host_header = http_host_header_value(host)?;
    let request = [
        format!("GET {} HTTP/1.1", path_and_query(url)),
        format!("Host: {host_header}"),
        "Accept: image/png,image/jpeg,image/gif,image/webp,image/avif;q=0.9,image/svg+xml;q=0.9,*/*;q=0.1".to_string(),
        "User-Agent: cmux-markdown-image-loader".to_string(),
        "Connection: close".to_string(),
        String::new(),
        String::new(),
    ]
    .join("\r\n");
    Some(request.into_bytes())
}

/// Normalize a host: strip surrounding brackets/whitespace, then trailing dots,
/// then lowercase (port of `normalizedRemoteImageHost`).
fn normalized_remote_image_host(raw_host: &str) -> String {
    raw_host
        .trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace())
        .trim_matches('.')
        .to_ascii_lowercase()
}

/// The host of `url` as a string (domain or literal, without IPv6 brackets).
fn host_string(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(d) => Some(d.to_string()),
        Host::Ipv4(a) => Some(a.to_string()),
        Host::Ipv6(a) => Some(a.to_string()),
    }
}

/// Whether every byte of `value` is a safe HTTP header character (`0x21..=0xff`,
/// excluding DEL) — an injection guard (port of `isSafeHTTPHeaderValue`).
fn is_safe_http_header_value(value: &str) -> bool {
    value.bytes().all(|byte| byte >= 0x21 && byte != 0x7f)
}

/// The `Host:` header value for `raw_host`, bracketing IPv6 literals, or `None`
/// if unsafe (port of `httpHostHeaderValue`).
fn http_host_header_value(raw_host: &str) -> Option<String> {
    let host = normalized_remote_image_host(raw_host);
    if !is_safe_http_header_value(&host) {
        return None;
    }
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        Some(format!("[{host}]"))
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    // --- scheme gate / extraction --------------------------------------------

    #[test]
    fn scheme_gate_extracts_allowed_url() {
        let req = "cmux-remote-image://image?url=https%3A%2F%2Fexample.com%2Fa.png";
        assert_eq!(
            remote_image_url(req).as_deref(),
            Some("https://example.com/a.png")
        );
    }

    #[test]
    fn wrong_scheme_rejected() {
        assert_eq!(
            remote_image_url("https://image?url=https%3A%2F%2Fexample.com%2Fa.png"),
            None
        );
    }

    #[test]
    fn missing_url_query_rejected() {
        assert_eq!(remote_image_url("cmux-remote-image://image?other=1"), None);
    }

    #[test]
    fn extraction_rejects_blocked_inner_url() {
        let req = "cmux-remote-image://image?url=http%3A%2F%2Fexample.com%2Fa.png";
        assert_eq!(remote_image_url(req), None);
    }

    // --- https-only / userinfo / port ----------------------------------------

    #[test]
    fn https_only() {
        assert!(!is_potentially_safe_remote_image_url(&parse("http://example.com/a.png")));
        assert!(is_potentially_safe_remote_image_url(&parse("https://example.com/a.png")));
    }

    #[test]
    fn userinfo_rejected() {
        assert!(!is_potentially_safe_remote_image_url(&parse("https://user@example.com/x")));
        assert!(!is_potentially_safe_remote_image_url(&parse("https://user:pass@example.com/x")));
    }

    #[test]
    fn nondefault_port_rejected_default_allowed() {
        assert!(!is_potentially_safe_remote_image_url(&parse("https://example.com:8443/x")));
        assert!(is_potentially_safe_remote_image_url(&parse("https://example.com:443/x")));
        assert!(is_potentially_safe_remote_image_url(&parse("https://example.com/x")));
    }

    // --- host literal blocklist ----------------------------------------------

    #[test]
    fn loopback_names_rejected() {
        for h in ["https://localhost/x", "https://foo.localhost/x", "https://box.local/x"] {
            assert!(!is_potentially_safe_remote_image_url(&parse(h)), "{h}");
        }
        assert!(is_potentially_safe_remote_image_url(&parse("https://example.com/x")));
    }

    #[test]
    fn host_normalization_case_and_trailing_dot() {
        assert!(is_potentially_safe_remote_image_url(&parse("https://EXAMPLE.com/x")));
        // A trailing-dot FQDN is still a normal domain.
        assert!(is_potentially_safe_remote_image_url(&parse("https://example.com./x")));
    }

    // --- IPv4 blocklist ------------------------------------------------------

    #[test]
    fn ipv4_blocklist() {
        for blocked in [
            [0, 0, 0, 0], [10, 1, 2, 3], [100, 64, 0, 1], [100, 127, 255, 255],
            [127, 0, 0, 1], [169, 254, 1, 1], [172, 16, 0, 1], [172, 31, 255, 255],
            [192, 0, 0, 1], [192, 168, 1, 1], [198, 18, 0, 1], [198, 19, 255, 255],
            [224, 0, 0, 1], [255, 255, 255, 255],
        ] {
            assert!(!is_allowed_ipv4(blocked), "{blocked:?} should be blocked");
        }
        for allowed in [
            [8, 8, 8, 8], [1, 1, 1, 1], [100, 63, 255, 255], [100, 128, 0, 1],
            [172, 15, 0, 1], [172, 32, 0, 1], [198, 17, 0, 1], [198, 20, 0, 1],
            [223, 255, 255, 255],
        ] {
            assert!(is_allowed_ipv4(allowed), "{allowed:?} should be allowed");
        }
    }

    #[test]
    fn ipv4_literal_url_blocked() {
        assert!(!is_potentially_safe_remote_image_url(&parse("https://127.0.0.1/x")));
        assert!(!is_potentially_safe_remote_image_url(&parse("https://10.0.0.1/x")));
        assert!(is_potentially_safe_remote_image_url(&parse("https://8.8.8.8/x")));
    }

    #[test]
    fn decimal_ipv4_form_is_classified_not_bypassed() {
        // WHATWG parses 2130706433 as 127.0.0.1 — must be blocked (Swift's
        // inet_pton would miss this; documented security-improving divergence).
        assert!(!is_potentially_safe_remote_image_url(&parse("https://2130706433/x")));
    }

    // --- IPv6 blocklist ------------------------------------------------------

    #[test]
    fn ipv6_literal_url_blocklist() {
        for blocked in [
            "https://[::]/x", "https://[::1]/x", "https://[fc00::1]/x", "https://[fdff::1]/x",
            "https://[fe80::1]/x", "https://[fec0::1]/x", "https://[ff02::1]/x",
            "https://[::ffff:127.0.0.1]/x", "https://[::1.2.3.4]/x",
        ] {
            assert!(!is_potentially_safe_remote_image_url(&parse(blocked)), "{blocked}");
        }
        for allowed in ["https://[2606:4700:4700::1111]/x", "https://[::ffff:8.8.8.8]/x"] {
            assert!(is_potentially_safe_remote_image_url(&parse(allowed)), "{allowed}");
        }
    }

    // --- consent host --------------------------------------------------------

    #[test]
    fn consent_host() {
        assert_eq!(
            remote_image_consent_host(&parse("https://EXAMPLE.com/x")).as_deref(),
            Some("example.com")
        );
        assert_eq!(remote_image_consent_host(&parse("http://example.com/x")), None);
    }

    // --- MIME allowlist ------------------------------------------------------

    #[test]
    fn mime_allowlist() {
        assert_eq!(canonical_image_mime_type(Some("image/png")).as_deref(), Some("image/png"));
        assert_eq!(canonical_image_mime_type(Some("image/jpg")).as_deref(), Some("image/jpeg"));
        assert_eq!(canonical_image_mime_type(Some("image/jpeg")).as_deref(), Some("image/jpeg"));
        assert_eq!(
            canonical_image_mime_type(Some("IMAGE/PNG; charset=binary")).as_deref(),
            Some("image/png")
        );
        assert_eq!(canonical_image_mime_type(Some("image/svg+xml")).as_deref(), Some("image/svg+xml"));
        assert_eq!(canonical_image_mime_type(Some("image/gif")).as_deref(), Some("image/gif"));
        assert_eq!(canonical_image_mime_type(Some("text/html")), None);
        assert_eq!(canonical_image_mime_type(None), None);
    }

    // --- request framing / header safety -------------------------------------

    #[test]
    fn path_and_query_defaults_to_slash() {
        assert_eq!(path_and_query(&parse("https://example.com")), "/");
        assert_eq!(path_and_query(&parse("https://example.com/a/b.png")), "/a/b.png");
        assert_eq!(path_and_query(&parse("https://example.com/a?x=1&y=2")), "/a?x=1&y=2");
    }

    #[test]
    fn request_bytes_frames_a_get() {
        let bytes = request_bytes(&parse("https://example.com/a.png"), "example.com").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("GET /a.png HTTP/1.1\r\n"));
        assert!(text.contains("\r\nHost: example.com\r\n"));
        assert!(text.contains("\r\nConnection: close\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn request_bytes_brackets_ipv6_host() {
        let bytes = request_bytes(&parse("https://[2606:4700::1111]/a.png"), "2606:4700::1111").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("\r\nHost: [2606:4700::1111]\r\n"), "{text}");
    }

    #[test]
    fn unsafe_header_host_rejected() {
        // A space / control char in the host is an injection attempt.
        assert!(request_bytes(&parse("https://example.com/a.png"), "evil host").is_none());
    }

    #[test]
    fn resolved_ip_screening() {
        assert!(is_allowed_resolved_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_allowed_resolved_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_allowed_resolved_ip("::1".parse().unwrap()));
        assert!(is_allowed_resolved_ip("2606:4700:4700::1111".parse().unwrap()));
    }
}
