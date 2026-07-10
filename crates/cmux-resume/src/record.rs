//! The signed approval record and its trust primitives.
//!
//! Ported from `SurfaceResumeApprovalPolicy` (`SessionPersistence.swift:262-266`)
//! and `SurfaceResumeApprovalRecord` (`SessionPersistence.swift:491-634`): the
//! Codable record, its designated-init normalization, the golden-byte signing
//! payload, sign/verify, and longest-prefix binding matching.

use std::collections::{BTreeSet, HashMap};

use cmux_tmux::SurfaceResumeBindingSnapshot;

use crate::canonicalizer;
use crate::signature::SurfaceResumeApprovalSignature;

/// The trust policy attached to an approval record.
///
/// Ported from `enum SurfaceResumeApprovalPolicy: String`
/// (`SessionPersistence.swift:262-266`). The `String` raw values (`manual`,
/// `prompt`, `auto`) are wire- and signature-critical.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceResumeApprovalPolicy {
    Manual,
    Prompt,
    Auto,
}

impl SurfaceResumeApprovalPolicy {
    /// The Swift `rawValue` string used inside the signing payload.
    pub fn raw_value(self) -> &'static str {
        match self {
            SurfaceResumeApprovalPolicy::Manual => "manual",
            SurfaceResumeApprovalPolicy::Prompt => "prompt",
            SurfaceResumeApprovalPolicy::Auto => "auto",
        }
    }
}

/// A signed record authorizing a resume command prefix under a trust policy.
///
/// Ported from `struct SurfaceResumeApprovalRecord: Codable, Equatable`
/// (`SessionPersistence.swift:491-634`).
///
// Serde parity with Swift's synthesized `Codable`: optional fields are OMITTED
// when nil (`encodeIfPresent`) and default to `None`/`[]` when absent on decode
// (`decodeIfPresent`). Unlike `SurfaceResumeBindingSnapshot`, this struct has NO
// custom `init(from:)` in Swift — synthesized `Codable` decodes stored
// properties directly WITHOUT re-running the designated initializer, so decode
// does NOT re-normalize. Only the [`SurfaceResumeApprovalRecord::new`]
// constructor normalizes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceResumeApprovalRecord {
    pub version: i64,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "commandPrefix")]
    pub command_prefix: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<HashMap<String, String>>,
    #[serde(rename = "environmentKeys")]
    pub environment_keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub policy: SurfaceResumeApprovalPolicy,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
    #[serde(rename = "updatedAt")]
    pub updated_at: f64,
    #[serde(rename = "lastUsedAt", skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl SurfaceResumeApprovalRecord {
    /// Designated initializer, reproducing the Swift `init(...)`
    /// (`SessionPersistence.swift:506-533`) normalization exactly: `version` is
    /// forced to `1`, string fields are trimmed/nil'd, `commandPrefix` drops
    /// empty tokens, `cwd` is lexically normalized, `environment` is filtered,
    /// and `environmentKeys` is the sorted union of explicit + derived keys.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: Option<&str>,
        command_prefix: Vec<String>,
        cwd: Option<&str>,
        environment: Option<HashMap<String, String>>,
        environment_keys: Vec<String>,
        source: Option<&str>,
        policy: SurfaceResumeApprovalPolicy,
        created_at: f64,
        updated_at: f64,
        last_used_at: Option<f64>,
        signature: Option<&str>,
    ) -> Self {
        let environment = normalized_environment(environment);
        let environment_keys = normalized_environment_keys(&environment_keys, environment.as_ref());
        Self {
            version: 1,
            id,
            name: normalized(name),
            command_prefix: command_prefix
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect(),
            cwd: canonicalizer::normalized_cwd(cwd),
            environment,
            environment_keys,
            source: normalized(source),
            policy,
            created_at,
            updated_at,
            last_used_at,
            signature: normalized(signature),
        }
    }

    /// `commandPrefix` rendered as a shell-quoted command line.
    /// Mirrors Swift `commandPrefixText` (`:535-537`).
    pub fn command_prefix_text(&self) -> String {
        self.command_prefix
            .iter()
            .map(|s| canonicalizer::shell_quoted(s))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Whether this record authorizes `binding`: a longest-prefix token match
    /// over the binding's command, plus cwd and full-environment equality.
    /// Mirrors Swift `matches(_:)` (`:539-556`).
    pub fn matches(&self, binding: &SurfaceResumeBindingSnapshot) -> bool {
        if self.command_prefix.is_empty() {
            return false;
        }
        let tokens = match canonicalizer::tokens(&binding.command) {
            Some(t) => t,
            None => return false,
        };
        if tokens.len() < self.command_prefix.len() {
            return false;
        }
        if tokens[..self.command_prefix.len()] != self.command_prefix[..] {
            return false;
        }
        if let Some(cwd) = &self.cwd {
            if canonicalizer::normalized_cwd(binding.cwd.as_deref()).as_deref()
                != Some(cwd.as_str())
            {
                return false;
            }
        }
        let empty = HashMap::new();
        let binding_environment = binding.environment.as_ref().unwrap_or(&empty);
        match &self.environment {
            Some(environment) if !environment.is_empty() => binding_environment == environment,
            _ => binding_environment.is_empty(),
        }
    }

    /// The exact byte payload that is HMAC-signed.
    ///
    /// GOLDEN-BYTE CRITICAL. Faithful port of `signingPayloadData()`
    /// (`SessionPersistence.swift:558-588`): field order, per-field base64
    /// encoding, sorted environment keys, and `TimeInterval` (Double) → String
    /// formatting must match Swift byte-for-byte or cross-platform signatures
    /// will not validate.
    pub fn signing_payload_data(&self) -> Vec<u8> {
        let encoded_prefix = self
            .command_prefix
            .iter()
            .map(|s| base64_encode(s))
            .collect::<Vec<_>>()
            .join(",");
        let encoded_environment_keys = self
            .environment_keys
            .iter()
            .map(|s| base64_encode(s))
            .collect::<Vec<_>>()
            .join(",");
        let encoded_environment = {
            let empty = HashMap::new();
            let environment = self.environment.as_ref().unwrap_or(&empty);
            let mut keys: Vec<&String> = environment.keys().collect();
            keys.sort();
            keys.iter()
                .map(|key| {
                    let value = environment.get(*key).map(|s| s.as_str()).unwrap_or("");
                    format!("{}={}", base64_encode(key), base64_encode(value))
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        let fields = [
            format!("version={}", self.version),
            format!("id={}", self.id),
            format!(
                "name={}",
                self.name.as_deref().map(base64_encode).unwrap_or_default()
            ),
            format!("commandPrefix={encoded_prefix}"),
            format!(
                "cwd={}",
                self.cwd.as_deref().map(base64_encode).unwrap_or_default()
            ),
            format!("environment={encoded_environment}"),
            format!("environmentKeys={encoded_environment_keys}"),
            format!(
                "source={}",
                self.source
                    .as_deref()
                    .map(base64_encode)
                    .unwrap_or_default()
            ),
            format!("policy={}", self.policy.raw_value()),
            format!("createdAt={}", swift_double_string(self.created_at)),
            format!("updatedAt={}", swift_double_string(self.updated_at)),
            format!(
                "lastUsedAt={}",
                self.last_used_at
                    .map(swift_double_string)
                    .unwrap_or_default()
            ),
        ];
        fields.join("\n").into_bytes()
    }

    /// Returns a copy with a freshly computed signature.
    /// Mirrors Swift `signed(secret:)` (`:590-594`).
    pub fn signed(&self, secret: &[u8]) -> Self {
        let mut copy = self.clone();
        copy.signature = Some(SurfaceResumeApprovalSignature::sign(
            &self.signing_payload_data(),
            secret,
        ));
        copy
    }

    /// Whether the stored signature matches a recomputation under `secret`.
    /// Mirrors Swift `hasValidSignature(secret:)` (`:596-599`).
    pub fn has_valid_signature(&self, secret: &[u8]) -> bool {
        match &self.signature {
            None => false,
            Some(signature) => SurfaceResumeApprovalSignature::verify(
                &self.signing_payload_data(),
                secret,
                signature,
            ),
        }
    }
}

/// Swift `Data($0.utf8).base64EncodedString()` for a `&str`.
fn base64_encode(value: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

/// Formats an `f64` the way Swift renders a `TimeInterval` (`Double`) via string
/// interpolation / `String(_:)`.
///
// DIVERGENCE: Swift's `Double` description always includes a decimal point for
// integral values (`1000.0`, not `1000`) and uses the shortest round-trippable
// digits otherwise. Rust's `{}` omits the trailing `.0` for integral floats and
// otherwise also emits shortest round-trippable digits, so appending `.0` when
// no `.`/`e` is present reproduces Swift for the moderate-magnitude,
// non-exponential timestamps this subsystem stores. The two formatters can
// still disagree for magnitudes large/small enough to trigger exponent notation
// (`1e+20` vs `1e20` style), which resume timestamps never reach.
fn swift_double_string(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    let rendered = format!("{value}");
    if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
        rendered
    } else {
        format!("{rendered}.0")
    }
}

/// Swift `normalized(_:)` (`:601-607`): trim whitespace/newlines, nil if empty.
fn normalized(raw_value: Option<&str>) -> Option<String> {
    let raw = raw_value?;
    let trimmed = raw.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Swift record `normalizedEnvironment(_:)` (`:609-618`).
///
// DIVERGENCE (from the lane spec's prose, NOT from Swift): the spec text said
// the record's `normalizedEnvironment` drops sensitive keys via
// `isSensitiveEnvironmentKey`, citing `:447-482`. That is the *snapshot*'s
// filter (`SurfaceResumeBindingSnapshot`, in cmux-tmux). The RECORD's own
// `normalizedEnvironment` at `:609-618` only drops empty keys and
// non-printable-value entries and DELIBERATELY keeps sensitive keys — because
// the environment reaching a record has already been sensitive-filtered by the
// snapshot upstream. This port matches the actual Swift record code (no
// sensitive-key drop here) to stay faithful.
fn normalized_environment(
    environment: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    let environment = environment?;
    let mut result: HashMap<String, String> = HashMap::new();
    for (key, value) in environment {
        let trimmed_key = key.trim_matches(|c: char| c.is_whitespace());
        if trimmed_key.is_empty() {
            continue;
        }
        if !is_safe_environment_value(&value) {
            continue;
        }
        result.insert(trimmed_key.to_string(), value);
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Swift `isSafeEnvironmentValue(_:)` (`:620-622`): no control chars / DEL.
fn is_safe_environment_value(value: &str) -> bool {
    !value
        .chars()
        .any(|c| (c as u32) < 0x20 || (c as u32) == 0x7F)
}

/// Swift `normalizedEnvironmentKeys(_:environment:)` (`:624-633`): sorted union
/// of explicit trimmed non-empty keys and the environment's keys.
fn normalized_environment_keys(
    environment_keys: &[String],
    environment: Option<&HashMap<String, String>>,
) -> Vec<String> {
    // DIVERGENCE: Swift `Array(Set(...)).sorted()` uses Swift `String`
    // Comparable (Unicode canonical order). `BTreeSet<String>` sorts by Rust
    // byte/scalar order. Identical for the ASCII env-var keys these records
    // carry.
    let mut set: BTreeSet<String> = BTreeSet::new();
    for key in environment_keys {
        let trimmed = key.trim_matches(|c: char| c.is_whitespace());
        if !trimmed.is_empty() {
            set.insert(trimmed.to_string());
        }
    }
    if let Some(environment) = environment {
        for key in environment.keys() {
            set.insert(key.clone());
        }
    }
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn binding(
        command: &str,
        cwd: Option<&str>,
        source: Option<&str>,
        environment: Option<HashMap<String, String>>,
    ) -> SurfaceResumeBindingSnapshot {
        SurfaceResumeBindingSnapshot::new(
            None,
            None,
            command,
            cwd,
            None,
            source,
            environment,
            None,
            0.0,
        )
    }

    #[test]
    fn signing_payload_data_pins_golden_bytes() {
        let record = SurfaceResumeApprovalRecord::new(
            "abc123".to_string(),
            Some("My Session"),
            vec!["claude".to_string(), "--resume".to_string()],
            Some("/home/user/project"),
            Some(env(&[("FOO", "bar"), ("BAZ", "qux")])),
            Vec::new(),
            Some("agent-hook"),
            SurfaceResumeApprovalPolicy::Auto,
            1000.0,
            2000.5,
            Some(1500.0),
            None,
        );
        let expected = concat!(
            "version=1\n",
            "id=abc123\n",
            "name=TXkgU2Vzc2lvbg==\n",
            "commandPrefix=Y2xhdWRl,LS1yZXN1bWU=\n",
            "cwd=L2hvbWUvdXNlci9wcm9qZWN0\n",
            "environment=QkFa=cXV4,Rk9P=YmFy\n",
            "environmentKeys=QkFa,Rk9P\n",
            "source=YWdlbnQtaG9vaw==\n",
            "policy=auto\n",
            "createdAt=1000.0\n",
            "updatedAt=2000.5\n",
            "lastUsedAt=1500.0",
        );
        assert_eq!(
            String::from_utf8(record.signing_payload_data()).unwrap(),
            expected
        );
    }

    #[test]
    fn signing_payload_data_empty_optionals_render_blank() {
        let record = SurfaceResumeApprovalRecord::new(
            "id0".to_string(),
            None,
            vec!["cmd".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Manual,
            0.0,
            0.0,
            None,
            None,
        );
        let expected = concat!(
            "version=1\n",
            "id=id0\n",
            "name=\n",
            "commandPrefix=Y21k\n",
            "cwd=\n",
            "environment=\n",
            "environmentKeys=\n",
            "source=\n",
            "policy=manual\n",
            "createdAt=0.0\n",
            "updatedAt=0.0\n",
            "lastUsedAt=",
        );
        assert_eq!(
            String::from_utf8(record.signing_payload_data()).unwrap(),
            expected
        );
    }

    #[test]
    fn new_drops_empty_command_prefix_tokens() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["a".to_string(), String::new(), "b".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Manual,
            0.0,
            0.0,
            None,
            None,
        );
        assert_eq!(
            record.command_prefix,
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn normalized_environment_keeps_sensitive_but_drops_empty_key_and_unsafe_value() {
        // FAITHFUL to Swift record `:609-618`: sensitive keys are RETAINED here
        // (they are already filtered upstream by the snapshot). Empty keys and
        // non-printable values are dropped.
        let mut e = env(&[("API_KEY", "keep-me"), ("SAFE", "ok")]);
        e.insert("  ".to_string(), "blank-key".to_string());
        e.insert("BAD".to_string(), "line1\nline2".to_string());
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["cmd".to_string()],
            None,
            Some(e),
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Manual,
            0.0,
            0.0,
            None,
            None,
        );
        let environment = record.environment.unwrap();
        assert!(environment.contains_key("API_KEY")); // sensitive kept
        assert!(environment.contains_key("SAFE"));
        assert!(!environment.contains_key("")); // empty key dropped
        assert!(!environment.contains_key("BAD")); // control-char value dropped
    }

    #[test]
    fn normalized_environment_keys_is_sorted_union() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["cmd".to_string()],
            None,
            Some(env(&[("FOO", "1"), ("BAR", "2")])),
            vec!["ZED".to_string(), " ".to_string(), "FOO".to_string()],
            None,
            SurfaceResumeApprovalPolicy::Manual,
            0.0,
            0.0,
            None,
            None,
        );
        // Union of {ZED, FOO} explicit and {FOO, BAR} derived, sorted.
        assert_eq!(
            record.environment_keys,
            vec!["BAR".to_string(), "FOO".to_string(), "ZED".to_string()]
        );
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["cmd".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            1.0,
            1.0,
            None,
            None,
        );
        let signed = record.signed(b"secret");
        assert!(signed.signature.is_some());
        assert!(signed.has_valid_signature(b"secret"));
        assert!(!signed.has_valid_signature(b"other"));
    }

    #[test]
    fn tampering_a_field_invalidates_signature() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["cmd".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            1.0,
            1.0,
            None,
            None,
        );
        let mut signed = record.signed(b"secret");
        signed.policy = SurfaceResumeApprovalPolicy::Manual;
        assert!(!signed.has_valid_signature(b"secret"));
    }

    #[test]
    fn unsigned_record_has_no_valid_signature() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["cmd".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            1.0,
            1.0,
            None,
            None,
        );
        assert!(!record.has_valid_signature(b"secret"));
    }

    #[test]
    fn matches_exact_prefix() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["claude".to_string(), "--resume".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(record.matches(&binding("claude --resume", None, None, None)));
    }

    #[test]
    fn matches_longer_command_with_extra_tokens() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["claude".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(record.matches(&binding("claude --resume main", None, None, None)));
    }

    #[test]
    fn matches_rejects_when_command_shorter_than_prefix() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["claude".to_string(), "--resume".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(!record.matches(&binding("claude", None, None, None)));
    }

    #[test]
    fn matches_rejects_cwd_mismatch() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["claude".to_string()],
            Some("/home/a"),
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(record.matches(&binding("claude", Some("/home/a"), None, None)));
        assert!(record.matches(&binding("claude", Some("/home/a/"), None, None))); // normalized-equal
        assert!(!record.matches(&binding("claude", Some("/home/b"), None, None)));
    }

    #[test]
    fn matches_environment_equality_rules() {
        // Empty record env: matches only bindings with empty env.
        let empty_env_record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["c".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(empty_env_record.matches(&binding("c", None, None, None)));
        assert!(!empty_env_record.matches(&binding("c", None, None, Some(env(&[("K", "v")])))));

        // Non-empty record env: requires full equality.
        let env_record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["c".to_string()],
            None,
            Some(env(&[("K", "v")])),
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(env_record.matches(&binding("c", None, None, Some(env(&[("K", "v")])))));
        assert!(!env_record.matches(&binding("c", None, None, Some(env(&[("K", "other")])))));
        assert!(!env_record.matches(&binding("c", None, None, None)));
    }

    #[test]
    fn matches_rejects_unparseable_command() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["c".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert!(!record.matches(&binding("'unterminated", None, None, None)));
    }

    #[test]
    fn command_prefix_text_shell_quotes() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["claude".to_string(), "a b".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Auto,
            0.0,
            0.0,
            None,
            None,
        );
        assert_eq!(record.command_prefix_text(), "claude 'a b'");
    }

    #[test]
    fn serde_omits_nil_optionals_and_uses_camel_case() {
        let record = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            None,
            vec!["c".to_string()],
            None,
            None,
            Vec::new(),
            None,
            SurfaceResumeApprovalPolicy::Manual,
            1.0,
            2.0,
            None,
            None,
        );
        let value = serde_json::to_value(&record).unwrap();
        assert!(value.get("name").is_none());
        assert!(value.get("cwd").is_none());
        assert!(value.get("environment").is_none());
        assert!(value.get("source").is_none());
        assert!(value.get("lastUsedAt").is_none());
        assert!(value.get("signature").is_none());
        assert_eq!(
            value
                .get("commandPrefix")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(value.get("createdAt").is_some());
        assert!(value.get("environmentKeys").is_some());
        assert_eq!(value.get("policy").unwrap().as_str(), Some("manual"));
    }

    #[test]
    fn serde_round_trip_preserves_signature_validity() {
        let signed = SurfaceResumeApprovalRecord::new(
            "id".to_string(),
            Some("n"),
            vec!["c".to_string()],
            Some("/x"),
            Some(env(&[("K", "v")])),
            Vec::new(),
            Some("cli"),
            SurfaceResumeApprovalPolicy::Auto,
            1.0,
            2.0,
            Some(3.0),
            None,
        )
        .signed(b"secret");
        let json = serde_json::to_string(&signed).unwrap();
        let back: SurfaceResumeApprovalRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, signed);
        assert!(back.has_valid_signature(b"secret"));
    }
}
