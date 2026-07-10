//! HMAC-SHA256 approval signatures.
//!
//! Ported from `SurfaceResumeApprovalSignature` in
//! `Sources/SessionPersistence.swift:698-708`. Swift uses CryptoKit
//! `HMAC<SHA256>` + `Data.base64EncodedString()`; this port uses the `hmac` +
//! `sha2` crates with `base64` standard encoding to produce a byte-identical
//! signature string for the same payload and secret.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Namespace type mirroring the Swift `enum SurfaceResumeApprovalSignature`.
pub struct SurfaceResumeApprovalSignature;

impl SurfaceResumeApprovalSignature {
    /// Computes the base64-encoded HMAC-SHA256 of `payload` under `secret`.
    ///
    /// Mirrors `SurfaceResumeApprovalSignature.sign(_:secret:)`.
    pub fn sign(payload: &[u8], secret: &[u8]) -> String {
        // HMAC accepts a key of any length; `new_from_slice` never errors.
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
        mac.update(payload);
        let code = mac.finalize().into_bytes();
        STANDARD.encode(code)
    }

    /// Constant-time verification: recomputes the signature and compares it to
    /// `signature` without early-exit timing leaks.
    ///
    /// Swift compares the recomputed base64 string with `==`; this port keeps
    /// the same recompute-then-compare shape but uses a constant-time byte
    /// comparison as the crate's security seam.
    pub fn verify(payload: &[u8], secret: &[u8], signature: &str) -> bool {
        let expected = Self::sign(payload, secret);
        constant_time_eq(expected.as_bytes(), signature.as_bytes())
    }
}

/// Length-then-content constant-time equality. The recomputed signature is a
/// fixed-length base64 string, so the length branch reveals nothing sensitive.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_pins_golden_base64_vector() {
        // HMAC-SHA256(key="secret", msg="payload") base64, computed independently
        // via `openssl dgst -sha256 -hmac secret`. Pins the exact byte layout so
        // any drift in the HMAC/base64 pipeline is caught.
        let sig = SurfaceResumeApprovalSignature::sign(b"payload", b"secret");
        assert_eq!(sig, "uC/LeRrOxXhZuYm0MKgmSIzi5Hn9+SMmvQoug3WkK6Q=");
    }

    #[test]
    fn sign_verify_round_trip() {
        let sig = SurfaceResumeApprovalSignature::sign(b"the-payload", b"the-secret");
        assert!(SurfaceResumeApprovalSignature::verify(
            b"the-payload",
            b"the-secret",
            &sig
        ));
    }

    #[test]
    fn verify_rejects_tampered_payload() {
        let sig = SurfaceResumeApprovalSignature::sign(b"the-payload", b"the-secret");
        assert!(!SurfaceResumeApprovalSignature::verify(
            b"the-payload-x",
            b"the-secret",
            &sig
        ));
    }

    #[test]
    fn verify_rejects_wrong_secret() {
        let sig = SurfaceResumeApprovalSignature::sign(b"the-payload", b"the-secret");
        assert!(!SurfaceResumeApprovalSignature::verify(
            b"the-payload",
            b"other-secret",
            &sig
        ));
    }

    #[test]
    fn verify_rejects_wrong_length_signature() {
        assert!(!SurfaceResumeApprovalSignature::verify(b"p", b"s", "short"));
    }
}
