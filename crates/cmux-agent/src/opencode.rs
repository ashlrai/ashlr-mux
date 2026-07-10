//! OpenCode HTTP-loopback authorization (M3 WS3).
//!
//! Port of `OpenCodeServerAuth` (`Sources/Panels/AgentSessionStreamAccumulators.swift:3-16`).
//! The OpenCode `serve` HTTP API is protected with HTTP Basic auth using the
//! credentials cmux mints into the launch env (`OPENCODE_SERVER_USERNAME` /
//! `OPENCODE_SERVER_PASSWORD`, see `AgentSessionLaunchPlan`). The transport layer
//! (`cmux-process`) scrapes the loopback URL from stdout; this computes the
//! matching `Authorization` header value, which the caller passes to the HTTP
//! client (kept here, in the env-owning crate, so `cmux-process` need not depend
//! on `cmux-agent`).

use std::collections::BTreeMap;

/// Basic-auth credentials for the OpenCode loopback server, derived from the
/// launch environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeServerAuth {
    /// The full `Authorization` header value, e.g. `Basic b3BlbmNvZGU6c2VjcmV0`.
    pub authorization_header: String,
}

impl OpenCodeServerAuth {
    /// Build the auth from a launch environment, or `None` when no (non-empty)
    /// `OPENCODE_SERVER_PASSWORD` is present. Username defaults to `opencode`
    /// when absent/empty, matching the Swift original.
    pub fn from_environment(environment: &BTreeMap<String, String>) -> Option<Self> {
        let password = environment
            .get("OPENCODE_SERVER_PASSWORD")
            .filter(|value| !value.is_empty())?;
        let username = environment
            .get("OPENCODE_SERVER_USERNAME")
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("opencode");
        let token = format!("{username}:{password}");
        Some(Self {
            authorization_header: format!("Basic {}", base64_encode(token.as_bytes())),
        })
    }
}

/// Standard Base64 (RFC 4648, `+/` alphabet, `=` padding). Small, dependency-free
/// encoder — the only input is a short `user:pass` token.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 test vectors + the padding cases.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn builds_basic_header_with_default_username() {
        let auth =
            OpenCodeServerAuth::from_environment(&env(&[("OPENCODE_SERVER_PASSWORD", "secret")]))
                .expect("auth");
        // base64("opencode:secret")
        assert_eq!(
            auth.authorization_header,
            format!("Basic {}", base64_encode(b"opencode:secret"))
        );
    }

    #[test]
    fn honors_explicit_username() {
        let auth = OpenCodeServerAuth::from_environment(&env(&[
            ("OPENCODE_SERVER_USERNAME", "alice"),
            ("OPENCODE_SERVER_PASSWORD", "pw"),
        ]))
        .expect("auth");
        assert_eq!(
            auth.authorization_header,
            format!("Basic {}", base64_encode(b"alice:pw"))
        );
    }

    #[test]
    fn empty_username_falls_back_to_default() {
        let auth = OpenCodeServerAuth::from_environment(&env(&[
            ("OPENCODE_SERVER_USERNAME", ""),
            ("OPENCODE_SERVER_PASSWORD", "pw"),
        ]))
        .expect("auth");
        assert_eq!(
            auth.authorization_header,
            format!("Basic {}", base64_encode(b"opencode:pw"))
        );
    }

    #[test]
    fn no_password_yields_none() {
        assert!(OpenCodeServerAuth::from_environment(&env(&[])).is_none());
        assert!(
            OpenCodeServerAuth::from_environment(&env(&[("OPENCODE_SERVER_PASSWORD", "")]))
                .is_none()
        );
    }
}
