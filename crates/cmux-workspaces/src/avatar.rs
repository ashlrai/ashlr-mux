//! Machine avatar color palette: the pure slot-hash + gradient source
//! resolution keyed to a workspace's owning machine, so every workspace on the
//! same Mac reads with one color in the aggregated multi-Mac list.
//!
//! Port of the pure core of
//! `Packages/iOS/CmuxMobileShellModel/.../MachineAvatarPalette.swift:1-42`
//! (the djb2 slot resolver) and the pure resolution/parsing halves of
//! `Packages/iOS/CmuxMobileShellUI/.../MachineAvatarColors.swift:29-97`
//! (`gradient(index:)` wrapping, the `gradient(customColor:fallbackIndex:...)`
//! source selection, and `Color(hexString:)`).
//!
//! DIVERGENCE (UI shell left out): The Swift UI type owns the concrete swatch
//! table (`static let palettes: [[Color]]`, lines 14-23) and builds
//! `LinearGradient`s. Those SwiftUI values stay in the GUI layer. The port
//! resolves only WHICH source a machine's avatar draws from — a wrapped
//! built-in palette **slot index** or a parsed **custom color** — and the host
//! UI maps the slot to its swatch and constructs the gradient. The palette
//! COUNT (8) is retained as a pure `slot_count` parameter (defaulting to
//! [`MachineAvatarPalette::DEFAULT_SLOT_COUNT`], matching `palettes.count`).
//!
//! DIVERGENCE (color lane): Swift builds `Color(.sRGB, red:green:blue:opacity:)`;
//! the port keeps the parsed color as sRGB f64 components in [`AvatarColor`],
//! per the repo-wide sRGB-f64 color-lane convention.

/// Maps a workspace to a stable avatar color slot keyed to its OWNING MACHINE,
/// so every workspace on the same Mac shares one color in the aggregated
/// multi-Mac list.
///
/// Port of `MachineAvatarPalette` (`MachineAvatarPalette.swift:8-42`). The UI
/// layer maps a returned slot in `0..slot_count` to a concrete gradient; this
/// stays free of any UI dependency so it is unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineAvatarPalette {
    /// Number of distinct color slots in the target palette
    /// (`MachineAvatarPalette.slotCount`, line 14).
    pub slot_count: usize,
}

impl MachineAvatarPalette {
    /// Default number of distinct color slots
    /// (`MachineAvatarPalette.defaultSlotCount`, line 11). The UI passes its
    /// real palette count so the slot is always in range.
    pub const DEFAULT_SLOT_COUNT: usize = 8;

    /// Create a palette slot resolver (`init(slotCount:)`, line 17).
    pub fn new(slot_count: usize) -> Self {
        Self { slot_count }
    }

    /// Stable color slot for a workspace (`slot(machineID:fallbackID:)`,
    /// lines 26-41). Keyed to `machine_id` so same-machine workspaces collide
    /// on one color by design; falls back to `fallback_id` (the workspace id)
    /// when the machine is unknown — `None` or empty — so the avatar still has
    /// a stable color.
    ///
    /// djb2 (seed 5381, `hash = hash * 33 + scalar`) over the source's Unicode
    /// scalar values. Swift's `&*` / `&+` wrap on `Int` (64-bit), mirrored here
    /// with `i64` wrapping arithmetic; the double-modulo
    /// `((hash % count) + count) % count` normalizes the possibly-negative hash
    /// into `0..count`. A Rust `char` is exactly one Unicode scalar, so
    /// `str::chars` matches Swift's `String.unicodeScalars`.
    pub fn slot(&self, machine_id: Option<&str>, fallback_id: &str) -> usize {
        let source = match machine_id {
            Some(id) if !id.is_empty() => id,
            _ => fallback_id,
        };
        let mut hash: i64 = 5381;
        for scalar in source.chars() {
            hash = hash.wrapping_mul(33).wrapping_add(i64::from(scalar as u32));
        }
        let count = (self.slot_count.max(1)) as i64;
        (((hash % count) + count) % count) as usize
    }
}

impl Default for MachineAvatarPalette {
    /// `init(slotCount:)`'s default argument (`MachineAvatarPalette.swift:17`).
    fn default() -> Self {
        Self::new(Self::DEFAULT_SLOT_COUNT)
    }
}

/// A parsed sRGB color from `Color(hexString:)`
/// (`MachineAvatarColors.swift:71-97`). Components are in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvatarColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub opacity: f64,
}

/// The resolved source a machine's avatar gradient draws from
/// (`gradient(customColor:fallbackIndex:machineID:fallbackID:)`,
/// `MachineAvatarColors.swift:46-66`). The UI maps [`Self::PaletteSlot`] to its
/// built-in swatch table and builds the `[color, color·0.72]` gradient for
/// [`Self::CustomColor`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MachineAvatarGradient {
    /// A built-in palette swatch index, already wrapped into `0..slot_count`.
    PaletteSlot(usize),
    /// A user custom solid color parsed from a `#`-hex string.
    CustomColor(AvatarColor),
}

/// `gradient(index:)`'s slot wrapping (`MachineAvatarColors.swift:30`):
/// `((index % count) + count) % count`. `count` is the palette size
/// (`palettes.count`, always positive); guarded with `max(1)` since it is a
/// parameter here (no observable divergence — the real value is 8).
pub fn wrapped_palette_slot(index: i64, slot_count: usize) -> usize {
    let count = (slot_count.max(1)) as i64;
    (((index % count) + count) % count) as usize
}

/// Resolve a machine's avatar gradient source honoring its user override first
/// (`gradient(customColor:fallbackIndex:machineID:fallbackID:)`,
/// `MachineAvatarColors.swift:46-66`):
///
/// 1. A non-empty `custom_color` prefixed `"palette:"` whose remainder parses
///    as an integer → that wrapped palette slot (`lines 52-55`).
/// 2. Else a non-empty `custom_color` parsing as a `#`-hex color → that custom
///    color (`lines 57-62`).
/// 3. Else `fallback_index`, if present → that wrapped palette slot
///    (`line 64`).
/// 4. Else the id-hash slot from [`MachineAvatarPalette::slot`] (`line 65`).
///
/// `"palette:"` parses with Swift `Int(_:)` semantics (optional leading `+`/`-`
/// then base-10 digits, whole string, overflow → reject); a non-integer
/// remainder falls through to the hex-parse step against the FULL string (which
/// lacks `#`, so it too fails) and then to the index/hash fallback — matching
/// Swift's fall-through exactly.
pub fn resolve_gradient_source(
    custom_color: Option<&str>,
    fallback_index: Option<i64>,
    machine_id: Option<&str>,
    fallback_id: &str,
    slot_count: usize,
) -> MachineAvatarGradient {
    if let Some(custom) = custom_color {
        if !custom.is_empty() {
            if let Some(rest) = custom.strip_prefix("palette:") {
                if let Some(n) = parse_swift_int(rest) {
                    return MachineAvatarGradient::PaletteSlot(wrapped_palette_slot(n, slot_count));
                }
            }
            if let Some(color) = parse_hex_color(custom) {
                return MachineAvatarGradient::CustomColor(color);
            }
        }
    }
    if let Some(index) = fallback_index {
        return MachineAvatarGradient::PaletteSlot(wrapped_palette_slot(index, slot_count));
    }
    let slot = MachineAvatarPalette::new(slot_count).slot(machine_id, fallback_id);
    MachineAvatarGradient::PaletteSlot(slot)
}

/// Parse a `#RGB` / `#RRGGBB` / `#RRGGBBAA` hex string into sRGB components
/// (`Color(hexString:)`, `MachineAvatarColors.swift:71-97`). `None` when
/// malformed. This is a SUPERSET of `cmux-workspaces`'
/// [`crate::normalize_hex`] (6-digit-only) — it also accepts the 3- and
/// 8-digit forms.
///
/// Swift trims `whitespacesAndNewlines`, requires a leading `#`, drops it, then
/// parses the remainder with `UInt64(_, radix: 16)` (optional single leading
/// `+`/`-`; `-` valid only for magnitude zero — see [`parse_u64_radix16`]) and
/// switches on the remainder's `Character` count (3/6/8). Any other count, a
/// missing `#`, or a parse failure yields `None`.
pub fn parse_hex_color(hex_string: &str) -> Option<AvatarColor> {
    let trimmed = hex_string.trim();
    let body = trimmed.strip_prefix('#')?;
    let value = parse_u64_radix16(body)?;
    let (red, green, blue, opacity) = match body.chars().count() {
        3 => (
            ((value >> 8) & 0xF) as f64 / 15.0,
            ((value >> 4) & 0xF) as f64 / 15.0,
            (value & 0xF) as f64 / 15.0,
            1.0,
        ),
        6 => (
            ((value >> 16) & 0xFF) as f64 / 255.0,
            ((value >> 8) & 0xFF) as f64 / 255.0,
            (value & 0xFF) as f64 / 255.0,
            1.0,
        ),
        8 => (
            ((value >> 24) & 0xFF) as f64 / 255.0,
            ((value >> 16) & 0xFF) as f64 / 255.0,
            ((value >> 8) & 0xFF) as f64 / 255.0,
            (value & 0xFF) as f64 / 255.0,
        ),
        _ => return None,
    };
    Some(AvatarColor {
        red,
        green,
        blue,
        opacity,
    })
}

/// Whether/what `UInt64(body, radix: 16)` parses in Swift: optional single
/// leading `+`/`-`, then one or more ASCII hex digits; `-` requires magnitude
/// zero (unsigned range); overflow → `None`. Mirrors the same rule
/// `tab_colors::parses_as_swift_u64_radix16` documents, returning the value.
fn parse_u64_radix16(body: &str) -> Option<u64> {
    let bytes = body.as_bytes();
    let (negative, digits) = match bytes.first() {
        Some(b'+') => (false, &bytes[1..]),
        Some(b'-') => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let mut value: u64 = 0;
    for &b in digits {
        let digit = u64::from((b as char).to_digit(16)?);
        value = value.checked_mul(16)?.checked_add(digit)?;
    }
    if negative && value != 0 {
        return None;
    }
    Some(value)
}

/// Whether/what Swift `Int(_ text: String)` parses (base 10): an optional
/// single leading `+`/`-`, then one or more ASCII decimal digits, the whole
/// string, no whitespace/underscores/prefix; overflow → `None`. Rust's
/// `i64::from_str` accepts exactly that grammar (`Int` is 64-bit on the target
/// platforms), so it is a faithful stand-in.
fn parse_swift_int(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ---- MachineAvatarPaletteTests.swift oracles (verbatim) ----

    // sameMachineSharesSlotRegardlessOfWorkspace (lines 5-10).
    #[test]
    fn same_machine_shares_slot_regardless_of_workspace() {
        let palette = MachineAvatarPalette::default();
        let a = palette.slot(Some("mac-studio-abc"), "ws-1");
        let b = palette.slot(Some("mac-studio-abc"), "ws-2");
        assert_eq!(a, b);
    }

    // nilOrEmptyMachineFallsBackToWorkspaceID (lines 12-20).
    #[test]
    fn nil_or_empty_machine_falls_back_to_workspace_id() {
        let palette = MachineAvatarPalette::default();
        let via_nil = palette.slot(None, "ws-42");
        let via_empty = palette.slot(Some(""), "ws-42");
        let direct = palette.slot(Some("ws-42"), "ignored");
        // Unknown machine keys off the workspace id, so all three agree.
        assert_eq!(via_nil, via_empty);
        assert_eq!(via_nil, direct);
        // Hand-computed djb2 pin for "ws-42" @ slot_count 8:
        // 5381 →(w)177692 →(s)5863951 →(-)193510428 →(4)6385844176
        // →(2)210732857858; 210732857858 % 8 == 2.
        assert_eq!(via_nil, 2);
    }

    // slotIsAlwaysInRange (lines 22-28).
    #[test]
    fn slot_is_always_in_range() {
        let palette = MachineAvatarPalette::new(8);
        for id in ["", "a", "mac-mini-1", "100.64.0.7", "AAAA", "ZZZZ", "🙂x"] {
            let slot = palette.slot(Some(id), "fb");
            assert!(slot < 8, "slot for {id:?} out of range: {slot}");
        }
    }

    // distinctMachinesSpreadAcrossSlots (lines 30-36).
    #[test]
    fn distinct_machines_spread_across_slots() {
        // djb2 should not pile a handful of realistic machine ids onto one slot.
        let ids = [
            "cmux-lawrence",
            "cmux-macmini",
            "cmux-studio",
            "macbook-pro",
            "mac-mini-2",
        ];
        let palette = MachineAvatarPalette::default();
        let slots: HashSet<usize> = ids.iter().map(|id| palette.slot(Some(id), "fb")).collect();
        assert!(slots.len() >= 3);
    }

    // ---- Additional parity pins ----

    #[test]
    fn slot_empty_source_uses_seed_only_hash() {
        // Both machine and fallback empty → source "" → loop never runs, so the
        // slot is 5381 % 8 == 5.
        let palette = MachineAvatarPalette::new(8);
        assert_eq!(palette.slot(Some(""), ""), 5);
        assert_eq!(palette.slot(None, ""), 5);
    }

    #[test]
    fn slot_wrapping_stays_in_range_for_long_ids() {
        // A long id overflows the i64 djb2 accumulator (wrapping, possibly
        // negative); the double-modulo must still land in range.
        let palette = MachineAvatarPalette::new(8);
        let long_id = "x".repeat(64);
        let slot = palette.slot(Some(&long_id), "fb");
        assert!(slot < 8);
    }

    #[test]
    fn default_slot_count_is_eight() {
        assert_eq!(MachineAvatarPalette::DEFAULT_SLOT_COUNT, 8);
        assert_eq!(MachineAvatarPalette::default().slot_count, 8);
    }

    // ---- wrapped_palette_slot (gradient(index:)) ----

    #[test]
    fn wrapped_palette_slot_table() {
        assert_eq!(wrapped_palette_slot(3, 8), 3);
        assert_eq!(wrapped_palette_slot(0, 8), 0);
        assert_eq!(wrapped_palette_slot(8, 8), 0);
        assert_eq!(wrapped_palette_slot(11, 8), 3);
        assert_eq!(wrapped_palette_slot(-1, 8), 7);
        assert_eq!(wrapped_palette_slot(-8, 8), 0);
        assert_eq!(wrapped_palette_slot(-9, 8), 7);
        assert_eq!(wrapped_palette_slot(20, 8), 4);
    }

    // ---- resolve_gradient_source ----

    #[test]
    fn resolve_prefers_palette_prefix_override() {
        assert_eq!(
            resolve_gradient_source(Some("palette:3"), None, None, "x", 8),
            MachineAvatarGradient::PaletteSlot(3)
        );
        // Wrapped past the palette size.
        assert_eq!(
            resolve_gradient_source(Some("palette:11"), None, None, "x", 8),
            MachineAvatarGradient::PaletteSlot(3)
        );
        // Negative index wraps.
        assert_eq!(
            resolve_gradient_source(Some("palette:-1"), None, None, "x", 8),
            MachineAvatarGradient::PaletteSlot(7)
        );
        // A "palette:" override beats an available fallback index.
        assert_eq!(
            resolve_gradient_source(Some("palette:2"), Some(5), None, "x", 8),
            MachineAvatarGradient::PaletteSlot(2)
        );
    }

    #[test]
    fn resolve_falls_through_non_integer_palette_prefix() {
        // "palette:abc" is not an Int and (as a whole) not a #-hex, so it falls
        // through to the fallback index, then the id hash.
        assert_eq!(
            resolve_gradient_source(Some("palette:abc"), Some(5), None, "x", 8),
            MachineAvatarGradient::PaletteSlot(5)
        );
        // Empty remainder after "palette:" also falls through.
        assert_eq!(
            resolve_gradient_source(Some("palette:"), Some(1), None, "x", 8),
            MachineAvatarGradient::PaletteSlot(1)
        );
    }

    #[test]
    fn resolve_uses_custom_hex_color() {
        assert_eq!(
            resolve_gradient_source(Some("#FF0000"), Some(3), None, "x", 8),
            MachineAvatarGradient::CustomColor(AvatarColor {
                red: 1.0,
                green: 0.0,
                blue: 0.0,
                opacity: 1.0,
            })
        );
        // Short form + a trailing alpha both route here.
        assert_eq!(
            resolve_gradient_source(Some("#0F0"), None, None, "x", 8),
            MachineAvatarGradient::CustomColor(AvatarColor {
                red: 0.0,
                green: 1.0,
                blue: 0.0,
                opacity: 1.0,
            })
        );
    }

    #[test]
    fn resolve_empty_custom_color_skips_to_fallback_index() {
        assert_eq!(
            resolve_gradient_source(Some(""), Some(5), None, "x", 8),
            MachineAvatarGradient::PaletteSlot(5)
        );
        // Fallback index wraps like gradient(index:).
        assert_eq!(
            resolve_gradient_source(None, Some(20), None, "x", 8),
            MachineAvatarGradient::PaletteSlot(4)
        );
    }

    #[test]
    fn resolve_falls_back_to_id_hash() {
        // No override, no fallback index → the machine id-hash slot.
        assert_eq!(
            resolve_gradient_source(None, None, None, "ws-42", 8),
            MachineAvatarGradient::PaletteSlot(2)
        );
        // A malformed custom color with no fallback index also lands on the hash.
        assert_eq!(
            resolve_gradient_source(Some("not-a-color"), None, None, "ws-42", 8),
            MachineAvatarGradient::PaletteSlot(2)
        );
        // Machine id wins over the fallback id for the hash source.
        let hashed = resolve_gradient_source(None, None, Some("mac-abc"), "ws-42", 8);
        assert_eq!(
            hashed,
            MachineAvatarGradient::PaletteSlot(
                MachineAvatarPalette::new(8).slot(Some("mac-abc"), "ws-42")
            )
        );
    }

    // ---- parse_hex_color (Color(hexString:)) ----

    #[test]
    fn parse_hex_color_three_digit() {
        assert_eq!(
            parse_hex_color("#FFF"),
            Some(AvatarColor {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                opacity: 1.0,
            })
        );
        assert_eq!(
            parse_hex_color("#000"),
            Some(AvatarColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                opacity: 1.0,
            })
        );
        assert_eq!(
            parse_hex_color("#ABC"),
            Some(AvatarColor {
                red: 10.0 / 15.0,
                green: 11.0 / 15.0,
                blue: 12.0 / 15.0,
                opacity: 1.0,
            })
        );
    }

    #[test]
    fn parse_hex_color_six_digit() {
        assert_eq!(
            parse_hex_color("#FF8800"),
            Some(AvatarColor {
                red: 1.0,
                green: 136.0 / 255.0,
                blue: 0.0,
                opacity: 1.0,
            })
        );
        // Lowercase + surrounding whitespace/newlines are trimmed.
        assert_eq!(
            parse_hex_color("  #ffffff \n"),
            Some(AvatarColor {
                red: 1.0,
                green: 1.0,
                blue: 1.0,
                opacity: 1.0,
            })
        );
    }

    #[test]
    fn parse_hex_color_eight_digit_has_alpha() {
        assert_eq!(
            parse_hex_color("#12345678"),
            Some(AvatarColor {
                red: 0x12 as f64 / 255.0,
                green: 0x34 as f64 / 255.0,
                blue: 0x56 as f64 / 255.0,
                opacity: 0x78 as f64 / 255.0,
            })
        );
        // Half-opacity red.
        assert_eq!(
            parse_hex_color("#FF000080"),
            Some(AvatarColor {
                red: 1.0,
                green: 0.0,
                blue: 0.0,
                opacity: 128.0 / 255.0,
            })
        );
    }

    #[test]
    fn parse_hex_color_rejects_malformed() {
        assert_eq!(parse_hex_color("FFFFFF"), None); // no '#'
        assert_eq!(parse_hex_color("#"), None); // empty body
        assert_eq!(parse_hex_color("#FFFF"), None); // 4-digit
        assert_eq!(parse_hex_color("#FFFFF"), None); // 5-digit
        assert_eq!(parse_hex_color("#FFFFFFF"), None); // 7-digit
        assert_eq!(parse_hex_color("#FFFFFFFFF"), None); // 9-digit
        assert_eq!(parse_hex_color("#GGG"), None); // non-hex digits
        assert_eq!(parse_hex_color("#12 456"), None); // interior space
        assert_eq!(parse_hex_color(""), None);
    }

    #[test]
    fn parse_hex_color_mirrors_uint64_sign_edge() {
        // Swift UInt64(_, radix:16) accepts a single leading '+' before hex
        // digits; the count switch counts the sign char, so "+00000" (6 chars)
        // parses to value 0 → opaque black. Faithful mirror of the Foundation
        // parse (an unrealistic input, pinned to lock the semantics).
        assert_eq!(
            parse_hex_color("#+00000"),
            Some(AvatarColor {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                opacity: 1.0,
            })
        );
        // A negative nonzero magnitude is out of range for UInt64 → None.
        assert_eq!(parse_hex_color("#-00001"), None);
    }
}
