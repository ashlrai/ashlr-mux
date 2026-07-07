// Port of the render-sequencing guard in
// `Sources/CommandPalette/CommandPaletteOverlay.swift:35-63`
// (`CommandPaletteOverlayRenderModel`). The guard decides whether an
// asynchronously-arriving command-list batch is fresh enough to apply, or must
// be dropped because a newer batch already won.
//
// This logic lives ONLY in the SwiftUI `@Observable` model, not in
// `crates/cmux-command-palette` — so this is a fresh port from Swift, not a
// wrapper over a ported Rust fn. The Swift `scheduleCommandListUpdate`
// semantics are the parity oracle.
//
// TWO-CLOCK DESIGN (load-bearing): there are two independent monotonic counters
//   - `sequence`: issued per *schedule call* inside the model (Swift line 46-47)
//   - `resultsVersion`: supplied by the *producer* per results mutation
// A batch is applied only when it is newest on BOTH axes (Swift lines 51-52).
// They must not be collapsed into one counter.
//
// The Swift `await Task.yield()` (line 50) is an async scheduling debounce, not
// part of the decision — so this guard is fully synchronous and `now`-free. The
// async hop belongs to the D4 host.
//
// DIVERGENCE (sanctioned, unreachable in practice): Swift uses `&+=` wrapping
// `UInt64`. JS `number` will not wrap at UI scale, so we use plain `number`.
// Same pattern already documented for `windowStore.ts`.

/** Internal counters of the render-sequencing guard (all start at 0). */
export interface RenderSequencingState {
  /** Monotonic issue counter, bumped on each `issue()` (Swift line 46-47). */
  scheduledSequence: number;
  /** Last-applied schedule sequence (Swift `appliedCommandListSequence`). */
  appliedSequence: number;
  /** Last-applied results version (Swift `appliedCommandListResultsVersion`). */
  appliedResultsVersion: number;
}

/**
 * The render-sequencing guard: a small pure state machine mirroring the state
 * fields + accept/drop decision of `CommandPaletteOverlayRenderModel`.
 *
 * Usage: call {@link issue} to stamp an async schedule request with a fresh id,
 * then {@link applyIfCurrent} (or {@link decide}) when the batch arrives to test
 * whether it is still the newest on both the sequence and results-version axes.
 */
export class RenderSequencingGuard {
  private scheduledSequence = 0;
  private appliedSequence = 0;
  private appliedResultsVersion = 0;

  /**
   * Increments and returns the next schedule sequence id (Swift line 46-47:
   * `scheduledCommandListSequence &+= 1; let sequence = scheduledCommandListSequence`).
   * The caller stamps its async request with this id and later passes it back to
   * {@link applyIfCurrent}.
   */
  issue(): number {
    this.scheduledSequence += 1;
    return this.scheduledSequence;
  }

  /**
   * Pure freshness test for an arriving batch (Swift lines 51-52), WITHOUT
   * mutating applied state. Returns `true` iff the batch is newest on both axes:
   * `sequence >= appliedSequence && resultsVersion >= appliedResultsVersion`.
   *
   * Both comparisons are `>=` (not `>`): a re-schedule at the same sequence with
   * a higher results version, or the same results version, is still accepted.
   */
  decide(sequence: number, resultsVersion: number): boolean {
    return (
      sequence >= this.appliedSequence &&
      resultsVersion >= this.appliedResultsVersion
    );
  }

  /**
   * The accept/drop transition (Swift lines 51-56). Returns `true` and advances
   * applied state when the batch is current; returns `false` and leaves state
   * untouched when it is stale.
   *
   * On accept: `appliedSequence = sequence` (line 53) and
   * `appliedResultsVersion = max(appliedResultsVersion, resultsVersion)` (line
   * 54) — note the `max`, not a plain assignment.
   */
  applyIfCurrent(sequence: number, resultsVersion: number): boolean {
    if (!this.decide(sequence, resultsVersion)) {
      return false;
    }
    this.appliedSequence = sequence;
    this.appliedResultsVersion = Math.max(
      this.appliedResultsVersion,
      resultsVersion,
    );
    return true;
  }

  /** A copy of the current internal counters (for inspection / debugging). */
  snapshot(): RenderSequencingState {
    return {
      scheduledSequence: this.scheduledSequence,
      appliedSequence: this.appliedSequence,
      appliedResultsVersion: this.appliedResultsVersion,
    };
  }
}
