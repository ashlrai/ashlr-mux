//! cmux-resume — SurfaceResume approval subsystem.
//!
//! Headless, 100%-pure port of the approval subsystem from the canonical macOS
//! `Sources/SessionPersistence.swift`:
//!
//! - [`canonicalizer`] ← `SurfaceResumeCommandCanonicalizer`
//!   (`SessionPersistence.swift:636-696`): a char-by-char shell-token lexer,
//!   lexical cwd normalization, and shell-quoting.
//! - [`signature`] ← `SurfaceResumeApprovalSignature`
//!   (`SessionPersistence.swift:698-708`): base64 HMAC-SHA256 signing +
//!   constant-time verification.
//! - [`record`] ← `SurfaceResumeApprovalPolicy` / `SurfaceResumeApprovalRecord`
//!   (`SessionPersistence.swift:262-266`, `:491-634`): the Codable record, its
//!   designated-init normalization, the golden-byte signing payload, and
//!   longest-prefix binding matching.
//! - [`store`] ← `SurfaceResumeApprovalStore`
//!   (`SessionPersistence.swift:710-1093`): pure load/valid/match/approve/update
//!   and the trust-decision functions that gate whether a restored surface may
//!   auto-run its resume command.
//!
//! Every I/O boundary in Swift (file URL, file manager, signing secret) is an
//! injected seam, so the whole subsystem is unit-testable with tempfiles and a
//! fixed secret. The store consumes the read-only
//! [`cmux_tmux::SurfaceResumeBindingSnapshot`] as its restored-binding input.
//!
//! Only the STANDALONE JSON-file storage path is ported; the
//! `cmux.json`-settings-embedded storage and the macOS-Keychain default signing
//! secret are documented seams handled by later host wiring.

pub mod canonicalizer;
pub mod record;
pub mod signature;
pub mod store;

pub use record::{SurfaceResumeApprovalPolicy, SurfaceResumeApprovalRecord};
pub use signature::SurfaceResumeApprovalSignature;
pub use store::{
    AppliedApproval, StoredFile, applying_promptless_cli_manual_approval_if_needed,
    applying_stored_approval, approve, load_records, load_standalone_records, matching_record,
    should_prompt_for_proposal, update, valid_records,
};
