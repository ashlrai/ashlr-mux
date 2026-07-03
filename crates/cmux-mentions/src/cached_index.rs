//! Port of `Sources/TextBoxMentionCachedIndex.swift`.
//!
//! Swift stores `Date` values; this port uses `f64` seconds (the same
//! `TimeInterval` unit Swift's TTL arithmetic runs in), supplied by the host
//! so the cache logic stays pure and testable.

use crate::candidate_index::MentionCandidateIndex;

/// Swift: `struct TextBoxMentionCachedIndex`.
#[derive(Debug, Clone)]
pub struct MentionCachedIndex {
    pub index: MentionCandidateIndex,
    /// Seconds timestamp (Swift `createdAt: Date`).
    pub created_at: f64,
    /// Seconds timestamp (Swift `lastAccessedAt: Date`).
    pub last_accessed_at: f64,
    /// Seconds timestamp (Swift `refreshStartedAt: Date`).
    pub refresh_started_at: f64,
}
