use std::collections::HashSet;

use uuid::Uuid;

use crate::output::PaneMemoryGuardrailEngineOutput;
use crate::pane_key::PaneMemoryPaneKey;
use crate::sample::PaneMemorySample;

/// Stateless-per-call decision core for the guardrail. Owns only the
/// warned/dismissed sets so the threshold crossing logic (edge-trigger +
/// hysteresis) is testable without timers, ghostty, or libproc.
///
/// Port of Swift `struct PaneMemoryGuardrailEngine`. Divergences:
/// - Swift `mutating func` on a `struct` -> Rust `&mut self` methods (the
///   Swift type is already synchronous; no actor/async is involved).
/// - Swift `Set<...>` -> `std::collections::HashSet<...>` (order-independent).
/// - Swift `private(set) var` -> private fields plus the read-only accessor
///   methods [`warned_panes`](Self::warned_panes) /
///   [`dismissed_panes`](Self::dismissed_panes).
/// - Swift computed `warnedWorkspaceIds` -> the
///   [`warned_workspace_ids`](Self::warned_workspace_ids) method.
/// - `Int64(Double(thresholdBytes) * clearFraction)` ->
///   `(threshold_bytes as f64 * CLEAR_FRACTION) as i64`. Both truncate toward
///   zero. Edge divergence: Swift `Int64(_: Double)` traps on NaN/overflow
///   while Rust's `as` saturates; unreachable for realistic byte thresholds.
#[derive(Debug, Clone, Default)]
pub struct PaneMemoryGuardrailEngine {
    warned_panes: HashSet<PaneMemoryPaneKey>,
    dismissed_panes: HashSet<PaneMemoryPaneKey>,
}

impl PaneMemoryGuardrailEngine {
    /// Banner clears once a warned pane drops below `clearFraction × threshold`.
    /// The gap between warn and clear is hysteresis so a pane hovering at the
    /// threshold does not flap the badge/banner every tick.
    ///
    /// Faithful port of `static let clearFraction = 0.8`.
    pub const CLEAR_FRACTION: f64 = 0.8;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only view of the warned set (Swift `private(set) var warnedPanes`).
    #[must_use]
    pub fn warned_panes(&self) -> &HashSet<PaneMemoryPaneKey> {
        &self.warned_panes
    }

    /// Read-only view of the dismissed set (Swift `private(set) var dismissedPanes`).
    #[must_use]
    pub fn dismissed_panes(&self) -> &HashSet<PaneMemoryPaneKey> {
        &self.dismissed_panes
    }

    /// Faithful port of `var warnedWorkspaceIds: Set<UUID> { Set(warnedPanes.map(\.workspaceId)) }`.
    #[must_use]
    pub fn warned_workspace_ids(&self) -> HashSet<Uuid> {
        self.warned_panes.iter().map(|k| k.workspace_id).collect()
    }

    /// Faithful port of `ingest(samples:thresholdBytes:)` (line 17).
    ///
    /// Divergence: Swift takes `samples: [PaneMemorySample]` by value; here we
    /// borrow a slice since the engine only reads samples.
    pub fn ingest(
        &mut self,
        samples: &[PaneMemorySample],
        threshold_bytes: i64,
    ) -> PaneMemoryGuardrailEngineOutput {
        let clear_bytes = (threshold_bytes as f64 * Self::CLEAR_FRACTION) as i64;
        let live_keys: HashSet<PaneMemoryPaneKey> = samples.iter().map(PaneMemorySample::key).collect();
        // Forget panes that no longer exist so closed panes never keep a badge.
        self.warned_panes.retain(|k| live_keys.contains(k));
        self.dismissed_panes.retain(|k| live_keys.contains(k));

        let mut banners_to_present: Vec<crate::warning::PaneMemoryWarning> = Vec::new();
        let mut cleared_panes: HashSet<PaneMemoryPaneKey> = HashSet::new();

        for sample in samples {
            let key = sample.key();
            if sample.memory_bytes >= threshold_bytes {
                // Swift `warnedPanes.insert(key).inserted` always performs the
                // insert (side effect); `HashSet::insert` returns `true` when
                // the key was newly inserted, identical to `.inserted`.
                let inserted = self.warned_panes.insert(key);
                if inserted && !self.dismissed_panes.contains(&key) {
                    // First crossing (or first since it cleared) — fire once.
                    banners_to_present.push(sample.warning());
                }
            } else if sample.memory_bytes < clear_bytes {
                self.warned_panes.remove(&key);
                self.dismissed_panes.remove(&key);
                cleared_panes.insert(key);
            }
            // In the hysteresis band [clear_bytes, threshold_bytes): keep state.
        }

        PaneMemoryGuardrailEngineOutput {
            banners_to_present,
            warned_workspace_ids: self.warned_workspace_ids(),
            warned_pane_keys: self.warned_panes.clone(),
            cleared_panes,
        }
    }

    /// User dismissed the banner for `key`; suppress re-firing while it stays
    /// high. The badge persists until the pane drops below the clear level.
    ///
    /// Faithful port of `dismiss(_:)`.
    pub fn dismiss(&mut self, key: PaneMemoryPaneKey) {
        self.dismissed_panes.insert(key);
    }

    /// The pane's runaway tree was killed; drop its warned/dismissed state so a
    /// future leak re-warns cleanly.
    ///
    /// Faithful port of `acknowledgeHandled(_:)`.
    pub fn acknowledge_handled(&mut self, key: PaneMemoryPaneKey) {
        self.warned_panes.remove(&key);
        self.dismissed_panes.remove(&key);
    }

    /// Faithful port of `reset()`.
    pub fn reset(&mut self) {
        self.warned_panes.clear();
        self.dismissed_panes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::PaneMemoryDescriptor;
    use crate::warning::PaneMemoryWarning;

    const THRESHOLD: i64 = 1000;

    fn ws(n: u128) -> Uuid {
        Uuid::from_u128(0x1000_0000_0000_0000_0000_0000_0000_0000 + n)
    }

    fn panel(n: u128) -> Uuid {
        Uuid::from_u128(0x2000_0000_0000_0000_0000_0000_0000_0000 + n)
    }

    fn key(w: u128, p: u128) -> PaneMemoryPaneKey {
        PaneMemoryPaneKey::new(ws(w), panel(p))
    }

    /// Build a sample for pane (workspace `w`, panel `p`) reporting `bytes`.
    fn sample(w: u128, p: u128, bytes: i64) -> PaneMemorySample {
        PaneMemorySample {
            descriptor: PaneMemoryDescriptor {
                workspace_id: ws(w),
                panel_id: panel(p),
                workspace_title: format!("ws{w}"),
                pane_title: format!("pane{p}"),
                tty_name: Some(format!("/dev/ttys00{p}")),
                foreground_pid: Some(4000 + p as i64),
            },
            memory_bytes: bytes,
            resident_bytes: bytes,
            memory_pressure_process_group_ids: vec![],
            foreground_command: Some("node".to_string()),
        }
    }

    fn set_keys<const N: usize>(keys: [PaneMemoryPaneKey; N]) -> HashSet<PaneMemoryPaneKey> {
        keys.into_iter().collect()
    }

    fn set_ws<const N: usize>(ids: [Uuid; N]) -> HashSet<Uuid> {
        ids.into_iter().collect()
    }

    fn warning(w: u128, p: u128, bytes: i64) -> PaneMemoryWarning {
        PaneMemoryWarning {
            workspace_id: ws(w),
            panel_id: panel(p),
            workspace_title: format!("ws{w}"),
            pane_title: format!("pane{p}"),
            memory_bytes: bytes,
            foreground_command: Some("node".to_string()),
        }
    }

    // --- clear-fraction constant -----------------------------------------

    #[test]
    fn clear_fraction_is_0_8() {
        assert_eq!(PaneMemoryGuardrailEngine::CLEAR_FRACTION, 0.8);
    }

    // --- (a) below->above fires once; staying above does not re-fire ------

    #[test]
    fn oracle_a_below_then_above_fires_once() {
        let mut engine = PaneMemoryGuardrailEngine::new();

        // Below threshold and below clear level (500 < 800): no banner, and the
        // pane is reported cleared this tick even though it was never warned
        // (faithful to Swift's unconditional clearedPanes.insert on this branch).
        let out = engine.ingest(&[sample(1, 1, 500)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([]),
                warned_pane_keys: set_keys([]),
                cleared_panes: set_keys([key(1, 1)]),
            }
        );

        // Crossing up: fires exactly one banner carrying the current bytes.
        let out = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![warning(1, 1, 1500)],
                warned_workspace_ids: set_ws([ws(1)]),
                warned_pane_keys: set_keys([key(1, 1)]),
                cleared_panes: set_keys([]),
            }
        );
    }

    #[test]
    fn oracle_a_staying_above_does_not_refire() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD); // fires

        // Still above on the next tick: edge-trigger means no re-fire.
        let out = engine.ingest(&[sample(1, 1, 2000)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([ws(1)]),
                warned_pane_keys: set_keys([key(1, 1)]),
                cleared_panes: set_keys([]),
            }
        );
    }

    #[test]
    fn edge_trigger_fires_exactly_at_threshold() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        // memory_bytes == threshold uses `>=`, so it fires.
        let out = engine.ingest(&[sample(1, 1, THRESHOLD)], THRESHOLD);
        assert_eq!(out.banners_to_present, vec![warning(1, 1, THRESHOLD)]);
        assert_eq!(out.warned_pane_keys, set_keys([key(1, 1)]));
    }

    // --- (b) hysteresis band does not clear; below 0.8x clears -----------

    #[test]
    fn oracle_b_hysteresis_band_keeps_state() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD); // warned

        // 900 is in [clear_bytes=800, threshold=1000): keep warned, no clear.
        let out = engine.ingest(&[sample(1, 1, 900)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([ws(1)]),
                warned_pane_keys: set_keys([key(1, 1)]),
                cleared_panes: set_keys([]),
            }
        );
    }

    #[test]
    fn oracle_b_at_exact_clear_bytes_stays_warned() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD); // warned

        // clear_bytes = 800; the `< clear_bytes` test is strict, so exactly 800
        // is still in the hysteresis band and does NOT clear.
        let out = engine.ingest(&[sample(1, 1, 800)], THRESHOLD);
        assert_eq!(out.cleared_panes, set_keys([]));
        assert_eq!(out.warned_pane_keys, set_keys([key(1, 1)]));
    }

    #[test]
    fn oracle_b_below_clear_bytes_clears() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD); // warned

        // 799 < 800: drops warned+dismissed state and reports the clear.
        let out = engine.ingest(&[sample(1, 1, 799)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([]),
                warned_pane_keys: set_keys([]),
                cleared_panes: set_keys([key(1, 1)]),
            }
        );
        assert!(engine.warned_panes().is_empty());
    }

    #[test]
    fn clear_bytes_truncates_toward_zero() {
        // threshold 999 -> 999 * 0.8 = 799.2 -> truncated to 799.
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 5000)], 999); // warned

        // 799 is NOT < 799 -> still warned (band).
        let out = engine.ingest(&[sample(1, 1, 799)], 999);
        assert_eq!(out.cleared_panes, set_keys([]));
        assert_eq!(out.warned_pane_keys, set_keys([key(1, 1)]));

        // 798 < 799 -> clears.
        let out = engine.ingest(&[sample(1, 1, 798)], 999);
        assert_eq!(out.cleared_panes, set_keys([key(1, 1)]));
        assert!(out.warned_pane_keys.is_empty());
    }

    // --- (c) pane missing from batch is forgotten ------------------------

    #[test]
    fn oracle_c_missing_pane_is_forgotten() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD); // pane1 warned
        assert_eq!(engine.warned_panes(), &set_keys([key(1, 1)]));

        // Next batch omits pane1 entirely (only pane2, which is low). pane1 must
        // be dropped from warned state — no stale badge/warning.
        let out = engine.ingest(&[sample(1, 2, 100)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([]),
                warned_pane_keys: set_keys([]),
                cleared_panes: set_keys([key(1, 2)]),
            }
        );
        assert!(engine.warned_panes().is_empty());
    }

    #[test]
    fn empty_batch_forgets_everything() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        let out = engine.ingest(&[], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([]),
                warned_pane_keys: set_keys([]),
                cleared_panes: set_keys([]),
            }
        );
    }

    // --- (d) dismiss suppresses re-fire; drop+recross re-fires -----------

    #[test]
    fn oracle_d_dismiss_then_stay_high_no_warning() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let out = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        assert_eq!(out.banners_to_present, vec![warning(1, 1, 1500)]);

        engine.dismiss(key(1, 1));
        assert_eq!(engine.dismissed_panes(), &set_keys([key(1, 1)]));

        // Stays high: no new banner (still warned, edge-trigger + dismissed).
        let out = engine.ingest(&[sample(1, 1, 1800)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([ws(1)]),
                warned_pane_keys: set_keys([key(1, 1)]),
                cleared_panes: set_keys([]),
            }
        );
    }

    #[test]
    fn oracle_d_dismiss_before_first_crossing_suppresses_banner() {
        // Directly exercises the `!dismissedPanes.contains(key)` guard: a pane
        // dismissed while NOT warned is inserted into warned on the crossing
        // (inserted == true) but the banner is suppressed.
        let mut engine = PaneMemoryGuardrailEngine::new();
        engine.dismiss(key(1, 1));

        let out = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        assert_eq!(
            out,
            PaneMemoryGuardrailEngineOutput {
                banners_to_present: vec![],
                warned_workspace_ids: set_ws([ws(1)]),
                warned_pane_keys: set_keys([key(1, 1)]),
                cleared_panes: set_keys([]),
            }
        );
    }

    #[test]
    fn oracle_d_drop_below_clear_then_recross_refires() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD); // fires
        engine.dismiss(key(1, 1));

        // Drop below clear: clears warned AND dismissed state.
        let out = engine.ingest(&[sample(1, 1, 100)], THRESHOLD);
        assert_eq!(out.cleared_panes, set_keys([key(1, 1)]));
        assert!(engine.warned_panes().is_empty());
        assert!(engine.dismissed_panes().is_empty());

        // Re-crossing fires again cleanly (dismiss did not survive the clear).
        let out = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        assert_eq!(out.banners_to_present, vec![warning(1, 1, 1500)]);
        assert_eq!(out.warned_pane_keys, set_keys([key(1, 1)]));
    }

    // --- (e) reset empties state -----------------------------------------

    #[test]
    fn oracle_e_reset_empties_state() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        engine.dismiss(key(1, 1));
        assert!(!engine.warned_panes().is_empty());
        assert!(!engine.dismissed_panes().is_empty());

        engine.reset();
        assert!(engine.warned_panes().is_empty());
        assert!(engine.dismissed_panes().is_empty());

        // After reset the same high pane fires again from a clean slate.
        let out = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        assert_eq!(out.banners_to_present, vec![warning(1, 1, 1500)]);
    }

    // --- acknowledgeHandled ----------------------------------------------

    #[test]
    fn acknowledge_handled_drops_state_and_allows_refire() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let _ = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        engine.dismiss(key(1, 1));

        engine.acknowledge_handled(key(1, 1));
        assert!(engine.warned_panes().is_empty());
        assert!(engine.dismissed_panes().is_empty());

        // A future spike re-warns from a clean slate even while staying high.
        let out = engine.ingest(&[sample(1, 1, 1500)], THRESHOLD);
        assert_eq!(out.banners_to_present, vec![warning(1, 1, 1500)]);
    }

    // --- multi-pane aggregation ------------------------------------------

    #[test]
    fn multiple_panes_cross_in_batch_order() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        // pane(1,1) and pane(1,2) both cross; pane(2,3) stays low.
        let out = engine.ingest(
            &[
                sample(1, 1, 1500),
                sample(2, 3, 100),
                sample(1, 2, 1200),
            ],
            THRESHOLD,
        );
        // Banners follow sample order (Vec is order-dependent, matching Swift).
        assert_eq!(
            out.banners_to_present,
            vec![warning(1, 1, 1500), warning(1, 2, 1200)]
        );
        // Two distinct workspaces warned would be 2; both warned panes here are
        // in workspace 1, so warnedWorkspaceIds collapses to a single id.
        assert_eq!(out.warned_workspace_ids, set_ws([ws(1)]));
        assert_eq!(out.warned_pane_keys, set_keys([key(1, 1), key(1, 2)]));
        assert_eq!(out.cleared_panes, set_keys([key(2, 3)]));
    }

    #[test]
    fn warned_workspace_ids_spans_multiple_workspaces() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let out = engine.ingest(
            &[sample(1, 1, 1500), sample(2, 2, 1500)],
            THRESHOLD,
        );
        assert_eq!(out.warned_workspace_ids, set_ws([ws(1), ws(2)]));
        assert_eq!(out.warned_pane_keys, set_keys([key(1, 1), key(2, 2)]));
    }

    #[test]
    fn banner_to_present_returns_first() {
        let mut engine = PaneMemoryGuardrailEngine::new();
        let out = engine.ingest(
            &[sample(1, 1, 1500), sample(1, 2, 1200)],
            THRESHOLD,
        );
        assert_eq!(out.banner_to_present(), Some(&warning(1, 1, 1500)));
    }

    #[test]
    fn banner_to_present_is_none_when_empty() {
        let out = PaneMemoryGuardrailEngineOutput {
            banners_to_present: vec![],
            warned_workspace_ids: set_ws([]),
            warned_pane_keys: set_keys([]),
            cleared_panes: set_keys([]),
        };
        assert_eq!(out.banner_to_present(), None);
    }

    // --- value-type accessors --------------------------------------------

    #[test]
    fn warning_id_is_panel_id_and_key_matches() {
        let w = warning(7, 9, 1234);
        assert_eq!(w.id(), panel(9));
        assert_eq!(w.key(), key(7, 9));
    }

    #[test]
    fn sample_warning_and_key_map_from_descriptor() {
        let s = sample(3, 4, 4242);
        assert_eq!(s.key(), key(3, 4));
        assert_eq!(s.warning(), warning(3, 4, 4242));
    }
}
