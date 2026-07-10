//! Stateless pure decoders for `tmux -CC` control-mode message payloads.
//!
//! Ported from `RemoteTmuxControlMessageDecoding.swift`.
//!
//! These transform untrusted remote-tmux text (a `display-message` `key=value,…`
//! line, a captured stderr string, an optimistic window reorder) into the values
//! the mirror applies. They hold no state.

use std::collections::HashMap;

/// Builds the escape sequence that restores a pane's terminal state onto the
/// mirror surface, from a `display-message` `key=value,…` line. Sets the scroll
/// region (DECSTBM), the DEC private modes (wrap/cursor/insert/app-cursor-keys/
/// keypad), mouse tracking, origin mode, and finally the cursor position.
///
/// The cursor placement is emitted LAST on purpose: setting the scroll region
/// (DECSTBM) and changing origin mode (DECOM) each move the cursor to the home
/// position, so any earlier cursor placement would be lost. When origin mode is
/// on with a restricted region, tmux's absolute cursor row is translated to the
/// region-relative row the (origin-relative) CUP then expects.
pub fn pane_state_seed_sequence(line: &str) -> Vec<u8> {
    let mut fields: HashMap<String, String> = HashMap::new();
    // Swift `line.split(separator: ",")` omits empty subsequences by default.
    for pair in line.split(',').filter(|s| !s.is_empty()) {
        // Swift `pair.split(separator: "=", maxSplits: 1)` — one split at the
        // first `=`, omitting empty pieces. So a key with an empty side (e.g.
        // `=v` or `k=`) yields a single non-empty piece and is NOT recorded.
        let kv = split_first_equals_omitting_empty(pair);
        if kv.len() == 2 {
            fields.insert(kv[0].to_string(), kv[1].to_string());
        }
    }

    let on = |key: &str| fields.get(key).map(|v| v == "1").unwrap_or(false);
    // Clamp to a plausible terminal-dimension range: the values come from an
    // untrusted remote, and out-of-range or non-numeric values are treated as
    // absent.
    let num = |key: &str| -> Option<i64> {
        fields
            .get(key)
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|n| (0..=65535).contains(n))
    };

    // Reset SGR attributes first so the cursor's style pen starts from a known
    // baseline on a surface REUSED across reconnect.
    let mut seq = String::from("\u{1b}[m");

    // Scroll region (DECSTBM) — tmux reports 0-based, DECSTBM is 1-based. Only
    // seed a RESTRICTED region.
    let region_upper = num("scroll_region_upper");
    let mut restricted_region = false;
    if let (Some(upper), Some(lower)) = (region_upper, num("scroll_region_lower")) {
        if lower >= upper {
            let is_full_window =
                upper == 0 && num("pane_height").map(|h| lower == h - 1).unwrap_or(false);
            if !is_full_window {
                seq += &format!("\u{1b}[{};{}r", upper + 1, lower + 1);
                restricted_region = true;
            }
        }
    }

    seq += if on("wrap_flag") {
        "\u{1b}[?7h"
    } else {
        "\u{1b}[?7l"
    }; // DECAWM
    seq += if on("cursor_flag") {
        "\u{1b}[?25h"
    } else {
        "\u{1b}[?25l"
    }; // DECTCEM
    seq += if on("insert_flag") {
        "\u{1b}[4h"
    } else {
        "\u{1b}[4l"
    }; // IRM
    seq += if on("keypad_cursor_flag") {
        "\u{1b}[?1h"
    } else {
        "\u{1b}[?1l"
    }; // DECCKM
    seq += if on("keypad_flag") {
        "\u{1b}="
    } else {
        "\u{1b}>"
    }; // DECKPAM / DECKPNM

    // Reset all mouse tracking + encoding modes FIRST, then conditionally enable
    // the active one below.
    seq += "\u{1b}[?1000l\u{1b}[?1002l\u{1b}[?1003l\u{1b}[?1005l\u{1b}[?1006l";
    if on("mouse_all_flag") {
        seq += "\u{1b}[?1003h";
    } else if on("mouse_button_flag") {
        seq += "\u{1b}[?1002h";
    } else if on("mouse_standard_flag") {
        seq += "\u{1b}[?1000h";
    }
    if on("mouse_sgr_flag") {
        seq += "\u{1b}[?1006h";
    } else if on("mouse_utf8_flag") {
        seq += "\u{1b}[?1005h";
    }

    // Origin mode (DECOM) before the cursor — changing it homes the cursor.
    let origin_on = on("origin_flag");
    seq += if origin_on {
        "\u{1b}[?6h"
    } else {
        "\u{1b}[?6l"
    };

    // Cursor LAST. tmux reports an absolute row; with origin mode on and a
    // restricted region the CUP is interpreted region-relative, so subtract the
    // region top.
    if let (Some(cx), Some(cy)) = (num("cursor_x"), num("cursor_y")) {
        let row = if origin_on && restricted_region {
            (cy - region_upper.unwrap_or(0)).max(0)
        } else {
            cy
        };
        seq += &format!("\u{1b}[{};{}H", row + 1, cx + 1);
    }

    seq.into_bytes()
}

/// Returns `order` with the windows in `reordered` rearranged into `reordered`'s
/// sequence, leaving windows not in that set in their positions.
///
/// Mirrors Swift `windowOrder(_:applyingReorder:)`.
pub fn window_order(order: &[i64], reordered: &[i64]) -> Vec<i64> {
    let set: std::collections::HashSet<i64> = reordered.iter().copied().collect();
    let mut iter = reordered.iter().copied();
    order
        .iter()
        .map(|&value| {
            if set.contains(&value) {
                iter.next().unwrap_or(value)
            } else {
                value
            }
        })
        .collect()
}

/// Whether captured ssh/tmux stderr indicates the session/server is genuinely
/// gone (reconnect should stop and end) vs a transient transport failure (host
/// unreachable / connection refused — keep retrying).
pub fn stderr_indicates_session_gone(stderr: &str) -> bool {
    let lowered = stderr.to_lowercase();
    lowered.contains("can't find session")
        || lowered.contains("can\u{2019}t find session")
        || lowered.contains("no server running")
        || lowered.contains("no current session")
        || lowered.contains("session not found")
        || lowered.contains("lost server")
}

/// Splits `pair` at its first `=` and drops any empty side, matching Swift
/// `split(separator: "=", maxSplits: 1)` (which omits empty subsequences).
fn split_first_equals_omitting_empty(pair: &str) -> Vec<&str> {
    match pair.find('=') {
        Some(idx) => {
            let a = &pair[..idx];
            let b = &pair[idx + 1..];
            [a, b].into_iter().filter(|s| !s.is_empty()).collect()
        }
        None => {
            if pair.is_empty() {
                vec![]
            } else {
                vec![pair]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(line: &str) -> String {
        String::from_utf8(pane_state_seed_sequence(line)).unwrap()
    }

    #[test]
    fn baseline_sequence_starts_with_sgr_reset() {
        let s = seq("");
        assert!(s.starts_with("\u{1b}[m"));
    }

    #[test]
    fn default_flags_emit_off_forms() {
        let s = seq("");
        assert!(s.contains("\u{1b}[?7l")); // wrap off
        assert!(s.contains("\u{1b}[?25l")); // cursor hidden
        assert!(s.contains("\u{1b}[4l")); // insert off
        assert!(s.contains("\u{1b}[?1l")); // app cursor keys off
        assert!(s.contains("\u{1b}>")); // keypad numeric
        assert!(s.contains("\u{1b}[?6l")); // origin off
    }

    #[test]
    fn flags_on_emit_on_forms() {
        let s = seq("wrap_flag=1,cursor_flag=1,insert_flag=1,keypad_cursor_flag=1,keypad_flag=1,origin_flag=1");
        assert!(s.contains("\u{1b}[?7h"));
        assert!(s.contains("\u{1b}[?25h"));
        assert!(s.contains("\u{1b}[4h"));
        assert!(s.contains("\u{1b}[?1h"));
        assert!(s.contains("\u{1b}="));
        assert!(s.contains("\u{1b}[?6h"));
    }

    #[test]
    fn restricted_region_is_seeded_one_based() {
        // upper=2 lower=10 → DECSTBM "3;11r".
        let s = seq("scroll_region_upper=2,scroll_region_lower=10,pane_height=40");
        assert!(s.contains("\u{1b}[3;11r"));
    }

    #[test]
    fn full_window_region_is_not_seeded() {
        // upper=0 lower=height-1 → default region, not emitted.
        let s = seq("scroll_region_upper=0,scroll_region_lower=23,pane_height=24");
        assert!(!s.contains("\u{1b}[1;24r"));
    }

    #[test]
    fn cursor_placed_last_and_one_based() {
        let s = seq("cursor_x=5,cursor_y=3");
        assert!(s.ends_with("\u{1b}[4;6H"));
    }

    #[test]
    fn cursor_region_relative_under_origin_mode() {
        // origin on + restricted region upper=2 → row = cy - 2, then +1.
        let s = seq(
            "origin_flag=1,scroll_region_upper=2,scroll_region_lower=20,pane_height=40,cursor_x=0,cursor_y=5",
        );
        // row = max(0, 5-2)=3 → CUP "4;1H".
        assert!(s.ends_with("\u{1b}[4;1H"));
    }

    #[test]
    fn mouse_all_flag_enables_1003_and_resets_first() {
        let s = seq("mouse_all_flag=1,mouse_sgr_flag=1");
        assert!(s.contains("\u{1b}[?1000l\u{1b}[?1002l\u{1b}[?1003l\u{1b}[?1005l\u{1b}[?1006l"));
        assert!(s.contains("\u{1b}[?1003h"));
        assert!(s.contains("\u{1b}[?1006h"));
    }

    #[test]
    fn mouse_button_preferred_over_standard() {
        let s = seq("mouse_button_flag=1,mouse_standard_flag=1");
        assert!(s.contains("\u{1b}[?1002h"));
        assert!(!s.contains("\u{1b}[?1000h"));
    }

    #[test]
    fn out_of_range_cursor_is_ignored() {
        // cursor_y=70000 is out of 0..=65535 → treated as absent, no CUP.
        let s = seq("cursor_x=1,cursor_y=70000");
        assert!(!s.contains('H'));
    }

    #[test]
    fn empty_sided_key_value_pairs_are_ignored() {
        // "=1" and "wrap_flag=" yield a single non-empty piece → not recorded, so
        // wrap stays at its default off form.
        let s = seq("=1,wrap_flag=");
        assert!(s.contains("\u{1b}[?7l"));
    }

    #[test]
    fn window_order_reorders_only_members() {
        // Reorder [1,3] → they appear in that sequence at their original slots;
        // 2 and 4 stay put.
        assert_eq!(window_order(&[1, 2, 3, 4], &[3, 1]), vec![3, 2, 1, 4]);
    }

    #[test]
    fn window_order_leaves_nonmembers_untouched() {
        assert_eq!(window_order(&[5, 6, 7], &[]), vec![5, 6, 7]);
    }

    #[test]
    fn stderr_session_gone_matches_known_phrases() {
        assert!(stderr_indicates_session_gone(
            "tmux: can't find session: main"
        ));
        assert!(stderr_indicates_session_gone(
            "no server running on /tmp/tmux-1000/default"
        ));
        assert!(stderr_indicates_session_gone("LOST SERVER"));
        assert!(stderr_indicates_session_gone("session not found"));
        // Curly apostrophe variant.
        assert!(stderr_indicates_session_gone("can\u{2019}t find session"));
    }

    #[test]
    fn stderr_transient_failures_not_session_gone() {
        assert!(!stderr_indicates_session_gone(
            "ssh: connect to host example.com port 22: Connection refused"
        ));
        assert!(!stderr_indicates_session_gone(
            "ssh: Could not resolve hostname"
        ));
    }
}
