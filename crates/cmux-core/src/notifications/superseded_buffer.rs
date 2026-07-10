//! Per-tab/surface stash of superseded phone-banner ids whose dismiss is
//! deferred until the replacement banner push is queued.
//!
//! Whole-file port of `cmux/Sources/SupersededPhoneDismissBuffer.swift`. Holds
//! opaque notification id strings only, never content; bounded per key.
//!
//! DEVIATION: Swift keys on `UUID`; the Rust notification model uses `String`
//! ids, so [`SupersededPhoneDismissBuffer::key`] takes `&str` tab/surface ids.
//! The `"<tab>:<surface-or-empty>"` key shape (and its `hasPrefix("<tab>:")`
//! tab-scoped match) is preserved exactly.

use std::collections::HashMap;

/// Per-tab/surface stash of superseded phone-banner ids.
#[derive(Debug, Clone, Default)]
pub struct SupersededPhoneDismissBuffer {
    ids_by_key: HashMap<String, Vec<String>>,
}

impl SupersededPhoneDismissBuffer {
    /// Max stashed banner ids for one tab/surface; oldest evicted past this.
    ///
    /// Verbatim from Swift `capacityPerKey` (`SupersededPhoneDismissBuffer.swift`
    /// 14).
    pub const CAPACITY_PER_KEY: usize = 64;

    pub fn new() -> Self {
        Self::default()
    }

    /// The stash key for a notification's tab/surface, mirroring the phone push
    /// throttle key.
    ///
    /// Verbatim port of Swift `key(tabId:surfaceId:)`
    /// (`SupersededPhoneDismissBuffer.swift` 18-20):
    /// `"<tab>:<surface-or-empty>"`.
    pub fn key(tab_id: &str, surface_id: Option<&str>) -> String {
        format!("{tab_id}:{}", surface_id.unwrap_or(""))
    }

    /// Park superseded banner ids until the replacement push is queued.
    /// Duplicates are kept once; the oldest evicted past [`Self::CAPACITY_PER_KEY`].
    ///
    /// Verbatim port of Swift `stash(ids:forKey:)`
    /// (`SupersededPhoneDismissBuffer.swift` 24-34).
    pub fn stash(&mut self, ids: &[String], key: &str) {
        if ids.is_empty() {
            return;
        }
        let pending = self.ids_by_key.entry(key.to_string()).or_default();
        for id in ids {
            if !pending.contains(id) {
                pending.push(id.clone());
            }
        }
        if pending.len() > Self::CAPACITY_PER_KEY {
            let overflow = pending.len() - Self::CAPACITY_PER_KEY;
            pending.drain(0..overflow);
        }
    }

    /// Take (and clear) everything stashed for the key, oldest first.
    ///
    /// Verbatim port of Swift `flush(forKey:)`
    /// (`SupersededPhoneDismissBuffer.swift` 37-39).
    pub fn flush_for_key(&mut self, key: &str) -> Vec<String> {
        self.ids_by_key.remove(key).unwrap_or_default()
    }

    /// Take (and clear) everything stashed under the given tab, for tab-scoped
    /// read/clear operations.
    ///
    /// Verbatim port of Swift `flush(matchingTabId:)`
    /// (`SupersededPhoneDismissBuffer.swift` 44-51): drains every key with the
    /// `"<tab>:"` prefix in sorted key order.
    pub fn flush_matching_tab_id(&mut self, tab_id: &str) -> Vec<String> {
        let prefix = format!("{tab_id}:");
        let mut keys: Vec<String> = self
            .ids_by_key
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        keys.sort();
        let mut drained: Vec<String> = Vec::new();
        for key in keys {
            if let Some(ids) = self.ids_by_key.remove(&key) {
                drained.extend(ids);
            }
        }
        drained
    }

    /// Take (and clear) everything stashed across all keys, for clear-all /
    /// mark-all-read operations.
    ///
    /// Verbatim port of Swift `flushAll()`
    /// (`SupersededPhoneDismissBuffer.swift` 55-58): sorted key order.
    pub fn flush_all(&mut self) -> Vec<String> {
        let mut keys: Vec<String> = self.ids_by_key.keys().cloned().collect();
        keys.sort();
        let mut drained: Vec<String> = Vec::new();
        for key in keys {
            if let Some(ids) = self.ids_by_key.remove(&key) {
                drained.extend(ids);
            }
        }
        self.ids_by_key.clear();
        drained
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// Oracle: `supersededPhoneDismissBufferAccumulatesAndFlushesOnce`.
    #[test]
    fn accumulates_dedupes_and_flushes_once() {
        let mut buffer = SupersededPhoneDismissBuffer::new();
        let key = SupersededPhoneDismissBuffer::key("tab", Some("surface"));

        buffer.stash(&ids(&["a"]), &key);
        buffer.stash(&ids(&["b", "a"]), &key); // replayed "a" kept once

        assert_eq!(buffer.flush_for_key(&key), ids(&["a", "b"]));
        assert_eq!(buffer.flush_for_key(&key), Vec::<String>::new());
    }

    /// Oracle: `supersededPhoneDismissBufferIsBoundedAndPerKey`.
    #[test]
    fn bounded_per_key_evicts_oldest() {
        let mut buffer = SupersededPhoneDismissBuffer::new();
        let hot = SupersededPhoneDismissBuffer::key("tab", Some("surface"));
        let other = SupersededPhoneDismissBuffer::key("tab2", None);

        let hot_ids: Vec<String> = (0..70).map(|n| format!("n-{n}")).collect();
        buffer.stash(&hot_ids, &hot);
        buffer.stash(&ids(&["x"]), &other);

        let flushed = buffer.flush_for_key(&hot);
        assert_eq!(
            flushed.len(),
            SupersededPhoneDismissBuffer::CAPACITY_PER_KEY
        );
        assert_eq!(flushed.first().map(String::as_str), Some("n-6")); // oldest evicted
        assert_eq!(flushed.last().map(String::as_str), Some("n-69"));
        assert_eq!(buffer.flush_for_key(&other), ids(&["x"])); // keys independent
    }

    /// Oracle: `supersededPhoneDismissBufferTabAndGlobalFlush`.
    #[test]
    fn tab_scoped_and_global_flush() {
        let mut buffer = SupersededPhoneDismissBuffer::new();
        buffer.stash(
            &ids(&["a1"]),
            &SupersededPhoneDismissBuffer::key("tabA", Some("s1")),
        );
        buffer.stash(
            &ids(&["a2"]),
            &SupersededPhoneDismissBuffer::key("tabA", None),
        );
        buffer.stash(
            &ids(&["b1"]),
            &SupersededPhoneDismissBuffer::key("tabB", Some("s1")),
        );

        let mut tab_a = buffer.flush_matching_tab_id("tabA");
        tab_a.sort();
        assert_eq!(tab_a, ids(&["a1", "a2"]));
        assert_eq!(buffer.flush_matching_tab_id("tabA"), Vec::<String>::new()); // once

        assert_eq!(buffer.flush_all(), ids(&["b1"]));
        assert_eq!(buffer.flush_all(), Vec::<String>::new());
    }

    #[test]
    fn empty_stash_is_noop() {
        let mut buffer = SupersededPhoneDismissBuffer::new();
        let key = SupersededPhoneDismissBuffer::key("tab", None);
        buffer.stash(&[], &key);
        assert_eq!(buffer.flush_for_key(&key), Vec::<String>::new());
    }

    #[test]
    fn key_shape_matches_swift() {
        assert_eq!(SupersededPhoneDismissBuffer::key("t", Some("s")), "t:s");
        assert_eq!(SupersededPhoneDismissBuffer::key("t", None), "t:");
    }
}
