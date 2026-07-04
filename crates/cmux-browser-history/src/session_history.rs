//! Restored browser session-history replay state machine.
//!
//! Headless port of the `Navigation` value types from the canonical macOS
//! `Packages/macOS/CmuxBrowser` package:
//!
//! - `RestoredSessionHistory.swift` — the pure back/forward replay state machine.
//! - `SessionHistoryURLSanitizer.swift` — URL eligibility + serialization rules.
//! - `SessionHistoryTraversalDecision.swift` — the traversal-outcome enum.
//! - `NavigationAvailability.swift` — resolved back/forward availability.
//! - `SessionNavigationHistorySnapshot.swift` — persisted back/forward strings.
//!
//! WebKit cannot rehydrate a `WKBackForwardList` from serialized URLs, so cmux
//! keeps its own restored stacks and replays them by issuing fresh navigations.
//! Once the live page accumulates native history, traversal defers to WebKit and
//! the restored stacks are realigned to track the live current entry. This is a
//! pure value-semantics state machine: it owns the stacks/flags and is driven by
//! the surface, which supplies the live current URL and performs the resulting
//! `WKWebView` calls. It reaches no WebKit/AppKit/app-target type. Its one
//! impurity — classifying diff-viewer/loopback *temporary* URLs — is inverted
//! into the injected [`SessionHistoryURLSanitizer::new`] `is_temporary` seam,
//! exactly as in Swift.
//!
//! ## DIVERGENCE: `url` crate vs Foundation `URL`
//!
//! Swift persists `URL.absoluteString`, which preserves the string exactly as
//! parsed. The `url` crate normalizes: for an authority-only http(s) URL such as
//! `https://a.test` it fills in the empty path as `/`, so `Url::as_str()` yields
//! `https://a.test/`. This matches the divergence already documented in
//! `lib.rs` (`candidate`). All *`Url`-value* comparisons are unaffected because
//! both sides of every comparison route through `Url::parse`; only assertions on
//! *serialized strings* (snapshot output, `clearedForward` payload) carry the
//! trailing slash. Ported oracle tests adjust those expected strings and flag
//! each spot with a `DIVERGENCE` comment.

use url::Url;

/// The injected temporary-URL classification seam. Boxed so the sanitizer is a
/// concrete value type (mirrors the Swift `@Sendable (URL?) -> Bool` closure).
type IsTemporaryFn = Box<dyn Fn(Option<&Url>) -> bool + Send + Sync>;

/// Normalizes URLs for session-history persistence and replay.
///
/// Port of `SessionHistoryURLSanitizer` (`SessionHistoryURLSanitizer.swift`).
///
/// Two rules govern which URLs are eligible: a URL must be non-empty and not
/// `about:blank`, and it must not be a *temporary* URL (a diff-viewer
/// custom-scheme URL or a remote loopback proxy alias) whose backing server is
/// gone after a restart. The temporary-URL classification depends on app-target
/// types, so it is inverted into the injected `is_temporary` seam rather than
/// reached for here.
pub struct SessionHistoryURLSanitizer {
    is_temporary: IsTemporaryFn,
}

impl SessionHistoryURLSanitizer {
    /// Creates a sanitizer.
    ///
    /// Port of `init(isTemporary:)`.
    ///
    /// - `is_temporary`: returns `true` when a URL is a transient session-history
    ///   URL (diff viewer or remote loopback proxy alias) that must never be
    ///   persisted or replayed across restarts.
    pub fn new(is_temporary: impl Fn(Option<&Url>) -> bool + Send + Sync + 'static) -> Self {
        Self {
            is_temporary: Box::new(is_temporary),
        }
    }

    /// Returns whether a URL is a transient session-history URL.
    ///
    /// Port of `isTemporarySessionHistoryURL(_:)`.
    pub fn is_temporary_session_history_url(&self, url: Option<&Url>) -> bool {
        (self.is_temporary)(url)
    }

    /// Returns the serialized string for a URL, or `None` when the URL is not
    /// eligible for session-history persistence.
    ///
    /// Port of `serializableSessionHistoryURLString(_:)`.
    pub fn serializable_session_history_url_string(&self, url: Option<&Url>) -> Option<String> {
        let url = url?;
        if (self.is_temporary)(Some(url)) {
            return None;
        }
        // Swift: url.absoluteString.trimmingCharacters(in: .whitespacesAndNewlines).
        let value = url.as_str().trim();
        if value.is_empty() || value == "about:blank" {
            return None;
        }
        Some(value.to_string())
    }

    /// Parses a stored history string into an eligible URL, or `None` when the
    /// string is empty, `about:blank`, unparseable, or temporary.
    ///
    /// Port of `sanitizedSessionHistoryURL(_:)`.
    pub fn sanitized_session_history_url(&self, raw: Option<&str>) -> Option<Url> {
        let raw = raw?;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "about:blank" {
            return None;
        }
        let url = Url::parse(trimmed).ok()?;
        if (self.is_temporary)(Some(&url)) {
            return None;
        }
        Some(url)
    }

    /// Maps a list of stored history strings to eligible URLs, dropping any that
    /// fail [`sanitized_session_history_url`](Self::sanitized_session_history_url).
    ///
    /// Port of `sanitizedSessionHistoryURLs(_:)`.
    pub fn sanitized_session_history_urls<S: AsRef<str>>(&self, values: &[S]) -> Vec<Url> {
        values
            .iter()
            .filter_map(|value| self.sanitized_session_history_url(Some(value.as_ref())))
            .collect()
    }
}

/// The resolved back/forward availability for a browser surface.
///
/// Port of `NavigationAvailability` (`NavigationAvailability.swift`). Combines
/// the live WebKit `canGoBack`/`canGoForward` flags with any restored
/// session-history stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigationAvailability {
    /// Whether back navigation is possible from the current entry.
    pub can_go_back: bool,
    /// Whether forward navigation is possible from the current entry.
    pub can_go_forward: bool,
}

impl NavigationAvailability {
    /// Creates an availability value. Port of `init(canGoBack:canGoForward:)`.
    pub fn new(can_go_back: bool, can_go_forward: bool) -> Self {
        Self {
            can_go_back,
            can_go_forward,
        }
    }
}

/// The action a browser surface should take to satisfy a back/forward request
/// while restored session history is active.
///
/// Port of `SessionHistoryTraversalDecision`
/// (`SessionHistoryTraversalDecision.swift`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionHistoryTraversalDecision {
    /// Pop the restored stack and navigate to this URL as a non-typed,
    /// history-preserving load.
    Navigate(Url),
    /// Defer to WebKit's native `goBack()`.
    NativeGoBack,
    /// Defer to WebKit's native `goForward()`.
    NativeGoForward,
    /// No traversal is possible; the caller should only refresh availability.
    RefreshOnly,
}

/// The back/forward URL strings captured for session persistence.
///
/// Port of `SessionNavigationHistorySnapshot`
/// (`SessionNavigationHistorySnapshot.swift`). `back_history_url_strings` is
/// ordered oldest-first (WebKit `backForwardList.backList` order);
/// `forward_history_url_strings` is ordered nearest-forward-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionNavigationHistorySnapshot {
    /// Back-list URLs, oldest first.
    pub back_history_url_strings: Vec<String>,
    /// Forward-list URLs, nearest-forward first.
    pub forward_history_url_strings: Vec<String>,
}

impl SessionNavigationHistorySnapshot {
    /// Creates a snapshot. Port of
    /// `init(backHistoryURLStrings:forwardHistoryURLStrings:)`.
    pub fn new(back_history_url_strings: Vec<String>, forward_history_url_strings: Vec<String>) -> Self {
        Self {
            back_history_url_strings,
            forward_history_url_strings,
        }
    }
}

/// The outcome of realigning the restored stacks to a live current URL.
///
/// Port of `RestoredSessionHistory.RealignOutcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealignOutcome {
    /// Replay inactive or live current unresolved/already current: no change.
    NoChange,
    /// Stacks were rebalanced around the live current entry.
    Rebalanced,
    /// The live current was not found in either stack and the forward stack was
    /// cleared. The caller should emit the forward-clear debug log.
    ClearedForward {
        /// The serialized live current URL that was not found in either stack.
        live_current_string: String,
    },
}

/// The replayable back/forward history a browser surface restores from a prior
/// launch, plus the native WebKit availability flags it is reconciled against.
///
/// Port of `RestoredSessionHistory` (`RestoredSessionHistory.swift`).
///
/// Storage convention: `back` is ordered oldest-first; `forward` is ordered with
/// the nearest-forward entry at the end so traversal is a `pop()`.
pub struct RestoredSessionHistory {
    uses_restored_session_history: bool,
    back: Vec<Url>,
    forward: Vec<Url>,
    current: Option<Url>,
    sanitizer: SessionHistoryURLSanitizer,
}

impl RestoredSessionHistory {
    /// Creates an empty, inactive restored history.
    ///
    /// Port of `init(sanitizer:)`.
    pub fn new(sanitizer: SessionHistoryURLSanitizer) -> Self {
        Self {
            uses_restored_session_history: false,
            back: Vec::new(),
            forward: Vec::new(),
            current: None,
            sanitizer,
        }
    }

    /// Whether restored session-history replay is currently active. When `false`
    /// the surface uses WebKit's native back-forward list exclusively.
    ///
    /// Port of the `usesRestoredSessionHistory` stored property.
    pub fn uses_restored_session_history(&self) -> bool {
        self.uses_restored_session_history
    }

    /// Back-list URLs, oldest first. Port of the `back` stored property.
    pub fn back(&self) -> &[Url] {
        &self.back
    }

    /// Forward-list URLs with the nearest-forward entry last. Port of the
    /// `forward` stored property.
    pub fn forward(&self) -> &[Url] {
        &self.forward
    }

    /// The URL of the entry the restored history currently points at. Port of
    /// the `current` stored property.
    pub fn current(&self) -> Option<&Url> {
        self.current.as_ref()
    }

    /// Computes back/forward availability by combining native WebKit flags with
    /// any restored stacks. When restored history is inactive the native flags
    /// pass through unchanged.
    ///
    /// Port of `availability(nativeCanGoBack:nativeCanGoForward:)`.
    pub fn availability(&self, native_can_go_back: bool, native_can_go_forward: bool) -> NavigationAvailability {
        if self.uses_restored_session_history {
            return NavigationAvailability::new(
                native_can_go_back || !self.back.is_empty(),
                native_can_go_forward || !self.forward.is_empty(),
            );
        }
        NavigationAvailability::new(native_can_go_back, native_can_go_forward)
    }

    /// Whether anything would need clearing if the surface's workspace context
    /// changes (any restored state present).
    ///
    /// Port of the `hasRestoredState` computed property.
    pub fn has_restored_state(&self) -> bool {
        self.current.is_some() || !self.back.is_empty() || !self.forward.is_empty()
    }

    /// Returns whether the resolved live current URL matches the restored current
    /// entry. Treats either side being non-serializable as aligned, matching the
    /// surface's reconciliation contract.
    ///
    /// Port of `isLiveAligned(withLiveCurrentURL:)`.
    pub fn is_live_aligned(&self, live_current_url: Option<&Url>) -> bool {
        let live_current = self.sanitizer.serializable_session_history_url_string(live_current_url);
        let restored_current = self
            .sanitizer
            .serializable_session_history_url_string(self.current.as_ref());
        match (live_current, restored_current) {
            (Some(live), Some(restored)) => live == restored,
            // Swift: `guard let liveCurrent, let restoredCurrent else { return true }`.
            _ => true,
        }
    }

    /// Loads restored stacks from persisted strings. Activates replay only when
    /// at least one eligible URL survives sanitization. `forward_history_url_strings`
    /// is supplied nearest-forward-first and stored reversed so traversal pops
    /// from the end.
    ///
    /// Port of
    /// `restore(backHistoryURLStrings:forwardHistoryURLStrings:currentURLString:)`.
    ///
    /// Returns `true` when replay became active (the caller should refresh
    /// availability), `false` when nothing eligible was restored.
    pub fn restore<S: AsRef<str>>(
        &mut self,
        back_history_url_strings: &[S],
        forward_history_url_strings: &[S],
        current_url_string: Option<&str>,
    ) -> bool {
        let restored_back = self.sanitizer.sanitized_session_history_urls(back_history_url_strings);
        let restored_forward = self
            .sanitizer
            .sanitized_session_history_urls(forward_history_url_strings);
        let restored_current = self.sanitizer.sanitized_session_history_url(current_url_string);
        if restored_back.is_empty() && restored_forward.is_empty() && restored_current.is_none() {
            return false;
        }

        self.uses_restored_session_history = true;
        self.back = restored_back;
        // Swift: `forward = Array(restoredForward.reversed())` — nearest-forward last.
        self.forward = restored_forward.into_iter().rev().collect();
        self.current = restored_current;
        true
    }

    /// Serializes a sequence of URLs via the sanitizer, dropping any that the
    /// sanitizer rejects. The caller controls ordering (e.g. `.iter()` vs
    /// `.iter().rev()`) so this preserves each call site's behavior exactly.
    fn serialize<'a>(&self, urls: impl Iterator<Item = &'a Url>) -> Vec<String> {
        urls.filter_map(|url| self.sanitizer.serializable_session_history_url_string(Some(url)))
            .collect()
    }

    /// Captures the current back/forward URLs for persistence, given the native
    /// WebKit back/forward lists and the live alignment.
    ///
    /// Port of `snapshot(nativeBackURLs:nativeForwardURLs:isLiveAligned:)`.
    pub fn snapshot(
        &self,
        native_back_urls: &[Url],
        native_forward_urls: &[Url],
        is_live_aligned: bool,
    ) -> SessionNavigationHistorySnapshot {
        let native_back: Vec<String> = self.serialize(native_back_urls.iter());
        let native_forward: Vec<String> = self.serialize(native_forward_urls.iter());

        if self.uses_restored_session_history {
            let back_strings: Vec<String> = self.serialize(self.back.iter());
            // `forward` is stored nearest-forward-last; reverse to serialize
            // nearest-forward-first, matching Swift `forward.reversed()`.
            let restored_forward: Vec<String> = self.serialize(self.forward.iter().rev());

            if is_live_aligned {
                return SessionNavigationHistorySnapshot::new(
                    back_strings,
                    if restored_forward.is_empty() {
                        native_forward
                    } else {
                        restored_forward
                    },
                );
            }

            // Swift: `backStrings + nativeBack`.
            let mut back_plus_native = back_strings;
            back_plus_native.extend(native_back);
            return SessionNavigationHistorySnapshot::new(back_plus_native, native_forward);
        }

        SessionNavigationHistorySnapshot::new(native_back, native_forward)
    }

    /// Realigns the restored stacks when WebKit navigated to an entry that is not
    /// the restored current (e.g. an in-page link from a replayed page). If the
    /// live entry is found in the back stack, entries after it move to forward;
    /// if found in the forward stack, entries before it move to back; otherwise
    /// the now-stale forward stack is cleared.
    ///
    /// Port of `realign(toLiveCurrentURL:)`.
    pub fn realign(&mut self, live_current_url: Option<&Url>) -> RealignOutcome {
        if !self.uses_restored_session_history {
            return RealignOutcome::NoChange;
        }
        let live_current_string = match self.sanitizer.serializable_session_history_url_string(live_current_url) {
            Some(value) => value,
            None => return RealignOutcome::NoChange,
        };
        // Swift: `guard serializable(current) != liveCurrentString else { return .noChange }`.
        if self
            .sanitizer
            .serializable_session_history_url_string(self.current.as_ref())
            .as_deref()
            == Some(live_current_string.as_str())
        {
            return RealignOutcome::NoChange;
        }

        let restored_back: Vec<String> = self.serialize(self.back.iter());
        let restored_forward: Vec<String> = self.serialize(self.forward.iter().rev());
        let restored_current = self
            .sanitizer
            .serializable_session_history_url_string(self.current.as_ref());

        // Swift: `restoredBack.lastIndex(of: liveCurrentString)`.
        if let Some(back_index) = restored_back.iter().rposition(|value| value == &live_current_string) {
            let new_back: Vec<String> = restored_back[..back_index].to_vec();
            let mut new_forward: Vec<String> = restored_back[back_index + 1..].to_vec();
            if let Some(current) = &restored_current {
                new_forward.push(current.clone());
            }
            new_forward.extend(restored_forward.iter().cloned());

            self.back = self.sanitizer.sanitized_session_history_urls(&new_back);
            self.forward = self
                .sanitizer
                .sanitized_session_history_urls(&new_forward)
                .into_iter()
                .rev()
                .collect();
            self.current = live_current_url.cloned();
            return RealignOutcome::Rebalanced;
        }

        // Swift: `restoredForward.firstIndex(of: liveCurrentString)`.
        if let Some(forward_index) = restored_forward.iter().position(|value| value == &live_current_string) {
            let mut new_back: Vec<String> = restored_back;
            if let Some(current) = &restored_current {
                new_back.push(current.clone());
            }
            new_back.extend(restored_forward[..forward_index].iter().cloned());
            let new_forward: Vec<String> = restored_forward[forward_index + 1..].to_vec();

            self.back = self.sanitizer.sanitized_session_history_urls(&new_back);
            self.forward = self
                .sanitizer
                .sanitized_session_history_urls(&new_forward)
                .into_iter()
                .rev()
                .collect();
            self.current = live_current_url.cloned();
            return RealignOutcome::Rebalanced;
        }

        if self.forward.is_empty() {
            return RealignOutcome::NoChange;
        }
        self.forward.clear();
        RealignOutcome::ClearedForward { live_current_string }
    }

    /// Decides how to satisfy a back request while replay is active.
    ///
    /// Port of `decideGoBack(isLiveAligned:nativeCanGoBack:resolvedCurrentURL:)`.
    pub fn decide_go_back(
        &mut self,
        is_live_aligned: bool,
        native_can_go_back: bool,
        resolved_current_url: Option<&Url>,
    ) -> SessionHistoryTraversalDecision {
        // Swift combines the boolean with `let targetURL = popBack()`: the pop is
        // only attempted when the boolean holds, and a `None` pop falls through.
        if is_live_aligned || !native_can_go_back {
            if let Some(target) = self.back.pop() {
                if let Some(resolved) = resolved_current_url {
                    self.forward.push(resolved.clone());
                }
                self.current = Some(target.clone());
                return SessionHistoryTraversalDecision::Navigate(target);
            }
        }

        if native_can_go_back {
            return SessionHistoryTraversalDecision::NativeGoBack;
        }

        SessionHistoryTraversalDecision::RefreshOnly
    }

    /// Decides how to satisfy a forward request while replay is active.
    ///
    /// Port of `decideGoForward(nativeCanGoForward:resolvedCurrentURL:)`.
    pub fn decide_go_forward(
        &mut self,
        native_can_go_forward: bool,
        resolved_current_url: Option<&Url>,
    ) -> SessionHistoryTraversalDecision {
        if native_can_go_forward {
            return SessionHistoryTraversalDecision::NativeGoForward;
        }

        let target = match self.forward.pop() {
            Some(target) => target,
            None => return SessionHistoryTraversalDecision::RefreshOnly,
        };
        if let Some(resolved) = resolved_current_url {
            self.back.push(resolved.clone());
        }
        self.current = Some(target.clone());
        SessionHistoryTraversalDecision::Navigate(target)
    }

    /// Deactivates replay and clears every restored stack. No-op when replay is
    /// already inactive.
    ///
    /// Port of `abandon()`. Returns `true` when replay was active and is now
    /// cleared (the caller should refresh availability), `false` otherwise.
    pub fn abandon(&mut self) -> bool {
        if !self.uses_restored_session_history {
            return false;
        }
        self.uses_restored_session_history = false;
        self.back.clear();
        self.forward.clear();
        self.current = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sanitizer that treats `cmux-diff://` URLs as temporary, mirroring the
    /// app-side diff-viewer classification without depending on app types.
    /// Port of `RestoredSessionHistoryTests.makeSanitizer`.
    ///
    /// The `url` crate lowercases schemes, so the Swift `.lowercased()` is
    /// implicit here.
    fn make_sanitizer() -> SessionHistoryURLSanitizer {
        SessionHistoryURLSanitizer::new(|url| url.map(Url::scheme) == Some("cmux-diff"))
    }

    /// Port of the `url(_:)` test helper (`URL(string:)!`).
    fn u(string: &str) -> Url {
        Url::parse(string).unwrap()
    }

    // ---- RestoredSessionHistory oracle ----

    // Port of `restoreActivates`.
    #[test]
    fn restore_activates_and_stores_forward_reversed() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        let became = history.restore(
            &["https://a.test", "https://b.test"],
            &["https://d.test", "https://e.test"],
            Some("https://c.test"),
        );
        assert!(became);
        assert!(history.uses_restored_session_history());
        assert_eq!(history.back(), &[u("https://a.test"), u("https://b.test")]);
        // forward stored nearest-forward-last
        assert_eq!(history.forward(), &[u("https://e.test"), u("https://d.test")]);
        assert_eq!(history.current(), Some(&u("https://c.test")));
    }

    // Port of `restoreRejectsTemporary`.
    #[test]
    fn restore_with_only_temporary_or_empty_entries_does_not_activate() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        let became = history.restore(
            &["cmux-diff://token", "about:blank", "  "],
            &[],
            Some("cmux-diff://token"),
        );
        assert!(!became);
        assert!(!history.uses_restored_session_history());
        assert!(history.back().is_empty());
        assert_eq!(history.current(), None);
    }

    // Port of `availabilityInactive`.
    #[test]
    fn availability_passes_native_flags_through_when_inactive() {
        let history = RestoredSessionHistory::new(make_sanitizer());
        assert_eq!(
            history.availability(true, false),
            NavigationAvailability::new(true, false)
        );
    }

    // Port of `availabilityActive`.
    #[test]
    fn availability_ors_restored_stacks_with_native_flags_when_active() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        assert_eq!(
            history.availability(false, false),
            NavigationAvailability::new(true, false)
        );
        assert_eq!(
            history.availability(false, true),
            NavigationAvailability::new(true, true)
        );
    }

    // Port of `goBackPops`.
    #[test]
    fn go_back_pops_restored_back_and_pushes_current_to_forward() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test", "https://b.test"], &[], Some("https://c.test"));
        let decision = history.decide_go_back(true, false, Some(&u("https://c.test")));
        assert_eq!(decision, SessionHistoryTraversalDecision::Navigate(u("https://b.test")));
        assert_eq!(history.current(), Some(&u("https://b.test")));
        assert_eq!(history.back(), &[u("https://a.test")]);
        assert_eq!(history.forward(), &[u("https://c.test")]);
    }

    // Port of `goBackNative`.
    #[test]
    fn go_back_defers_to_native_when_not_aligned_and_native_can_go_back() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        let decision = history.decide_go_back(false, true, Some(&u("https://c.test")));
        assert_eq!(decision, SessionHistoryTraversalDecision::NativeGoBack);
        assert_eq!(history.back(), &[u("https://a.test")]);
    }

    // Port of `goBackRefreshOnly`.
    #[test]
    fn go_back_with_empty_stack_and_no_native_history_refreshes_only() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&[], &["https://d.test"], Some("https://c.test"));
        let decision = history.decide_go_back(true, false, Some(&u("https://c.test")));
        assert_eq!(decision, SessionHistoryTraversalDecision::RefreshOnly);
    }

    // Port of `goForwardNative`.
    #[test]
    fn go_forward_prefers_native_when_available() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&[], &["https://d.test"], Some("https://c.test"));
        let decision = history.decide_go_forward(true, Some(&u("https://c.test")));
        assert_eq!(decision, SessionHistoryTraversalDecision::NativeGoForward);
        assert_eq!(history.forward(), &[u("https://d.test")]);
    }

    // Port of `goForwardPops`.
    #[test]
    fn go_forward_pops_restored_forward_and_pushes_current_to_back() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&[], &["https://d.test", "https://e.test"], Some("https://c.test"));
        // forward stack: [e, d] (d is nearest-forward, last)
        let decision = history.decide_go_forward(false, Some(&u("https://c.test")));
        assert_eq!(decision, SessionHistoryTraversalDecision::Navigate(u("https://d.test")));
        assert_eq!(history.current(), Some(&u("https://d.test")));
        assert_eq!(history.back(), &[u("https://c.test")]);
        assert_eq!(history.forward(), &[u("https://e.test")]);
    }

    // Port of `snapshotAligned`.
    #[test]
    fn snapshot_when_aligned_returns_restored_back_and_forward() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &["https://d.test"], Some("https://c.test"));
        let snap = history.snapshot(
            &[u("https://native-b.test")],
            &[u("https://native-f.test")],
            true,
        );
        // DIVERGENCE: `url` crate normalizes authority-only URLs with a trailing
        // slash, so the serialized strings carry `/` where Foundation would not.
        assert_eq!(
            snap,
            SessionNavigationHistorySnapshot::new(
                vec!["https://a.test/".to_string()],
                vec!["https://d.test/".to_string()],
            )
        );
    }

    // Port of `snapshotMisaligned`.
    #[test]
    fn snapshot_when_not_aligned_concatenates_restored_back_with_native_back() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &["https://d.test"], Some("https://c.test"));
        let snap = history.snapshot(
            &[u("https://native-b.test")],
            &[u("https://native-f.test")],
            false,
        );
        // DIVERGENCE: trailing-slash normalization (see module docs).
        assert_eq!(
            snap,
            SessionNavigationHistorySnapshot::new(
                vec!["https://a.test/".to_string(), "https://native-b.test/".to_string()],
                vec!["https://native-f.test/".to_string()],
            )
        );
    }

    // Port of `snapshotInactive`.
    #[test]
    fn snapshot_inactive_returns_native_lists() {
        let history = RestoredSessionHistory::new(make_sanitizer());
        let snap = history.snapshot(
            &[u("https://native-b.test")],
            &[u("https://native-f.test")],
            true,
        );
        // DIVERGENCE: trailing-slash normalization (see module docs).
        assert_eq!(
            snap,
            SessionNavigationHistorySnapshot::new(
                vec!["https://native-b.test/".to_string()],
                vec!["https://native-f.test/".to_string()],
            )
        );
    }

    // Port of `realignFromBack`.
    #[test]
    fn realign_moves_entries_after_a_back_list_match_into_forward() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test", "https://b.test"], &[], Some("https://c.test"));
        // live navigated back to a.test
        let outcome = history.realign(Some(&u("https://a.test")));
        assert_eq!(outcome, RealignOutcome::Rebalanced);
        assert!(history.back().is_empty());
        assert_eq!(history.current(), Some(&u("https://a.test")));
        // forward (nearest-last) should hold b then c: stored reversed -> [c, b]
        assert_eq!(history.forward(), &[u("https://c.test"), u("https://b.test")]);
    }

    // Port of `realignClearsForward`.
    #[test]
    fn realign_clears_stale_forward_when_live_current_not_found() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &["https://d.test"], Some("https://c.test"));
        let outcome = history.realign(Some(&u("https://elsewhere.test")));
        // DIVERGENCE: trailing-slash normalization on the serialized payload.
        assert_eq!(
            outcome,
            RealignOutcome::ClearedForward {
                live_current_string: "https://elsewhere.test/".to_string(),
            }
        );
        assert!(history.forward().is_empty());
    }

    // Port of `realignNoChange`.
    #[test]
    fn realign_is_a_no_op_when_already_aligned() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        let outcome = history.realign(Some(&u("https://c.test")));
        assert_eq!(outcome, RealignOutcome::NoChange);
    }

    // Port of `abandonClears`.
    #[test]
    fn abandon_clears_all_restored_state() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &["https://d.test"], Some("https://c.test"));
        let abandoned = history.abandon();
        assert!(abandoned);
        assert!(!history.uses_restored_session_history());
        assert!(history.back().is_empty());
        assert!(history.forward().is_empty());
        assert_eq!(history.current(), None);
        // second abandon is a no-op
        let abandoned_again = history.abandon();
        assert!(!abandoned_again);
    }

    // ---- Parity-risk edge cases beyond the direct oracle ----

    /// `realign` finding the live entry inside the forward stack moves the
    /// entries before it (plus the old current) into back. This exercises the
    /// `firstIndex(of:)` branch that the Swift oracle does not cover directly;
    /// expectations are hand-computed from the Swift formula.
    #[test]
    fn realign_from_forward_moves_earlier_entries_into_back() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        // forward supplied nearest-forward-first [d, e]; stored reversed -> [e, d].
        history.restore(&["https://a.test"], &["https://d.test", "https://e.test"], Some("https://c.test"));
        // restoredForward (reversed back to nearest-first) = [d, e]; live nav to e.
        let outcome = history.realign(Some(&u("https://e.test")));
        assert_eq!(outcome, RealignOutcome::Rebalanced);
        // newBack = restoredBack[a] + current[c] + restoredForward[..e]=[d] -> [a, c, d]
        assert_eq!(
            history.back(),
            &[u("https://a.test"), u("https://c.test"), u("https://d.test")]
        );
        // newForward = restoredForward[after e] = [] -> stored reversed = []
        assert!(history.forward().is_empty());
        assert_eq!(history.current(), Some(&u("https://e.test")));
    }

    /// `realign` is a no-op when replay is inactive, before any URL work.
    /// Port of the `guard usesRestoredSessionHistory` early return.
    #[test]
    fn realign_is_no_op_when_inactive() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        assert_eq!(history.realign(Some(&u("https://a.test"))), RealignOutcome::NoChange);
    }

    /// `realign` with a non-serializable (temporary) live current is a no-op.
    /// Port of the `guard let liveCurrentString` early return.
    #[test]
    fn realign_is_no_op_when_live_current_not_serializable() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &["https://d.test"], Some("https://c.test"));
        assert_eq!(history.realign(Some(&u("cmux-diff://x"))), RealignOutcome::NoChange);
        // forward untouched
        assert_eq!(history.forward(), &[u("https://d.test")]);
    }

    /// `realign` that does not match either stack and has an empty forward stack
    /// returns `NoChange` (the `guard !forward.isEmpty` early return), not
    /// `ClearedForward`.
    #[test]
    fn realign_unmatched_with_empty_forward_is_no_change() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        assert_eq!(
            history.realign(Some(&u("https://elsewhere.test"))),
            RealignOutcome::NoChange
        );
    }

    /// `has_restored_state` reflects any restored stack/current presence.
    #[test]
    fn has_restored_state_tracks_presence() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        assert!(!history.has_restored_state());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        assert!(history.has_restored_state());
        history.abandon();
        assert!(!history.has_restored_state());
    }

    /// `is_live_aligned` treats a non-serializable side as aligned and compares
    /// serialized strings otherwise. Port of the reconciliation contract.
    #[test]
    fn is_live_aligned_matches_serialized_current() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        assert!(history.is_live_aligned(Some(&u("https://c.test"))));
        assert!(!history.is_live_aligned(Some(&u("https://other.test"))));
        // Non-serializable live current -> treated as aligned.
        assert!(history.is_live_aligned(Some(&u("cmux-diff://x"))));
        assert!(history.is_live_aligned(None));
    }

    /// `decide_go_back` prefers popping the restored back stack over native even
    /// when native can go back, as long as the live entry is aligned.
    #[test]
    fn decide_go_back_prefers_restored_when_aligned_even_if_native_available() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        let decision = history.decide_go_back(true, true, Some(&u("https://c.test")));
        assert_eq!(decision, SessionHistoryTraversalDecision::Navigate(u("https://a.test")));
    }

    /// `decide_go_back` with a `None` resolved current still pops but pushes
    /// nothing onto forward (Swift `if let resolvedCurrentURL`).
    #[test]
    fn decide_go_back_with_no_resolved_current_pushes_nothing() {
        let mut history = RestoredSessionHistory::new(make_sanitizer());
        history.restore(&["https://a.test"], &[], Some("https://c.test"));
        let decision = history.decide_go_back(true, false, None);
        assert_eq!(decision, SessionHistoryTraversalDecision::Navigate(u("https://a.test")));
        assert!(history.forward().is_empty());
    }

    // ---- SessionHistoryURLSanitizer oracle ----

    // Port of `SessionHistoryURLSanitizerTests.serializableRejects`.
    #[test]
    fn serializable_rejects_temporary_empty_and_about_blank() {
        let s = make_sanitizer();
        assert_eq!(
            s.serializable_session_history_url_string(Some(&u("cmux-diff://x"))),
            None
        );
        assert_eq!(
            s.serializable_session_history_url_string(Some(&u("about:blank"))),
            None
        );
        assert_eq!(s.serializable_session_history_url_string(None), None);
        // DIVERGENCE: trailing-slash normalization -> "https://ok.test/".
        assert_eq!(
            s.serializable_session_history_url_string(Some(&u("https://ok.test"))),
            Some("https://ok.test/".to_string())
        );
    }

    // Port of `SessionHistoryURLSanitizerTests.sanitizedParses`.
    #[test]
    fn sanitized_parses_eligible_strings_only() {
        let s = make_sanitizer();
        assert_eq!(s.sanitized_session_history_url(Some("  ")), None);
        assert_eq!(s.sanitized_session_history_url(Some("about:blank")), None);
        assert_eq!(s.sanitized_session_history_url(Some("cmux-diff://x")), None);
        assert_eq!(s.sanitized_session_history_url(None), None);
        assert_eq!(
            s.sanitized_session_history_url(Some("https://ok.test")),
            Some(u("https://ok.test"))
        );
        assert_eq!(
            s.sanitized_session_history_urls(&["https://a.test", "about:blank", "https://b.test"]),
            vec![u("https://a.test"), u("https://b.test")]
        );
    }
}
