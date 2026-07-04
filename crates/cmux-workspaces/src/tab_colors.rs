//! Workspace tab color palette: model, hex normalization, persistence
//! shape, legacy migration, name/hex resolution, cache fingerprint, and
//! dark-mode display brightening.
//!
//! Port of app `Sources/WorkspaceTabColorSettings.swift:14-280`,
//! `Sources/WorkspaceTabColorResolution.swift:3-22`,
//! `Sources/WorkspaceTabColorEntry.swift:8-13`, plus the pure color math from
//! `Packages/macOS/CmuxFoundation/Sources/CmuxFoundation/Color/NSColor+Hex.swift:6-79`
//! and the pure caller-side rules from `Sources/Workspace.swift:4360-4366`
//! (silent-clear on invalid hex), `Sources/TerminalController.swift:4150-4174`
//! (v2 `workspace set_color` resolution), and
//! `Sources/CmuxWorkspaceDefinition.swift:26-47` (cmux.json color decode
//! error message).
//!
//! There is no hashing / automatic color assignment anywhere in the canonical
//! app: colors are only user-assigned, socket-assigned, or cmux.json-defined;
//! a new workspace has no custom color.
//!
//! DIVERGENCE (storage): Swift reads/writes `UserDefaults` directly. The port
//! is I/O-free: reads go through a [`PaletteStoreSnapshot`] value the host
//! adapter populates from its settings store, and writes are returned as a
//! [`PalettePersistOutcome`] the host applies. The host adapter must mirror
//! the Swift cast semantics when building the snapshot (see the field docs).
//!
//! DIVERGENCE (map ordering): Swift palette maps are unordered
//! `Dictionary<String, String>` values, so `first { ... }` scans in
//! `resolvedColorHex` and duplicate-normalized-key overwrites are
//! nondeterministic on macOS. The port keeps maps as ordered
//! `Vec<(String, String)>` with keyed (last-write-wins) upsert semantics and
//! resolves names over the canonical [`palette`] order (built-ins in default
//! order, then customs finder-sorted), making every outcome deterministic.
//!
//! DIVERGENCE (collation): `localizedStandardCompare` is macOS ICU
//! Finder-style collation (case-insensitive, numeric-aware, locale-sensitive).
//! [`finder_like_cmp`] mirrors the case-insensitive + numeric-aware core with
//! a deterministic full-string tiebreak; locale-specific collation of
//! non-ASCII names may differ from macOS. Beyond the in-process parse-cache
//! determinism/change-sensitivity, the custom-name ordering is also
//! CROSS-PROCESS observable: [`palette`]-derived name lists surface in the v2
//! `set_color` error payload's `named_colors` array
//! (`Sources/TerminalController.swift:4167-4170`). On the shipped input domain
//! (fixed built-in order + numeric `Custom N` names) the two orderings are
//! identical; a divergence needs two custom names differing only by
//! non-ASCII/case collation, which does not arise for realistic inputs.

use std::cmp::Ordering;
use std::fmt;

/// Primary palette storage key
/// (`Packages/macOS/CmuxSettings/.../WorkspaceColorsCatalogSection.swift:23-27`,
/// sourced by `WorkspaceTabColorSettings.paletteKey`).
pub const PALETTE_KEY: &str = "workspaceTabColor.colors";
/// Legacy built-in-override storage key (`WorkspaceTabColorSettings.swift:17`).
pub const LEGACY_DEFAULT_OVERRIDES_KEY: &str = "workspaceTabColor.defaultOverrides";
/// Legacy custom-color-list storage key (`WorkspaceTabColorSettings.swift:18`).
pub const LEGACY_CUSTOM_COLORS_KEY: &str = "workspaceTabColor.customColors";

/// A name + hex palette entry (`WorkspaceTabColorEntry`,
/// `Sources/WorkspaceTabColorEntry.swift:8-13`; identity = `name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabColorEntry {
    pub name: String,
    pub hex: String,
}

/// The fixed, ordered 16-entry built-in palette (`originalPRPalette`,
/// `WorkspaceTabColorSettings.swift:20-37`; exposed as `defaultPalette`).
pub const DEFAULT_PALETTE: [(&str, &str); 16] = [
    ("Red", "#C0392B"),
    ("Crimson", "#922B21"),
    ("Orange", "#A04000"),
    ("Amber", "#7D6608"),
    ("Olive", "#4A5C18"),
    ("Green", "#196F3D"),
    ("Teal", "#006B6B"),
    ("Aqua", "#0E6B8C"),
    ("Blue", "#1565C0"),
    ("Navy", "#1A5276"),
    ("Indigo", "#283593"),
    ("Purple", "#6A1B9A"),
    ("Magenta", "#AD1457"),
    ("Rose", "#880E4F"),
    ("Brown", "#7B3F00"),
    ("Charcoal", "#3E4B5E"),
];

/// The built-in palette as owned entries, in canonical order
/// (`WorkspaceTabColorSettings.defaultPalette`, lines 39-41).
pub fn default_palette() -> Vec<TabColorEntry> {
    DEFAULT_PALETTE
        .iter()
        .map(|(name, hex)| TabColorEntry {
            name: (*name).to_owned(),
            hex: (*hex).to_owned(),
        })
        .collect()
}

/// Value snapshot of the three palette storage slots, populated by the host
/// settings adapter. Mirrors the Swift `UserDefaults` read semantics
/// (`WorkspaceTabColorSettings.swift:184-221`):
///
/// - `stored`: the value under [`PALETTE_KEY`] **only if** it is a
///   homogeneous string→string dictionary (Swift
///   `defaults.dictionary(forKey:) as? [String: String]`, line 185). Any
///   missing, non-dictionary, or non-string-valued entry collapses the whole
///   slot to `None`.
/// - `legacy_overrides_present` / `legacy_custom_present`: **key existence**
///   regardless of value type (Swift `defaults.object(forKey:) != nil`,
///   lines 190-191). A wrong-typed legacy value still routes reads through
///   the legacy branch.
/// - `legacy_overrides` / `legacy_custom_colors`: the typed values when the
///   casts succeed (lines 196, 205), else `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteStoreSnapshot {
    pub stored: Option<Vec<(String, String)>>,
    pub legacy_overrides_present: bool,
    pub legacy_overrides: Option<Vec<(String, String)>>,
    pub legacy_custom_present: bool,
    pub legacy_custom_colors: Option<Vec<String>>,
}

/// The write the host must apply after a palette mutation
/// (`persistPaletteMap`, `WorkspaceTabColorSettings.swift:88-97`).
///
/// Either variant ALSO implies removing both legacy keys
/// ([`LEGACY_DEFAULT_OVERRIDES_KEY`], [`LEGACY_CUSTOM_COLORS_KEY`]; lines
/// 95-96). `reset` (lines 122-126) is simply: remove [`PALETTE_KEY`] and both
/// legacy keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PalettePersistOutcome {
    /// Store this normalized map under [`PALETTE_KEY`].
    SetMap(Vec<(String, String)>),
    /// The normalized map equals the built-in default palette exactly, so the
    /// primary key is removed (canonical-default elision, line 90-91).
    RemoveKey,
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Trims `CharacterSet.whitespacesAndNewlines` from both ends.
///
/// Swift's set (Zs + U+0009 tab + newlines U+000A-U+000D, U+0085, U+2028,
/// U+2029) is exactly the Unicode `White_Space` property set used by Rust's
/// `str::trim`, so plain `trim()` is byte-for-byte equivalent.
fn trim_whitespace_and_newlines(raw: &str) -> &str {
    raw.trim()
}

/// `WorkspaceTabColorSettings.normalizedHex` (lines 128-135): trim, strip AT
/// MOST ONE leading `#`, require a 6-character body that parses as
/// `UInt64(_, radix: 16)`, and return `"#" + body.uppercased()`.
///
/// Swift's `FixedWidthInteger.init?(_:radix:)` accepts an optional single
/// leading `+`/`-` sign before ASCII hex digits (no `0x`, no underscores, no
/// non-ASCII digits); a `-` sign is only in-range for `UInt64` when the
/// magnitude is zero. Mirrored exactly, so `"+ABCDE"` → `"#+ABCDE"` and
/// `"-00000"` → `"#-00000"`, while `"-00001"` → `None`.
///
/// Swift counts the body in `Character` grapheme clusters, Rust in `char`s;
/// the counts only differ for combining sequences, which always fail the
/// ASCII-only integer parse in both languages, so the observable result is
/// identical.
pub fn normalize_hex(raw: &str) -> Option<String> {
    let trimmed = trim_whitespace_and_newlines(raw);
    if trimmed.is_empty() {
        return None;
    }
    let body = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if body.chars().count() != 6 {
        return None;
    }
    if !parses_as_swift_u64_radix16(body) {
        return None;
    }
    Some(format!("#{}", body.to_ascii_uppercase()))
}

/// Whether `UInt64(body, radix: 16)` succeeds in Swift: optional single
/// leading `+`/`-`, then one or more ASCII hex digits; `-` requires magnitude
/// zero (unsigned range).
fn parses_as_swift_u64_radix16(body: &str) -> bool {
    let bytes = body.as_bytes();
    let (negative, digits) = match bytes.first() {
        Some(b'+') => (false, &bytes[1..]),
        Some(b'-') => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    if negative && digits.iter().any(|b| *b != b'0') {
        return false;
    }
    true
}

/// `normalizedColorName` (lines 237-240): trim; empty → `None`; else the
/// trimmed name (interior whitespace and case preserved).
pub fn normalized_color_name(raw: &str) -> Option<String> {
    let trimmed = trim_whitespace_and_newlines(raw);
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// `Workspace.setCustomColor` (`Sources/Workspace.swift:4360-4366`):
/// `Some(hex)` normalizes the hex — an INVALID hex silently clears the custom
/// color to `None`; `None` clears.
pub fn normalized_custom_color(hex: Option<&str>) -> Option<String> {
    normalize_hex(hex?)
}

/// `normalizedPaletteMap` (lines 223-231): drop entries whose name trims to
/// empty or whose hex fails [`normalize_hex`]; keep trimmed name + normalized
/// hex. Keyed last-write-wins upsert preserving first-occurrence order
/// (deterministic stand-in for Swift's unordered dictionary rebuild).
fn normalized_palette_map(raw: &[(String, String)]) -> Vec<(String, String)> {
    let mut normalized: Vec<(String, String)> = Vec::new();
    for (raw_name, raw_hex) in raw {
        let (Some(name), Some(hex)) = (normalized_color_name(raw_name), normalize_hex(raw_hex))
        else {
            continue;
        };
        upsert(&mut normalized, name, hex);
    }
    normalized
}

/// Keyed insert-or-replace preserving the existing key's position.
fn upsert(map: &mut Vec<(String, String)>, name: String, hex: String) {
    match map.iter_mut().find(|(existing, _)| *existing == name) {
        Some((_, value)) => *value = hex,
        None => map.push((name, hex)),
    }
}

fn map_get<'a>(map: &'a [(String, String)], name: &str) -> Option<&'a str> {
    map.iter()
        .find(|(key, _)| key == name)
        .map(|(_, hex)| hex.as_str())
}

/// Order-insensitive map equality (Swift compares `Dictionary` values).
fn maps_equal(a: &[(String, String)], b: &[(String, String)]) -> bool {
    a.len() == b.len() && a.iter().all(|(name, hex)| map_get(b, name) == Some(hex))
}

fn default_palette_map() -> Vec<(String, String)> {
    DEFAULT_PALETTE
        .iter()
        .map(|(name, hex)| ((*name).to_owned(), (*hex).to_owned()))
        .collect()
}

// ---------------------------------------------------------------------------
// Reads: stored / legacy / effective maps
// ---------------------------------------------------------------------------

/// `storedPaletteMap` (lines 184-187): the primary slot, re-normalized. A
/// present-but-all-invalid dictionary yields `Some(empty)` — which makes the
/// effective palette empty, NOT the default.
fn stored_palette_map(snapshot: &PaletteStoreSnapshot) -> Option<Vec<(String, String)>> {
    snapshot.stored.as_deref().map(normalized_palette_map)
}

/// `legacyPaletteMap` (lines 189-221): materialize the legacy two-key format
/// into a full palette map. `None` unless EITHER legacy key exists (by
/// presence, not by typed value).
///
/// Overrides apply only to built-in palette names (exact match) with valid
/// hex. Custom colors are applied in array order: invalid or
/// duplicate-within-the-array (by normalized hex) entries are skipped —
/// duplicates against override/built-in hexes are NOT skipped; each accepted
/// color `i` (1-based over accepted entries only) is named
/// `nextCustomColorName(existingNames: current keys, startingAt: i)`.
fn legacy_palette_map(snapshot: &PaletteStoreSnapshot) -> Option<Vec<(String, String)>> {
    if !snapshot.legacy_overrides_present && !snapshot.legacy_custom_present {
        return None;
    }

    let mut palette = default_palette_map();

    if let Some(raw_overrides) = &snapshot.legacy_overrides {
        for (name, hex) in raw_overrides {
            let is_built_in = DEFAULT_PALETTE.iter().any(|(built_in, _)| built_in == name);
            let Some(normalized) = normalize_hex(hex) else {
                continue;
            };
            if is_built_in {
                upsert(&mut palette, name.clone(), normalized);
            }
        }
    }

    if let Some(raw_custom_colors) = &snapshot.legacy_custom_colors {
        let mut index: u64 = 1;
        let mut seen_custom_hexes: Vec<String> = Vec::new();
        for raw_hex in raw_custom_colors {
            let Some(normalized) = normalize_hex(raw_hex) else {
                continue;
            };
            if seen_custom_hexes.contains(&normalized) {
                continue;
            }
            seen_custom_hexes.push(normalized.clone());
            let existing: Vec<String> = palette.iter().map(|(name, _)| name.clone()).collect();
            let name = next_custom_color_name(&existing, index);
            palette.push((name, normalized));
            index += 1;
        }
    }

    Some(palette)
}

/// `effectivePaletteMap` / `editablePaletteMap` / `resolvedPaletteMap`
/// (lines 164-182, 106-108): stored ?? legacy ?? default.
pub fn effective_palette_map(snapshot: &PaletteStoreSnapshot) -> Vec<(String, String)> {
    if let Some(stored) = stored_palette_map(snapshot) {
        return stored;
    }
    if let Some(legacy) = legacy_palette_map(snapshot) {
        return legacy;
    }
    default_palette_map()
}

/// `backupPaletteMap` (lines 99-104): stored ?? legacy (materialized) ?? None.
pub fn backup_palette_map(snapshot: &PaletteStoreSnapshot) -> Option<Vec<(String, String)>> {
    stored_palette_map(snapshot).or_else(|| legacy_palette_map(snapshot))
}

/// `palette()` (lines 43-57): built-in entries in default-palette order
/// (keeping only names still present in the effective map, taking the MAP's
/// hex — deleted built-ins disappear), followed by all non-built-in entries
/// sorted by name via [`finder_like_cmp`].
pub fn palette(snapshot: &PaletteStoreSnapshot) -> Vec<TabColorEntry> {
    let palette_map = effective_palette_map(snapshot);
    let mut entries: Vec<TabColorEntry> = DEFAULT_PALETTE
        .iter()
        .filter_map(|(name, _)| {
            map_get(&palette_map, name).map(|hex| TabColorEntry {
                name: (*name).to_owned(),
                hex: hex.to_owned(),
            })
        })
        .collect();
    let mut custom: Vec<&(String, String)> = palette_map
        .iter()
        .filter(|(name, _)| !DEFAULT_PALETTE.iter().any(|(built_in, _)| built_in == name))
        .collect();
    custom.sort_by(|(a, _), (b, _)| finder_like_cmp(a, b));
    entries.extend(custom.into_iter().map(|(name, hex)| TabColorEntry {
        name: name.clone(),
        hex: hex.clone(),
    }));
    entries
}

/// `customPaletteEntries` (lines 59-62): [`palette`] minus built-in NAMES.
pub fn custom_palette_entries(snapshot: &PaletteStoreSnapshot) -> Vec<TabColorEntry> {
    palette(snapshot)
        .into_iter()
        .filter(|entry| {
            !DEFAULT_PALETTE
                .iter()
                .any(|(built_in, _)| *built_in == entry.name)
        })
        .collect()
}

/// `defaultColorHex(named:)` (lines 64-66): exact-match (case-sensitive)
/// lookup in the built-in palette.
pub fn default_color_hex(name: &str) -> Option<&'static str> {
    DEFAULT_PALETTE
        .iter()
        .find(|(built_in, _)| *built_in == name)
        .map(|(_, hex)| *hex)
}

/// `currentColorHex(named:)` (lines 68-70): exact-match (case-SENSITIVE) key
/// lookup in the effective map. NOTE the asymmetry with the case-INSENSITIVE
/// resolvers [`resolved_color_hex`] / [`resolve_set_color_input`];
/// `TabManager.applyWorkspacePaletteColor` (`Sources/TabManager.swift:
/// 1731-1734`) uses this and silently no-ops when the name misses.
pub fn current_color_hex(name: &str, snapshot: &PaletteStoreSnapshot) -> Option<String> {
    map_get(&effective_palette_map(snapshot), name).map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Mutations (each returns the persist write for the host to apply)
// ---------------------------------------------------------------------------

/// `persistPaletteMap` (lines 88-97): normalize; if the normalized map equals
/// the default palette map exactly → [`PalettePersistOutcome::RemoveKey`];
/// else `SetMap(normalized)`. Both variants also remove the legacy keys.
pub fn persist_palette_map(raw: &[(String, String)]) -> PalettePersistOutcome {
    let normalized = normalized_palette_map(raw);
    if maps_equal(&normalized, &default_palette_map()) {
        PalettePersistOutcome::RemoveKey
    } else {
        PalettePersistOutcome::SetMap(normalized)
    }
}

/// `setColor(named:hex:)` (lines 72-79): both name and hex must normalize
/// (else no-op → `None`); upsert into the editable (= effective) map and
/// persist with default-elision.
pub fn set_color(
    name: &str,
    hex: &str,
    snapshot: &PaletteStoreSnapshot,
) -> Option<PalettePersistOutcome> {
    let name = normalized_color_name(name)?;
    let hex = normalize_hex(hex)?;
    let mut palette = effective_palette_map(snapshot);
    upsert(&mut palette, name, hex);
    Some(persist_palette_map(&palette))
}

/// `removeColor(named:)` (lines 81-86): name must normalize (else no-op →
/// `None`); remove the trimmed name and persist — Swift persists even when
/// the key was absent.
pub fn remove_color(name: &str, snapshot: &PaletteStoreSnapshot) -> Option<PalettePersistOutcome> {
    let name = normalized_color_name(name)?;
    let mut palette = effective_palette_map(snapshot);
    palette.retain(|(existing, _)| *existing != name);
    Some(persist_palette_map(&palette))
}

/// `addCustomColor` (lines 110-120): normalize (fail → `(None, None)`); if
/// ANY entry already has that exact normalized hex VALUE, return the hex
/// without persisting (dedupe by value); else insert under the next free
/// `"Custom N"` name and persist. Returns `(normalized_hex, persist)`.
pub fn add_custom_color(
    hex: &str,
    snapshot: &PaletteStoreSnapshot,
) -> (Option<String>, Option<PalettePersistOutcome>) {
    let Some(normalized) = normalize_hex(hex) else {
        return (None, None);
    };
    let mut palette = effective_palette_map(snapshot);
    if palette.iter().any(|(_, value)| *value == normalized) {
        return (Some(normalized), None);
    }
    let existing: Vec<String> = palette.iter().map(|(name, _)| name.clone()).collect();
    let name = next_custom_color_name(&existing, 1);
    palette.push((name, normalized.clone()));
    (Some(normalized), Some(persist_palette_map(&palette)))
}

/// `nextCustomColorName` (lines 242-254): candidates `"Custom 1"`,
/// `"Custom 2"`, ... starting at `max(1, starting_at)`; skip while any
/// existing name matches the candidate CASE-INSENSITIVELY.
fn next_custom_color_name(existing_names: &[String], starting_at: u64) -> String {
    let mut index = starting_at.max(1);
    loop {
        let candidate = format!("Custom {index}");
        if !existing_names
            .iter()
            .any(|name| eq_case_insensitive(name, &candidate))
        {
            return candidate;
        }
        index += 1;
    }
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Swift `caseInsensitiveCompare == .orderedSame`: Unicode case-insensitive
/// equality. Full-string `char`-wise lowercase fold; the canonical-equivalence
/// half of the Foundation comparison is a sanctioned negligible divergence
/// (all shipped palette names are ASCII).
fn eq_case_insensitive(a: &str, b: &str) -> bool {
    let mut a_chars = a.chars().flat_map(char::to_lowercase);
    let mut b_chars = b.chars().flat_map(char::to_lowercase);
    loop {
        match (a_chars.next(), b_chars.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => continue,
            _ => return false,
        }
    }
}

/// `resolvedColorHex` (`Sources/WorkspaceTabColorResolution.swift:4-14`, used
/// by the cmux.json workspace color decode): HEX FIRST — a valid hex wins
/// even over a palette entry literally named e.g. `"C0FFEE"`; else trim;
/// empty → `None`; else the first case-insensitive NAME match.
///
/// DIVERGENCE: Swift scans an unordered `Dictionary` with `.first {}` —
/// nondeterministic when two entries' names differ only by case. The port
/// scans the canonical ordered [`palette`] (built-ins in default order, then
/// finder-sorted customs), matching the deterministic order used by the v2
/// `set_color` resolver.
pub fn resolved_color_hex(raw: &str, snapshot: &PaletteStoreSnapshot) -> Option<String> {
    if let Some(normalized) = normalize_hex(raw) {
        return Some(normalized);
    }
    let trimmed = trim_whitespace_and_newlines(raw);
    if trimmed.is_empty() {
        return None;
    }
    palette(snapshot)
        .into_iter()
        .find(|entry| eq_case_insensitive(&entry.name, trimmed))
        .map(|entry| entry.hex)
}

/// v2 `workspace set_color` error message
/// (`Sources/TerminalController.swift:4153`).
pub const MISSING_COLOR_MESSAGE: &str = "Missing or invalid color";
/// v2 `workspace set_color` error message
/// (`Sources/TerminalController.swift:4168`).
pub const INVALID_COLOR_MESSAGE: &str =
    "Invalid color. Use a hex value (#RRGGBB) or a named color.";

/// Failure of [`resolve_set_color_input`]; both variants map to the v2 error
/// code `invalid_params` server-side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetColorError {
    /// Input trimmed to empty (`TerminalController.swift:4151-4154`).
    Missing,
    /// Neither a palette name nor a valid hex
    /// (`TerminalController.swift:4166-4171`); carries the palette names in
    /// palette order for the error payload's `named_colors`.
    Invalid { named_colors: Vec<String> },
}

impl SetColorError {
    /// The v2 error `message` string.
    pub fn message(&self) -> &'static str {
        match self {
            SetColorError::Missing => MISSING_COLOR_MESSAGE,
            SetColorError::Invalid { .. } => INVALID_COLOR_MESSAGE,
        }
    }
}

impl fmt::Display for SetColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// v2 socket `workspace set_color` resolution
/// (`Sources/TerminalController.swift:4150-4172`): trim (empty →
/// [`SetColorError::Missing`]); NAME FIRST over the ordered palette
/// (case-insensitive — so an entry named `"C0FFEE"` beats hex parsing here,
/// the OPPOSITE precedence of [`resolved_color_hex`]); else [`normalize_hex`];
/// else [`SetColorError::Invalid`] with the palette names in order. The v2
/// response envelope stays server-side.
pub fn resolve_set_color_input(
    input: &str,
    palette: &[TabColorEntry],
) -> Result<String, SetColorError> {
    let trimmed = trim_whitespace_and_newlines(input);
    if trimmed.is_empty() {
        return Err(SetColorError::Missing);
    }
    if let Some(entry) = palette
        .iter()
        .find(|entry| eq_case_insensitive(&entry.name, trimmed))
    {
        return Ok(entry.hex.clone());
    }
    if let Some(normalized) = normalize_hex(trimmed) {
        return Ok(normalized);
    }
    Err(SetColorError::Invalid {
        named_colors: palette.iter().map(|entry| entry.name.clone()).collect(),
    })
}

/// The cmux.json workspace color decode error text
/// (`Sources/CmuxWorkspaceDefinition.swift:36-40`), byte-for-byte, built from
/// the RAW string as typed in the file. Exposed so `cmux-config` (or its
/// caller) can run [`resolved_color_hex`] as a post-decode validation pass
/// without string drift.
pub fn invalid_color_message(raw: &str) -> String {
    format!("Invalid color \"{raw}\". Expected 6-digit hex format (#RRGGBB) or a workspace color name")
}

/// `paletteCacheFingerprint`
/// (`Sources/WorkspaceTabColorResolution.swift:16-21`): effective-map entries
/// sorted by [`finder_like_cmp`] on the NAME, rendered `"<name>=<hex>"`
/// joined with `"\n"` (no trailing newline; empty map → `""`). Used only as
/// an in-process config parse-cache key (`Sources/CmuxConfig.swift:3073`) —
/// only determinism and change-sensitivity are observable.
pub fn palette_cache_fingerprint(snapshot: &PaletteStoreSnapshot) -> String {
    let mut entries = effective_palette_map(snapshot);
    entries.sort_by(|(a, _), (b, _)| finder_like_cmp(a, b));
    entries
        .iter()
        .map(|(name, hex)| format!("{name}={hex}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Finder-like collation
// ---------------------------------------------------------------------------

/// Deterministic stand-in for `localizedStandardCompare` (Finder-like sort):
/// case-insensitive, ASCII-digit-run-as-number chunked compare (`"Custom 2"`
/// < `"Custom 10"`), tie-broken by full byte compare. See the module-level
/// collation DIVERGENCE note.
pub fn finder_like_cmp(a: &str, b: &str) -> Ordering {
    let chunked = chunked_cmp(a, b);
    if chunked != Ordering::Equal {
        return chunked;
    }
    a.as_bytes().cmp(b.as_bytes())
}

fn chunked_cmp(a: &str, b: &str) -> Ordering {
    let mut a_rest = a;
    let mut b_rest = b;
    loop {
        match (a_rest.is_empty(), b_rest.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        let a_digit = a_rest.as_bytes()[0].is_ascii_digit();
        let b_digit = b_rest.as_bytes()[0].is_ascii_digit();
        match (a_digit, b_digit) {
            (true, true) => {
                let (a_run, a_next) = split_digit_run(a_rest);
                let (b_run, b_next) = split_digit_run(b_rest);
                let numeric = cmp_digit_runs(a_run, b_run);
                if numeric != Ordering::Equal {
                    return numeric;
                }
                a_rest = a_next;
                b_rest = b_next;
            }
            (true, false) | (false, true) => {
                // Digit-run chunk vs text chunk: compare the leading chars
                // case-folded, like any other character mismatch.
                let folded = cmp_chars_folded(a_rest, b_rest);
                if folded != Ordering::Equal {
                    return folded;
                }
                unreachable!("digit vs non-digit leading chars cannot fold equal");
            }
            (false, false) => {
                let folded = cmp_chars_folded(a_rest, b_rest);
                if folded != Ordering::Equal {
                    return folded;
                }
                let mut a_chars = a_rest.chars();
                let mut b_chars = b_rest.chars();
                a_chars.next();
                b_chars.next();
                a_rest = a_chars.as_str();
                b_rest = b_chars.as_str();
            }
        }
    }
}

/// Compares only the FIRST char of each side, case-folded.
fn cmp_chars_folded(a: &str, b: &str) -> Ordering {
    let a_folded: Vec<char> = a.chars().next().into_iter().flat_map(char::to_lowercase).collect();
    let b_folded: Vec<char> = b.chars().next().into_iter().flat_map(char::to_lowercase).collect();
    a_folded.cmp(&b_folded)
}

fn split_digit_run(s: &str) -> (&str, &str) {
    let end = s
        .as_bytes()
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(s.len());
    s.split_at(end)
}

/// Numeric compare of two ASCII digit runs of arbitrary length: strip leading
/// zeros, compare significant length, then lexicographically.
fn cmp_digit_runs(a: &str, b: &str) -> Ordering {
    let a_sig = a.trim_start_matches('0');
    let b_sig = b.trim_start_matches('0');
    a_sig
        .len()
        .cmp(&b_sig.len())
        .then_with(|| a_sig.cmp(b_sig))
}

// ---------------------------------------------------------------------------
// Display math (pure-sRGB core of the GUI brightening; consumed by the web
// frontend for dark mode)
// ---------------------------------------------------------------------------

/// `NSColor.luminance` (`NSColor+Hex.swift:34-43`): `0.299r + 0.587g +
/// 0.114b` over `[0, 1]` components.
pub fn luminance(rgb: [u8; 3]) -> f64 {
    let [r, g, b] = rgb.map(|byte| f64::from(byte) / 255.0);
    0.299 * r + 0.587 * g + 0.114 * b
}

/// `brightenedForDarkAppearance` (`WorkspaceTabColorSettings.swift:256-279`)
/// over sRGB f64 HSB:
/// `boostedBrightness = min(1, max(b, 0.62) + (1 - b) * 0.28)` — the `(1 - b)`
/// term uses the ORIGINAL brightness, not the `max()`ed value;
/// `boostedSaturation = s <= 0.08 ? s : min(1, s + (1 - s) * 0.12)`
/// (gray preservation). Final bytes truncate toward zero like Swift `Int()`
/// (`NSColor+Hex.swift:71-73`; `0.999 * 255 = 254.745` → `254`).
///
/// DIVERGENCE: Swift round-trips through `NSColor`, whose colorspace
/// conversions can introduce ±1-byte deltas on macOS; this port keeps the
/// whole pipeline in sRGB f64 and pins self-consistent goldens plus the Swift
/// behavioral assertions (luminance strictly increases, grayscale stays
/// neutral).
pub fn brightened_for_dark_appearance_rgb(rgb: [u8; 3]) -> [u8; 3] {
    let (hue, saturation, brightness) = rgb_to_hsb(rgb);
    let boosted_brightness = (brightness.max(0.62) + (1.0 - brightness) * 0.28).min(1.0);
    let boosted_saturation = if saturation <= 0.08 {
        saturation
    } else {
        (saturation + (1.0 - saturation) * 0.12).min(1.0)
    };
    hsb_to_rgb(hue, boosted_saturation, boosted_brightness)
}

/// RGB bytes → HSB in `[0, 1)` hue turns, matching `NSColor.getHue`:
/// brightness = max component; saturation = `(max - min) / max` (0 when max
/// is 0); hue by the standard sextant formula.
fn rgb_to_hsb(rgb: [u8; 3]) -> (f64, f64, f64) {
    let [r, g, b] = rgb.map(|byte| f64::from(byte) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let brightness = max;
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        (((g - b) / delta).rem_euclid(6.0)) / 6.0
    } else if max == g {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };
    (hue, saturation, brightness)
}

/// HSB (hue in `[0, 1)` turns) → RGB bytes; bytes truncate toward zero and
/// clamp to `[0, 255]` like Swift `Int(component * 255)`
/// (`NSColor+Hex.swift:71-73`).
fn hsb_to_rgb(hue: f64, saturation: f64, brightness: f64) -> [u8; 3] {
    let (r, g, b) = if saturation <= 0.0 {
        (brightness, brightness, brightness)
    } else {
        let h6 = (hue.rem_euclid(1.0)) * 6.0;
        let sector = h6.floor();
        let f = h6 - sector;
        let p = brightness * (1.0 - saturation);
        let q = brightness * (1.0 - saturation * f);
        let t = brightness * (1.0 - saturation * (1.0 - f));
        match sector as i64 {
            0 => (brightness, t, p),
            1 => (q, brightness, p),
            2 => (p, brightness, t),
            3 => (p, q, brightness),
            4 => (t, p, brightness),
            _ => (brightness, p, q),
        }
    };
    [r, g, b].map(component_to_byte)
}

/// Swift `Int(component * 255)` clamped to `[0, 255]`: truncation toward
/// zero, NOT rounding.
fn component_to_byte(component: f64) -> u8 {
    ((component * 255.0) as i64).clamp(0, 255) as u8
}

fn parse_hex_rgb(normalized: &str) -> Option<[u8; 3]> {
    let body = normalized.strip_prefix('#')?;
    if body.len() != 6 || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(body, 16).ok()?;
    Some([
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    ])
}

/// `displayNSColor` + `hexString` (`WorkspaceTabColorSettings.swift:148-162`,
/// `NSColor+Hex.swift:64-79`) as a pure hex→hex transform: normalize (invalid
/// → `None`; note the sign-accepting [`normalize_hex`] outputs like
/// `"#+ABCDE"` are not parseable RGB and also yield `None`, matching Swift
/// where `NSColor(hex:)`'s `Scanner` stops at the sign); if `force_bright ||
/// dark` → brightened bytes rendered `#RRGGBB`; else the normalized hex
/// unchanged (Swift returns the base color, whose sRGB byte round-trip is the
/// identity).
pub fn display_color_hex(hex: &str, dark: bool, force_bright: bool) -> Option<String> {
    let normalized = normalize_hex(hex)?;
    let rgb = parse_hex_rgb(&normalized)?;
    if force_bright || dark {
        let [r, g, b] = brightened_for_dark_appearance_rgb(rgb);
        Some(format!("#{r:02X}{g:02X}{b:02X}"))
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snapshot() -> PaletteStoreSnapshot {
        PaletteStoreSnapshot::default()
    }

    fn snapshot_with_stored(entries: &[(&str, &str)]) -> PaletteStoreSnapshot {
        PaletteStoreSnapshot {
            stored: Some(
                entries
                    .iter()
                    .map(|(name, hex)| ((*name).to_owned(), (*hex).to_owned()))
                    .collect(),
            ),
            ..PaletteStoreSnapshot::default()
        }
    }

    fn stored_from_outcome(outcome: PalettePersistOutcome) -> PaletteStoreSnapshot {
        match outcome {
            PalettePersistOutcome::SetMap(map) => PaletteStoreSnapshot {
                stored: Some(map),
                ..PaletteStoreSnapshot::default()
            },
            PalettePersistOutcome::RemoveKey => PaletteStoreSnapshot::default(),
        }
    }

    // §3 — testNormalizedHexAcceptsAndNormalizesValidInput + edge pins.
    #[test]
    fn normalize_hex_oracle_table() {
        let cases: &[(&str, Option<&str>)] = &[
            // WorkspaceUnitTests.swift:3655-3660
            ("#abc123", Some("#ABC123")),
            ("  aBcDeF ", Some("#ABCDEF")),
            ("#1234", None),
            ("#GG1234", None),
            // Edge pins.
            ("", None),
            ("   ", None),
            ("##ABCDEF", None),   // strip at most ONE '#': body "#ABCDEF" is 7 chars
            ("#ABCDEFF", None),   // 7-char body
            ("#ABCDE", None),     // 5-char body
            ("\u{00A0}ABCDEF\n", Some("#ABCDEF")), // NBSP + newline trimmed
            ("\u{3000}#abc123\u{2028}", Some("#ABC123")), // ideographic space + line sep
            ("FFFFFF", Some("#FFFFFF")),
            ("#000000", Some("#000000")),
            // Swift UInt64(_, radix:16) sign rule.
            ("+ABCDE", Some("#+ABCDE")),
            ("-00000", Some("#-00000")),
            ("-00001", None), // negative nonzero out of range for UInt64
            ("++ABCD", None), // only one sign consumed
            ("A BCDE", None), // interior whitespace is not trimmed
            ("ABC12\u{0301}", None), // combining char fails the ASCII parse
            ("ＡＢＣ１２３", None),  // fullwidth digits are not ASCII hex
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_hex(input).as_deref(),
                *expected,
                "normalize_hex({input:?})"
            );
        }
    }

    #[test]
    fn normalized_color_name_trims_and_rejects_empty() {
        assert_eq!(normalized_color_name("  Neon Mint "), Some("Neon Mint".to_owned()));
        assert_eq!(normalized_color_name("\u{00A0}\n"), None);
        assert_eq!(normalized_color_name(""), None);
        // Interior whitespace and case preserved.
        assert_eq!(normalized_color_name(" a  B "), Some("a  B".to_owned()));
    }

    // §2 — testBuiltInPaletteMatchesOriginalPRPalette.
    #[test]
    fn default_palette_matches_original_pr_palette() {
        let palette = default_palette();
        assert_eq!(palette.len(), 16);
        assert_eq!(palette[0].name, "Red");
        assert_eq!(palette[0].hex, "#C0392B");
        assert_eq!(palette[15].name, "Charcoal");
        assert_eq!(palette[15].hex, "#3E4B5E");
        assert!(!palette.iter().any(|entry| entry.name == "Gold"));
        // Full byte/order pin.
        let expected = [
            ("Red", "#C0392B"),
            ("Crimson", "#922B21"),
            ("Orange", "#A04000"),
            ("Amber", "#7D6608"),
            ("Olive", "#4A5C18"),
            ("Green", "#196F3D"),
            ("Teal", "#006B6B"),
            ("Aqua", "#0E6B8C"),
            ("Blue", "#1565C0"),
            ("Navy", "#1A5276"),
            ("Indigo", "#283593"),
            ("Purple", "#6A1B9A"),
            ("Magenta", "#AD1457"),
            ("Rose", "#880E4F"),
            ("Brown", "#7B3F00"),
            ("Charcoal", "#3E4B5E"),
        ];
        for (entry, (name, hex)) in palette.iter().zip(expected) {
            assert_eq!((entry.name.as_str(), entry.hex.as_str()), (name, hex));
        }
    }

    // testPaletteFallsBackToBuiltInDefaultsWhenUnset.
    #[test]
    fn palette_falls_back_to_built_in_defaults_when_unset() {
        assert_eq!(palette(&empty_snapshot()), default_palette());
    }

    // testSetColorRoundTripFallsBackWhenResetToBase.
    #[test]
    fn set_color_round_trip_falls_back_when_reset_to_base() {
        let snap = empty_snapshot();
        assert_eq!(current_color_hex("Red", &snap).as_deref(), Some("#C0392B"));

        let outcome = set_color("Red", "#00aa33", &snap).expect("valid set");
        assert!(matches!(outcome, PalettePersistOutcome::SetMap(_)));
        let snap2 = stored_from_outcome(outcome);
        assert_eq!(current_color_hex("Red", &snap2).as_deref(), Some("#00AA33"));

        // Setting back to the base hex elides the key entirely.
        let outcome = set_color("Red", "#C0392B", &snap2).expect("valid set");
        assert_eq!(outcome, PalettePersistOutcome::RemoveKey);
        let snap3 = stored_from_outcome(outcome);
        assert_eq!(current_color_hex("Red", &snap3).as_deref(), Some("#C0392B"));
    }

    #[test]
    fn set_color_no_ops_on_invalid_name_or_hex() {
        let snap = empty_snapshot();
        assert_eq!(set_color("  ", "#123456", &snap), None);
        assert_eq!(set_color("Red", "nope", &snap), None);
    }

    // testAddCustomColorCreatesNamedEntriesAndDeduplicatesByHex (§5 oracles).
    #[test]
    fn add_custom_color_names_and_dedupes_by_hex() {
        let snap = empty_snapshot();
        let (hex, outcome) = add_custom_color(" #00aa33 ", &snap);
        assert_eq!(hex.as_deref(), Some("#00AA33"));
        let snap = stored_from_outcome(outcome.expect("persists"));

        let (hex, outcome) = add_custom_color("#112233", &snap);
        assert_eq!(hex.as_deref(), Some("#112233"));
        let snap = stored_from_outcome(outcome.expect("persists"));

        // Re-adding an existing hex value returns it WITHOUT persisting.
        let (hex, outcome) = add_custom_color("#00AA33", &snap);
        assert_eq!(hex.as_deref(), Some("#00AA33"));
        assert_eq!(outcome, None);

        // Invalid hex is rejected with no write.
        assert_eq!(add_custom_color("nope", &snap), (None, None));

        let custom = custom_palette_entries(&snap);
        assert_eq!(
            custom.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["Custom 1", "Custom 2"]
        );
        assert_eq!(
            custom.iter().map(|e| e.hex.as_str()).collect::<Vec<_>>(),
            ["#00AA33", "#112233"]
        );
    }

    #[test]
    fn add_custom_color_skips_case_insensitive_name_collision() {
        // A user-named "custom 1" entry blocks the "Custom 1" candidate.
        let mut map = default_palette_map();
        map.push(("custom 1".to_owned(), "#111111".to_owned()));
        let snap = snapshot_with_stored(
            &map.iter()
                .map(|(n, h)| (n.as_str(), h.as_str()))
                .collect::<Vec<_>>(),
        );
        let (hex, outcome) = add_custom_color("#222222", &snap);
        assert_eq!(hex.as_deref(), Some("#222222"));
        let snap = stored_from_outcome(outcome.expect("persists"));
        assert!(current_color_hex("Custom 2", &snap).is_some());
        assert!(current_color_hex("Custom 1", &snap).is_none());
    }

    // testPaletteDictionaryCanRemoveBuiltInEntriesAndAddNamedOnes.
    #[test]
    fn palette_can_remove_built_ins_and_add_named_entries() {
        let mut map = default_palette_map();
        map.retain(|(name, _)| name != "Red");
        map.push(("Neon Mint".to_owned(), "#00F5D4".to_owned()));
        let outcome = persist_palette_map(&map);
        let snap = stored_from_outcome(outcome);

        let resolved = palette(&snap);
        assert!(!resolved.iter().any(|entry| entry.name == "Red"));
        assert_eq!(resolved[0].name, "Crimson");
        assert_eq!(resolved.last().unwrap().name, "Neon Mint");
        assert_eq!(resolved.last().unwrap().hex, "#00F5D4");
    }

    #[test]
    fn remove_color_persists_and_trims_name() {
        let snap = empty_snapshot();
        let outcome = remove_color(" Red ", &snap).expect("valid name");
        let PalettePersistOutcome::SetMap(map) = outcome else {
            panic!("removing a built-in diverges from the default map");
        };
        assert_eq!(map_get(&map, "Red"), None);
        assert_eq!(map.len(), 15);
        // Removing an absent name still persists (Swift persists
        // unconditionally after the name guard) — and since the map equals
        // the defaults, the persist elides the key.
        assert_eq!(
            remove_color("Nonexistent", &snap),
            Some(PalettePersistOutcome::RemoveKey)
        );
        // Untrimmable name is a no-op.
        assert_eq!(remove_color("   ", &snap), None);
    }

    #[test]
    fn persist_normalizes_and_elides_default_map() {
        // Un-normalized spellings of the exact default palette elide the key.
        let map: Vec<(String, String)> = DEFAULT_PALETTE
            .iter()
            .map(|(name, hex)| (format!("  {name} "), hex.to_lowercase()))
            .collect();
        assert_eq!(persist_palette_map(&map), PalettePersistOutcome::RemoveKey);

        // Invalid entries are dropped during normalization.
        let map = vec![
            ("".to_owned(), "#123456".to_owned()),
            ("Ok".to_owned(), "not-a-hex".to_owned()),
            (" Kept ".to_owned(), " #abcdef ".to_owned()),
        ];
        assert_eq!(
            persist_palette_map(&map),
            PalettePersistOutcome::SetMap(vec![("Kept".to_owned(), "#ABCDEF".to_owned())])
        );
    }

    #[test]
    fn stored_all_invalid_yields_empty_effective_palette() {
        // A present, castable dictionary whose entries all fail normalization
        // is Some(empty) in Swift — the palette becomes EMPTY, not default.
        let snap = snapshot_with_stored(&[("", "#123456"), ("Name", "bad")]);
        assert_eq!(effective_palette_map(&snap), Vec::new());
        assert_eq!(palette(&snap), Vec::new());
        assert_eq!(palette_cache_fingerprint(&snap), "");
    }

    // §6 — testLegacyKeysStillResolveIntoEffectivePalette + migration table.
    #[test]
    fn legacy_keys_resolve_into_effective_palette() {
        let snap = PaletteStoreSnapshot {
            stored: None,
            legacy_overrides_present: true,
            legacy_overrides: Some(vec![("Blue".to_owned(), "#010203".to_owned())]),
            legacy_custom_present: true,
            legacy_custom_colors: Some(vec!["#778899".to_owned()]),
        };
        let resolved = palette(&snap);
        assert_eq!(
            resolved.iter().find(|e| e.name == "Blue").map(|e| e.hex.as_str()),
            Some("#010203")
        );
        assert_eq!(
            resolved
                .iter()
                .find(|e| e.name == "Custom 1")
                .map(|e| e.hex.as_str()),
            Some("#778899")
        );
    }

    #[test]
    fn legacy_migration_rules() {
        // Non-built-in override names and invalid hexes are dropped;
        // duplicate custom hexes (within the array) collapse; invalid custom
        // entries are skipped without consuming an index.
        let snap = PaletteStoreSnapshot {
            stored: None,
            legacy_overrides_present: true,
            legacy_overrides: Some(vec![
                ("Blue".to_owned(), "#010203".to_owned()),
                ("NotBuiltIn".to_owned(), "#111111".to_owned()),
                ("Red".to_owned(), "garbage".to_owned()),
                ("blue".to_owned(), "#0A0B0C".to_owned()), // case-sensitive: not built-in
            ]),
            legacy_custom_present: true,
            legacy_custom_colors: Some(vec![
                "bad".to_owned(),
                "#778899".to_owned(),
                " #778899 ".to_owned(), // same normalized hex → deduped
                "#C0392B".to_owned(),   // duplicates built-in Red's hex → NOT deduped
            ]),
        };
        let map = effective_palette_map(&snap);
        assert_eq!(map_get(&map, "Blue"), Some("#010203"));
        assert_eq!(map_get(&map, "Red"), Some("#C0392B")); // invalid override kept default
        assert_eq!(map_get(&map, "NotBuiltIn"), None);
        assert_eq!(map_get(&map, "blue"), None);
        assert_eq!(map_get(&map, "Custom 1"), Some("#778899"));
        assert_eq!(map_get(&map, "Custom 2"), Some("#C0392B"));
        assert_eq!(map_get(&map, "Custom 3"), None);
    }

    #[test]
    fn legacy_presence_without_typed_value_routes_to_legacy_branch() {
        // A wrong-typed legacy value still exists as a key: the legacy branch
        // fires and materializes the default palette (no stored key).
        let snap = PaletteStoreSnapshot {
            stored: None,
            legacy_overrides_present: true,
            legacy_overrides: None,
            legacy_custom_present: false,
            legacy_custom_colors: None,
        };
        assert_eq!(effective_palette_map(&snap), default_palette_map());
        assert_eq!(backup_palette_map(&snap), Some(default_palette_map()));
        // With no legacy keys at all, backup is None.
        assert_eq!(backup_palette_map(&empty_snapshot()), None);
    }

    #[test]
    fn stored_wins_over_legacy() {
        let snap = PaletteStoreSnapshot {
            stored: Some(vec![("Only".to_owned(), "#123456".to_owned())]),
            legacy_overrides_present: true,
            legacy_overrides: Some(vec![("Blue".to_owned(), "#010203".to_owned())]),
            legacy_custom_present: false,
            legacy_custom_colors: None,
        };
        assert_eq!(
            effective_palette_map(&snap),
            vec![("Only".to_owned(), "#123456".to_owned())]
        );
    }

    // §7(a) — resolvedColorHex: HEX FIRST.
    #[test]
    fn resolved_color_hex_hex_first_then_case_insensitive_name() {
        let snap = empty_snapshot();
        // CmuxConfigNamedColorTests: named color resolves via the palette.
        assert_eq!(resolved_color_hex("Indigo", &snap).as_deref(), Some("#283593"));
        assert_eq!(resolved_color_hex("indigo", &snap).as_deref(), Some("#283593"));
        assert_eq!(resolved_color_hex(" INDIGO \n", &snap).as_deref(), Some("#283593"));
        assert_eq!(resolved_color_hex("#abc123", &snap).as_deref(), Some("#ABC123"));
        assert_eq!(resolved_color_hex("Definitely Not A Palette Color", &snap), None);
        assert_eq!(resolved_color_hex("", &snap), None);
        assert_eq!(resolved_color_hex("   ", &snap), None);

        // A palette entry literally named "C0FFEE" is shadowed by hex parsing.
        let mut map = default_palette_map();
        map.push(("C0FFEE".to_owned(), "#111111".to_owned()));
        let snap = stored_from_outcome(persist_palette_map(&map));
        assert_eq!(resolved_color_hex("C0FFEE", &snap).as_deref(), Some("#C0FFEE"));
    }

    // §7(b) — socket set_color: NAME FIRST (opposite precedence).
    #[test]
    fn resolve_set_color_input_name_first_then_hex() {
        let entries = palette(&empty_snapshot());
        assert_eq!(
            resolve_set_color_input("indigo", &entries),
            Ok("#283593".to_owned())
        );
        assert_eq!(
            resolve_set_color_input("  Navy \n", &entries),
            Ok("#1A5276".to_owned())
        );
        assert_eq!(
            resolve_set_color_input("#abc123", &entries),
            Ok("#ABC123".to_owned())
        );
        assert_eq!(resolve_set_color_input("   ", &entries), Err(SetColorError::Missing));
        assert_eq!(SetColorError::Missing.message(), "Missing or invalid color");

        let err = resolve_set_color_input("nope", &entries).unwrap_err();
        let SetColorError::Invalid { named_colors } = &err else {
            panic!("expected Invalid");
        };
        assert_eq!(
            named_colors,
            &DEFAULT_PALETTE.iter().map(|(n, _)| (*n).to_owned()).collect::<Vec<_>>()
        );
        assert_eq!(
            err.message(),
            "Invalid color. Use a hex value (#RRGGBB) or a named color."
        );

        // NAME beats hex here: an entry named "C0FFEE" wins over hex parsing.
        let mut with_shadow = entries.clone();
        with_shadow.push(TabColorEntry {
            name: "C0FFEE".to_owned(),
            hex: "#111111".to_owned(),
        });
        assert_eq!(
            resolve_set_color_input("C0FFEE", &with_shadow),
            Ok("#111111".to_owned())
        );
    }

    // §7(d) — cmux.json decode error string, byte-for-byte.
    #[test]
    fn invalid_color_message_matches_swift_decoding_error() {
        assert_eq!(
            invalid_color_message("Definitely Not A Palette Color"),
            "Invalid color \"Definitely Not A Palette Color\". Expected 6-digit hex format (#RRGGBB) or a workspace color name"
        );
    }

    // §8 — fingerprint.
    #[test]
    fn palette_cache_fingerprint_is_sorted_and_change_sensitive() {
        let snap = snapshot_with_stored(&[
            ("Custom 10", "#101010"),
            ("Blue", "#1565C0"),
            ("Custom 2", "#020202"),
        ]);
        assert_eq!(
            palette_cache_fingerprint(&snap),
            "Blue=#1565C0\nCustom 2=#020202\nCustom 10=#101010"
        );
        // Changing one hex changes the fingerprint
        // (CmuxConfigNamedColorTests:60-99 cache-invalidation behavior).
        let changed = snapshot_with_stored(&[
            ("Custom 10", "#101010"),
            ("Blue", "#1565C0"),
            ("Custom 2", "#030303"),
        ]);
        assert_ne!(
            palette_cache_fingerprint(&snap),
            palette_cache_fingerprint(&changed)
        );
        assert_eq!(palette_cache_fingerprint(&snapshot_with_stored(&[])), "");
    }

    // §11 — finder_like_cmp table.
    #[test]
    fn finder_like_cmp_table() {
        use Ordering::*;
        let cases: &[(&str, &str, Ordering)] = &[
            ("Custom 2", "Custom 10", Less),
            ("Custom 10", "Custom 2", Greater),
            ("custom 2", "Custom 2", Greater), // case-fold equal → byte tiebreak
            ("Custom 2", "Custom 2", Equal),
            ("apple", "Banana", Less), // case-insensitive primary
            ("a2b", "a10a", Less),     // numeric run mid-string
            ("a01", "a1", Less),       // equal numerically → byte tiebreak ("a0" < "a1")
            ("", "a", Less),
            ("Custom", "Custom 1", Less), // prefix
            ("1", "a", Less),
            ("Custom 9", "Custom 10", Less),
            ("x100", "x20", Greater),
        ];
        for (a, b, expected) in cases {
            assert_eq!(finder_like_cmp(a, b), *expected, "finder_like_cmp({a:?}, {b:?})");
        }
        // Custom entries sort numerically inside palette().
        let mut map = default_palette_map();
        map.push(("Custom 10".to_owned(), "#101010".to_owned()));
        map.push(("Custom 2".to_owned(), "#020202".to_owned()));
        let snap = stored_from_outcome(persist_palette_map(&map));
        assert_eq!(
            custom_palette_entries(&snap)
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>(),
            ["Custom 2", "Custom 10"]
        );
    }

    #[test]
    fn default_and_current_color_hex_are_case_sensitive() {
        assert_eq!(default_color_hex("Blue"), Some("#1565C0"));
        assert_eq!(default_color_hex("blue"), None);
        let snap = empty_snapshot();
        assert_eq!(current_color_hex("Blue", &snap).as_deref(), Some("#1565C0"));
        // Case-SENSITIVE asymmetry with the resolvers
        // (TabManager.applyWorkspacePaletteColor no-ops on this miss).
        assert_eq!(current_color_hex("blue", &snap), None);
    }

    // §12 — display math.
    #[test]
    fn display_color_light_mode_keeps_original_hex() {
        // testDisplayColorLightModeKeepsOriginalHex
        assert_eq!(
            display_color_hex("#1A5276", false, false).as_deref(),
            Some("#1A5276")
        );
        // Normalizes on the way through.
        assert_eq!(
            display_color_hex(" 1a5276 ", false, false).as_deref(),
            Some("#1A5276")
        );
        assert_eq!(display_color_hex("nope", true, false), None);
        // Sign-bearing normalize_hex survivors are not renderable colors.
        assert_eq!(display_color_hex("+ABCDE", true, false), None);
    }

    #[test]
    fn display_color_dark_mode_brightens_and_force_bright_applies_in_light() {
        // testDisplayColorDarkModeBrightensColor
        let dark = display_color_hex("#1A5276", true, false).unwrap();
        assert_ne!(dark, "#1A5276");
        let base_rgb = parse_hex_rgb("#1A5276").unwrap();
        let dark_rgb = parse_hex_rgb(&dark).unwrap();
        assert!(luminance(dark_rgb) > luminance(base_rgb));

        // testDisplayColorForceBrightensInLightMode
        let forced = display_color_hex("#1A5276", false, true).unwrap();
        assert_eq!(forced, dark);
        assert_ne!(forced, "#1A5276");

        // Every built-in palette color strictly gains luminance in dark mode.
        for (_, hex) in DEFAULT_PALETTE {
            let base = parse_hex_rgb(hex).unwrap();
            let bright = brightened_for_dark_appearance_rgb(base);
            assert!(
                luminance(bright) > luminance(base),
                "{hex} did not brighten"
            );
        }
    }

    #[test]
    fn display_color_dark_mode_keeps_grayscale_neutral() {
        // testDisplayColorDarkModeKeepsGrayscaleNeutral: saturation 0 is
        // preserved exactly (<= 0.08 gray-preservation branch), so channels
        // stay equal (tighter than Swift's 0.003 tolerance).
        let bright = brightened_for_dark_appearance_rgb([0x80, 0x80, 0x80]);
        assert_eq!(bright[0], bright[1]);
        assert_eq!(bright[1], bright[2]);
        assert!(luminance(bright) > luminance([0x80, 0x80, 0x80]));
    }

    #[test]
    fn component_to_byte_truncates_toward_zero() {
        // Swift Int(0.999 * 255) = Int(254.745) = 254 — truncation, not rounding.
        assert_eq!(component_to_byte(0.999), 254);
        assert_eq!(component_to_byte(1.0), 255);
        assert_eq!(component_to_byte(1.5), 255); // clamped
        assert_eq!(component_to_byte(-0.5), 0); // clamped
        assert_eq!(component_to_byte(0.0), 0);
    }

    #[test]
    fn display_dark_mode_self_consistent_goldens() {
        // Self-consistent goldens from the pure-sRGB f64 pipeline (see the
        // DIVERGENCE note on brightened_for_dark_appearance_rgb: macOS
        // NSColor round-trips may differ by ±1 byte per channel).
        let cases: &[(&str, &str)] = &[
            ("#1A5276", "#2686C4"),
            ("#808080", "#C1C1C1"),
            ("#C0392B", "#D13929"),
            ("#196F3D", "#27C669"),
            ("#3E4B5E", "#7598CB"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                display_color_hex(input, true, false).as_deref(),
                Some(*expected),
                "dark-mode golden for {input}"
            );
        }
    }

    // §13 — Workspace.setCustomColor silent-clear semantics.
    #[test]
    fn normalized_custom_color_silently_clears_invalid_hex() {
        assert_eq!(normalized_custom_color(Some("#abc123")).as_deref(), Some("#ABC123"));
        assert_eq!(normalized_custom_color(Some("nope")), None); // invalid → silent clear
        assert_eq!(normalized_custom_color(None), None);
    }

    #[test]
    fn normalized_palette_map_last_write_wins_on_duplicate_normalized_names() {
        // Two raw spellings normalize to the same key: later entry wins
        // deterministically (Swift dict overwrite is nondeterministic).
        let snap = snapshot_with_stored(&[("Red", "#111111"), (" Red ", "#222222")]);
        assert_eq!(current_color_hex("Red", &snap).as_deref(), Some("#222222"));
    }
}
