//! Pure approval-store operations over an injected file path + signing secret.
//!
//! Ported from `enum SurfaceResumeApprovalStore` (`SessionPersistence.swift:
//! 710-1093`). Every function takes the store file path and signing secret
//! explicitly (the Swift `fileURL`/`fileManager`/`signingSecret` seams), so the
//! whole store is unit-testable with tempfiles and a fixed secret.
//!
//! SCOPE: only the STANDALONE JSON file path is ported. Swift also embeds
//! records inside `cmux.json` app settings
//! (`storesRecordsInCmuxSettings`/`loadRecordsFromCmuxSettings`, keyed on the
//! file being named `cmux.json`); that branch is intentionally stubbed here (see
//! [`stores_records_in_cmux_settings`]) so this lane does not pull in
//! cmux-config. Callers inject the standalone path.
//!
//! SECRET SEAM: Swift's `defaultSigningSecret` reads the macOS Keychain (or an
//! env/file fallback). That host wiring is out of scope — callers inject the
//! secret bytes. Windows secret storage is a later host-wiring slice.
// TODO(host-wiring): provide a Windows `default_signing_secret` seam (Credential
// Manager / DPAPI) mirroring Swift `defaultSigningSecret` (`:1028-1043`).

use std::cmp::Ordering;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use cmux_tmux::SurfaceResumeBindingSnapshot;

use crate::canonicalizer;
use crate::record::{SurfaceResumeApprovalPolicy, SurfaceResumeApprovalRecord};

/// The on-disk container: `{ "version": 1, "records": [...] }`.
/// Mirrors Swift `SurfaceResumeApprovalStore.StoredFile` (`:719-722`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoredFile {
    pub version: i64,
    pub records: Vec<SurfaceResumeApprovalRecord>,
}

/// The trust decision the store applies to a restored binding.
///
// DIVERGENCE: Swift mutates `binding.approvalPolicy`/`approvalRecordId`/
// `autoResume` in place and returns the whole `SurfaceResumeBindingSnapshot`.
// The cmux-tmux `SurfaceResumeBindingSnapshot` this lane consumes is READ-ONLY
// and (by its own documented divergence) carries neither `approvalPolicy` nor
// `approvalRecordId`. So the trust decision is surfaced as this dedicated
// value instead of a mutated snapshot; host wiring applies it to whatever
// runtime binding it holds.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedApproval {
    pub auto_resume: bool,
    pub approval_policy: SurfaceResumeApprovalPolicy,
    pub approval_record_id: Option<String>,
}

// ---- source predicates (Swift `SurfaceResumeBindingSnapshot` computed vars) ----

fn is_process_detected(binding: &SurfaceResumeBindingSnapshot) -> bool {
    binding.source.as_deref() == Some("process-detected")
}

fn is_agent_hook(binding: &SurfaceResumeBindingSnapshot) -> bool {
    binding.source.as_deref() == Some("agent-hook")
}

fn is_cli(binding: &SurfaceResumeBindingSnapshot) -> bool {
    binding.source.as_deref() == Some("cli")
}

// ---- loading ----

/// Loads records from `path`. In the standalone-only port this is exactly
/// [`load_standalone_records`]. Mirrors Swift `loadRecords` (`:738-767`) with
/// the cmux-settings branch stubbed off.
pub fn load_records(path: &Path) -> Vec<SurfaceResumeApprovalRecord> {
    load_standalone_records(path)
}

/// Reads the standalone JSON file, accepting either the `StoredFile` envelope or
/// a bare `[record]` array. Missing/unreadable/undecodable → empty.
/// Mirrors Swift `loadStandaloneRecords` (`:793-802`).
pub fn load_standalone_records(path: &Path) -> Vec<SurfaceResumeApprovalRecord> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    if let Ok(file) = serde_json::from_slice::<StoredFile>(&data) {
        return file.records;
    }
    serde_json::from_slice::<Vec<SurfaceResumeApprovalRecord>>(&data).unwrap_or_default()
}

/// Records whose signature validates under `signing_secret`.
/// Mirrors Swift `validRecords` (`:804-813`).
pub fn valid_records(path: &Path, signing_secret: &[u8]) -> Vec<SurfaceResumeApprovalRecord> {
    load_records(path)
        .into_iter()
        .filter(|record| record.has_valid_signature(signing_secret))
        .collect()
}

/// The best matching valid record for `binding`: longest command prefix first,
/// then most recently updated. Mirrors Swift `matchingRecord` (`:815-830`).
pub fn matching_record(
    binding: &SurfaceResumeBindingSnapshot,
    path: &Path,
    signing_secret: &[u8],
) -> Option<SurfaceResumeApprovalRecord> {
    let mut matches: Vec<SurfaceResumeApprovalRecord> = valid_records(path, signing_secret)
        .into_iter()
        .filter(|record| record.matches(binding))
        .collect();
    matches.sort_by(|lhs, rhs| {
        if lhs.command_prefix.len() != rhs.command_prefix.len() {
            // Longer prefix first.
            rhs.command_prefix.len().cmp(&lhs.command_prefix.len())
        } else {
            // Newer updatedAt first.
            rhs.updated_at
                .partial_cmp(&lhs.updated_at)
                .unwrap_or(Ordering::Equal)
        }
    });
    matches.into_iter().next()
}

// ---- trust decisions ----

/// Resolves the trust decision for a restored binding.
/// Mirrors Swift `applyingStoredApproval` (`:832-871`).
pub fn applying_stored_approval(
    binding: &SurfaceResumeBindingSnapshot,
    path: &Path,
    signing_secret: &[u8],
) -> AppliedApproval {
    if is_process_detected(binding) {
        return AppliedApproval {
            auto_resume: true,
            approval_policy: SurfaceResumeApprovalPolicy::Auto,
            approval_record_id: None,
        };
    }
    if is_agent_hook(binding) {
        let auto_resume = binding.auto_resume == Some(true);
        return AppliedApproval {
            auto_resume,
            approval_policy: if auto_resume {
                SurfaceResumeApprovalPolicy::Auto
            } else {
                SurfaceResumeApprovalPolicy::Manual
            },
            approval_record_id: None,
        };
    }
    match matching_record(binding, path, signing_secret) {
        None => AppliedApproval {
            auto_resume: false,
            approval_policy: SurfaceResumeApprovalPolicy::Manual,
            approval_record_id: None,
        },
        Some(record) => AppliedApproval {
            auto_resume: record.policy == SurfaceResumeApprovalPolicy::Auto,
            approval_policy: record.policy,
            approval_record_id: Some(record.id),
        },
    }
}

/// Whether the UI should prompt the user to approve `binding`.
/// Mirrors Swift `shouldPromptForProposal` (`:873-896`).
pub fn should_prompt_for_proposal(
    binding: &SurfaceResumeBindingSnapshot,
    existing_record: Option<&SurfaceResumeApprovalRecord>,
    is_main_thread: bool,
    is_running_tests: bool,
) -> bool {
    if !is_main_thread {
        return false;
    }
    if is_running_tests {
        return false;
    }
    if is_cli(binding) {
        return false;
    }
    if is_process_detected(binding) || is_agent_hook(binding) {
        return false;
    }
    if canonicalizer::tokens(&binding.command).is_none() {
        return false;
    }
    match existing_record {
        None => true,
        Some(record) => record.policy == SurfaceResumeApprovalPolicy::Prompt,
    }
}

/// For a CLI binding with no existing record: writes a promptless manual
/// approval and returns the resulting trust decision.
/// Mirrors Swift `applyingPromptlessCLIManualApprovalIfNeeded` (`:898-927`).
pub fn applying_promptless_cli_manual_approval_if_needed(
    binding: &SurfaceResumeBindingSnapshot,
    existing_record: Option<&SurfaceResumeApprovalRecord>,
    path: &Path,
    signing_secret: &[u8],
) -> Option<AppliedApproval> {
    if !is_cli(binding) || existing_record.is_some() {
        return None;
    }
    let record = approve(
        binding,
        SurfaceResumeApprovalPolicy::Manual,
        None,
        path,
        signing_secret,
    )?;
    // Swift recomputes `applyingStoredApproval` and then overwrites the three
    // approval fields with the freshly-written record's — so the final decision
    // is fully determined by `record`. `applyingStoredApproval` has no side
    // effects, so we compute the decision directly from `record`.
    Some(AppliedApproval {
        auto_resume: record.policy == SurfaceResumeApprovalPolicy::Auto,
        approval_policy: record.policy,
        approval_record_id: Some(record.id),
    })
}

// ---- writes ----

/// Creates (or replaces the matching) approval record for `binding`, signs it,
/// and persists it. Mirrors Swift `approve` (`:929-970`).
pub fn approve(
    binding: &SurfaceResumeBindingSnapshot,
    policy: SurfaceResumeApprovalPolicy,
    command_prefix: Option<Vec<String>>,
    path: &Path,
    signing_secret: &[u8],
) -> Option<SurfaceResumeApprovalRecord> {
    let tokens = canonicalizer::tokens(&binding.command)?;
    let prefix = command_prefix.unwrap_or_else(|| tokens.clone());
    if prefix.is_empty() || tokens.len() < prefix.len() || tokens[..prefix.len()] != prefix[..] {
        return None;
    }
    let now = now_unix();
    let existing = matching_record(binding, path, signing_secret);
    let environment = binding.environment.clone();
    let environment_keys: Vec<String> = binding
        .environment
        .as_ref()
        .map(|e| e.keys().cloned().collect())
        .unwrap_or_default();
    let record = SurfaceResumeApprovalRecord::new(
        existing
            .as_ref()
            .map(|r| r.id.clone())
            .unwrap_or_else(generate_uuid_lowercased),
        binding.name.as_deref(),
        prefix,
        binding.cwd.as_deref(),
        environment,
        environment_keys,
        binding.source.as_deref(),
        policy,
        existing.as_ref().map(|r| r.created_at).unwrap_or(now),
        now,
        existing.as_ref().and_then(|r| r.last_used_at),
        None,
    )
    .signed(signing_secret);
    write_replacing(&record, path);
    Some(record)
}

/// Updates a stored record's policy and/or command prefix, re-timestamps and
/// re-signs it. Mirrors Swift `update` (`:972-997`).
pub fn update(
    record_id: &str,
    policy: Option<SurfaceResumeApprovalPolicy>,
    command_prefix: Option<Vec<String>>,
    path: &Path,
    signing_secret: &[u8],
) -> bool {
    let mut records = load_records(path);
    let index = match records.iter().position(|r| r.id == record_id) {
        Some(index) => index,
        None => return false,
    };
    if !records[index].has_valid_signature(signing_secret) {
        return false;
    }
    if let Some(policy) = policy {
        records[index].policy = policy;
    }
    if let Some(command_prefix) = command_prefix {
        if command_prefix.is_empty() {
            return false;
        }
        // Set directly (like Swift): bypasses the init's empty-token filter.
        records[index].command_prefix = command_prefix;
    }
    records[index].updated_at = now_unix();
    let signed = records[index].signed(signing_secret);
    records[index] = signed;
    write_standalone_records(&records, path)
}

/// Loads, replaces-or-appends by id, and rewrites.
/// Mirrors Swift `writeReplacing` (`:1045-1057`).
fn write_replacing(record: &SurfaceResumeApprovalRecord, path: &Path) -> bool {
    let mut records = load_records(path);
    if let Some(index) = records.iter().position(|r| r.id == record.id) {
        records[index] = record.clone();
    } else {
        records.push(record.clone());
    }
    write_standalone_records(&records, path)
}

/// Writes the `StoredFile` envelope to `path`, creating parent directories.
/// Mirrors Swift `writeStandaloneRecords` (`:1071-1093`).
///
// DIVERGENCE: Swift also sets POSIX permissions (0o700 dir / 0o600 file) and
// posts `didChangeNotification` via NotificationCenter. Neither has a portable
// Windows equivalent in this pure lane; permission hardening and change
// notifications are host-wiring concerns handled by the caller.
fn write_standalone_records(records: &[SurfaceResumeApprovalRecord], path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let file = StoredFile {
        version: 1,
        records: records.to_vec(),
    };
    match serde_json::to_vec_pretty(&file) {
        Ok(data) => std::fs::write(path, data).is_ok(),
        Err(_) => false,
    }
}

/// Standalone-only: the cmux-settings-embedded storage branch is stubbed off.
/// Mirrors Swift `storesRecordsInCmuxSettings` (`:1095-1097`, `cmux.json`).
// SEAM: always false here so `load`/`write` take the standalone path. Porting
// the `cmux.json`-embedded branch requires cmux-config and is deferred.
#[allow(dead_code)]
fn stores_records_in_cmux_settings(_path: &Path) -> bool {
    false
}

/// Swift `Date().timeIntervalSince1970` as `f64` seconds, quantized to
/// milliseconds.
///
// DIVERGENCE: Swift signs the full-precision `Double` from `Date()`. serde_json
// (this workspace's build, without the `float_roundtrip` feature) uses a fast
// float PARSER that is not correctly-rounded: a full-precision timestamp like
// `1783060492.0182083` reads back 1 ULP off on reload, silently mutating the
// signing payload and invalidating a just-written signature. We cannot enable
// the feature (Cargo.toml is frozen for this lane), so timestamps are minted on
// a millisecond grid: a ms-grid value's shortest decimal has ≤13 significant
// digits, which serde_json's fast parser rounds EXACTLY, guaranteeing a stable
// self-consistent round-trip. Sub-millisecond precision is immaterial for
// approval bookkeeping. (Cross-host records authored at full precision by macOS
// would still need a correctly-rounded parser to validate here; wiring
// `serde_json/float_roundtrip` at the workspace level removes this divergence.)
fn now_unix() -> f64 {
    let raw = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    (raw * 1_000.0).round() / 1_000.0
}

/// A lowercased UUID-v4-shaped identifier.
///
// DIVERGENCE: Swift uses Foundation `UUID().uuidString.lowercased()` (a
// cryptographically-random v4). This pure lane has no `uuid`/`rand` dependency,
// so ids are generated from a time+counter-seeded xorshift and formatted with
// the v4 version/variant nibbles set. Adequate for record uniqueness within the
// store; host wiring can inject real UUIDs later. Ids are opaque and never
// participate in the signing payload's cross-platform contract beyond being
// copied verbatim.
fn generate_uuid_lowercased() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    let mut state = nanos ^ counter.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut bytes = [0u8; 16];
    bytes[0..8].copy_from_slice(&next().to_be_bytes());
    bytes[8..16].copy_from_slice(&next().to_be_bytes());
    bytes[6] = (bytes[6] & 0x0F) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // RFC 4122 variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    const SECRET: &[u8] = b"unit-test-secret";

    fn binding(
        command: &str,
        cwd: Option<&str>,
        source: Option<&str>,
        environment: Option<HashMap<String, String>>,
        auto_resume: Option<bool>,
    ) -> SurfaceResumeBindingSnapshot {
        SurfaceResumeBindingSnapshot::new(
            Some("name"),
            Some("kind"),
            command,
            cwd,
            None,
            source,
            environment,
            auto_resume,
            0.0,
        )
    }

    fn store_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("nested").join("resume-commands.json")
    }

    fn write_raw(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn make_record(
        id: &str,
        prefix: &[&str],
        policy: SurfaceResumeApprovalPolicy,
        updated_at: f64,
    ) -> SurfaceResumeApprovalRecord {
        SurfaceResumeApprovalRecord::new(
            id.to_string(),
            None,
            prefix.iter().map(|s| s.to_string()).collect(),
            None,
            None,
            Vec::new(),
            None,
            policy,
            0.0,
            updated_at,
            None,
            None,
        )
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = TempDir::new().unwrap();
        assert!(load_records(&store_path(&dir)).is_empty());
    }

    #[test]
    fn load_accepts_stored_file_envelope() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let record =
            make_record("r1", &["c"], SurfaceResumeApprovalPolicy::Manual, 1.0).signed(SECRET);
        let file = StoredFile {
            version: 1,
            records: vec![record],
        };
        write_raw(&path, &serde_json::to_string(&file).unwrap());
        let loaded = load_records(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "r1");
    }

    #[test]
    fn load_accepts_bare_array_fallback() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let record =
            make_record("r1", &["c"], SurfaceResumeApprovalPolicy::Manual, 1.0).signed(SECRET);
        write_raw(&path, &serde_json::to_string(&vec![record]).unwrap());
        let loaded = load_records(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "r1");
    }

    #[test]
    fn valid_records_drops_unsigned_and_tampered() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let good =
            make_record("good", &["c"], SurfaceResumeApprovalPolicy::Auto, 1.0).signed(SECRET);
        let unsigned = make_record("unsigned", &["c"], SurfaceResumeApprovalPolicy::Auto, 1.0);
        // Signed with a different secret → invalid under SECRET.
        let wrong_secret = make_record("wrong", &["c"], SurfaceResumeApprovalPolicy::Auto, 1.0)
            .signed(b"other-secret");
        let file = StoredFile {
            version: 1,
            records: vec![good, unsigned, wrong_secret],
        };
        write_raw(&path, &serde_json::to_string(&file).unwrap());
        let valid = valid_records(&path, SECRET);
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].id, "good");
    }

    #[test]
    fn matching_record_picks_longest_prefix_then_newest() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let short = make_record(
            "short",
            &["claude"],
            SurfaceResumeApprovalPolicy::Auto,
            100.0,
        )
        .signed(SECRET);
        let long_old = make_record(
            "long_old",
            &["claude", "--resume"],
            SurfaceResumeApprovalPolicy::Auto,
            1.0,
        )
        .signed(SECRET);
        let long_new = make_record(
            "long_new",
            &["claude", "--resume"],
            SurfaceResumeApprovalPolicy::Auto,
            2.0,
        )
        .signed(SECRET);
        let file = StoredFile {
            version: 1,
            records: vec![short, long_old, long_new],
        };
        write_raw(&path, &serde_json::to_string(&file).unwrap());
        let matched = matching_record(
            &binding("claude --resume main", None, None, None, None),
            &path,
            SECRET,
        )
        .unwrap();
        // Longest prefix wins; among equal-length, newest updatedAt wins.
        assert_eq!(matched.id, "long_new");
    }

    #[test]
    fn applying_stored_approval_process_detected_is_auto() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let decision = applying_stored_approval(
            &binding("claude", None, Some("process-detected"), None, None),
            &path,
            SECRET,
        );
        assert_eq!(
            decision,
            AppliedApproval {
                auto_resume: true,
                approval_policy: SurfaceResumeApprovalPolicy::Auto,
                approval_record_id: None,
            }
        );
    }

    #[test]
    fn applying_stored_approval_agent_hook_follows_auto_resume_flag() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let auto = applying_stored_approval(
            &binding("claude", None, Some("agent-hook"), None, Some(true)),
            &path,
            SECRET,
        );
        assert_eq!(auto.approval_policy, SurfaceResumeApprovalPolicy::Auto);
        assert!(auto.auto_resume);

        let manual = applying_stored_approval(
            &binding("claude", None, Some("agent-hook"), None, Some(false)),
            &path,
            SECRET,
        );
        assert_eq!(manual.approval_policy, SurfaceResumeApprovalPolicy::Manual);
        assert!(!manual.auto_resume);
    }

    #[test]
    fn applying_stored_approval_matched_record_uses_record_policy() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let record =
            make_record("r", &["claude"], SurfaceResumeApprovalPolicy::Auto, 1.0).signed(SECRET);
        let file = StoredFile {
            version: 1,
            records: vec![record],
        };
        write_raw(&path, &serde_json::to_string(&file).unwrap());
        let decision = applying_stored_approval(
            &binding("claude --resume", None, Some("other"), None, None),
            &path,
            SECRET,
        );
        assert_eq!(decision.approval_policy, SurfaceResumeApprovalPolicy::Auto);
        assert!(decision.auto_resume);
        assert_eq!(decision.approval_record_id.as_deref(), Some("r"));
    }

    #[test]
    fn applying_stored_approval_no_match_is_manual_no_auto() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let decision = applying_stored_approval(
            &binding("claude", None, Some("other"), None, None),
            &path,
            SECRET,
        );
        assert_eq!(
            decision,
            AppliedApproval {
                auto_resume: false,
                approval_policy: SurfaceResumeApprovalPolicy::Manual,
                approval_record_id: None,
            }
        );
    }

    #[test]
    fn approve_writes_and_round_trips_through_file() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let record = approve(
            &binding(
                "claude --resume main",
                Some("/w"),
                Some("other"),
                None,
                None,
            ),
            SurfaceResumeApprovalPolicy::Auto,
            None,
            &path,
            SECRET,
        )
        .unwrap();
        assert_eq!(record.command_prefix, vec!["claude", "--resume", "main"]);
        assert!(record.has_valid_signature(SECRET));

        // Persisted and reloadable + valid.
        let valid = valid_records(&path, SECRET);
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].id, record.id);

        // Now it is the matching record.
        let decision = applying_stored_approval(
            &binding(
                "claude --resume main",
                Some("/w"),
                Some("other"),
                None,
                None,
            ),
            &path,
            SECRET,
        );
        assert!(decision.auto_resume);
        assert_eq!(
            decision.approval_record_id.as_deref(),
            Some(record.id.as_str())
        );
    }

    #[test]
    fn approve_rejects_unparseable_command() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        assert!(approve(
            &binding("'unterminated", None, Some("other"), None, None),
            SurfaceResumeApprovalPolicy::Manual,
            None,
            &path,
            SECRET,
        )
        .is_none());
    }

    #[test]
    fn approve_rejects_prefix_not_matching_command() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        assert!(approve(
            &binding("claude", None, Some("other"), None, None),
            SurfaceResumeApprovalPolicy::Manual,
            Some(vec!["not".to_string(), "matching".to_string()]),
            &path,
            SECRET,
        )
        .is_none());
    }

    #[test]
    fn approve_reuses_existing_id_and_created_at() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let first = approve(
            &binding("claude --resume", None, Some("other"), None, None),
            SurfaceResumeApprovalPolicy::Manual,
            None,
            &path,
            SECRET,
        )
        .unwrap();
        let second = approve(
            &binding("claude --resume", None, Some("other"), None, None),
            SurfaceResumeApprovalPolicy::Auto,
            None,
            &path,
            SECRET,
        )
        .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.created_at, second.created_at);
        assert_eq!(second.policy, SurfaceResumeApprovalPolicy::Auto);
        // Still exactly one record on disk.
        assert_eq!(load_records(&path).len(), 1);
    }

    #[test]
    fn update_changes_policy_and_prefix_and_resigns() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let record = approve(
            &binding("claude --resume main", None, Some("other"), None, None),
            SurfaceResumeApprovalPolicy::Manual,
            None,
            &path,
            SECRET,
        )
        .unwrap();

        assert!(update(
            &record.id,
            Some(SurfaceResumeApprovalPolicy::Auto),
            Some(vec!["claude".to_string()]),
            &path,
            SECRET,
        ));

        let reloaded = valid_records(&path, SECRET);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].policy, SurfaceResumeApprovalPolicy::Auto);
        assert_eq!(reloaded[0].command_prefix, vec!["claude".to_string()]);
        assert!(reloaded[0].has_valid_signature(SECRET));
    }

    #[test]
    fn update_rejects_unknown_id_and_empty_prefix() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        let record = approve(
            &binding("claude", None, Some("other"), None, None),
            SurfaceResumeApprovalPolicy::Manual,
            None,
            &path,
            SECRET,
        )
        .unwrap();
        assert!(!update(
            "nope",
            Some(SurfaceResumeApprovalPolicy::Auto),
            None,
            &path,
            SECRET
        ));
        assert!(!update(&record.id, None, Some(Vec::new()), &path, SECRET));
    }

    #[test]
    fn update_rejects_record_with_invalid_signature() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        // Record signed with a different secret.
        let record = make_record("r", &["c"], SurfaceResumeApprovalPolicy::Manual, 1.0)
            .signed(b"other-secret");
        let file = StoredFile {
            version: 1,
            records: vec![record],
        };
        write_raw(&path, &serde_json::to_string(&file).unwrap());
        assert!(!update(
            "r",
            Some(SurfaceResumeApprovalPolicy::Auto),
            None,
            &path,
            SECRET
        ));
    }

    #[test]
    fn should_prompt_for_proposal_branches() {
        let b = binding("claude", None, Some("other"), None, None);
        // Not main thread → false.
        assert!(!should_prompt_for_proposal(&b, None, false, false));
        // Running tests → false.
        assert!(!should_prompt_for_proposal(&b, None, true, true));
        // CLI binding → false.
        assert!(!should_prompt_for_proposal(
            &binding("claude", None, Some("cli"), None, None),
            None,
            true,
            false
        ));
        // process-detected / agent-hook → false.
        assert!(!should_prompt_for_proposal(
            &binding("claude", None, Some("process-detected"), None, None),
            None,
            true,
            false
        ));
        // Unparseable command → false.
        assert!(!should_prompt_for_proposal(
            &binding("'bad", None, Some("other"), None, None),
            None,
            true,
            false
        ));
        // No existing record → true.
        assert!(should_prompt_for_proposal(&b, None, true, false));
        // Existing prompt record → true.
        let prompt = make_record("r", &["claude"], SurfaceResumeApprovalPolicy::Prompt, 0.0);
        assert!(should_prompt_for_proposal(&b, Some(&prompt), true, false));
        // Existing auto record → false.
        let auto = make_record("r", &["claude"], SurfaceResumeApprovalPolicy::Auto, 0.0);
        assert!(!should_prompt_for_proposal(&b, Some(&auto), true, false));
    }

    #[test]
    fn promptless_cli_manual_approval_writes_only_for_new_cli_binding() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        // Non-CLI → None.
        assert!(applying_promptless_cli_manual_approval_if_needed(
            &binding("claude", None, Some("other"), None, None),
            None,
            &path,
            SECRET,
        )
        .is_none());
        // CLI with existing record → None.
        let existing = make_record("r", &["claude"], SurfaceResumeApprovalPolicy::Manual, 0.0);
        assert!(applying_promptless_cli_manual_approval_if_needed(
            &binding("claude", None, Some("cli"), None, None),
            Some(&existing),
            &path,
            SECRET,
        )
        .is_none());
        // CLI, no existing record → writes a manual approval.
        let decision = applying_promptless_cli_manual_approval_if_needed(
            &binding("claude --resume", None, Some("cli"), None, None),
            None,
            &path,
            SECRET,
        )
        .unwrap();
        assert_eq!(
            decision.approval_policy,
            SurfaceResumeApprovalPolicy::Manual
        );
        assert!(!decision.auto_resume);
        assert!(decision.approval_record_id.is_some());
        assert_eq!(valid_records(&path, SECRET).len(), 1);
    }

    #[test]
    fn approve_reload_roundtrip_is_signature_stable() {
        // Regression guard: freshly-written records must always validate on
        // reload. serde_json's default float parser is not correctly-rounded, so
        // `now_unix()` mints millisecond-grid timestamps that round-trip exactly
        // (see `now_unix` divergence note). Runs many iterations because the
        // failure mode was timestamp-value-dependent and intermittent.
        for i in 0..1000 {
            let dir = TempDir::new().unwrap();
            let path = store_path(&dir);
            let record = approve(
                &binding("claude --resume main", None, Some("other"), None, None),
                SurfaceResumeApprovalPolicy::Manual,
                None,
                &path,
                SECRET,
            )
            .unwrap();
            let loaded = load_records(&path);
            assert!(
                !loaded.is_empty() && loaded[0].has_valid_signature(SECRET),
                "reload signature invalid at iter {i}: in-mem={} disk={}",
                record.updated_at,
                loaded.first().map(|r| r.updated_at).unwrap_or(f64::NAN),
            );
        }
    }

    #[test]
    fn generated_ids_are_unique_and_uuid_shaped() {
        let a = generate_uuid_lowercased();
        let b = generate_uuid_lowercased();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'4'); // version nibble
        let hyphens: Vec<usize> = a.match_indices('-').map(|(i, _)| i).collect();
        assert_eq!(hyphens, vec![8, 13, 18, 23]);
    }
}
