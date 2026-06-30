//! Pure notification-sound value selection.
//!
//! Ported from `cmux/Sources/NotificationSoundSettings.swift` — the
//! `systemSounds` table (75-93) and the value switch shared by `sound()`,
//! `usesSystemSound`, `isSilent`, and `isCustomFileSelected` (95-140).
//!
//! All playback / staging is deferred (M10 GUI + WS2): `NSSound`/`afconvert`
//! transcode to CAF, `%LOCALAPPDATA%` staging, `runCustomCommand` subprocess,
//! and the `SHQueryUserNotificationState` Focus/DND query. Only the pure value
//! mapping and the documented fail-open Focus contract live here.

/// The selected notification-sound value, resolved from the raw settings string
/// (and, for the custom-file case, the configured path).
///
/// Mirrors the cases of Swift `NotificationSoundSettings.sound()`
/// (`NotificationSoundSettings.swift` 95-120):
/// - `"default"` → [`NotificationSound::Default`]
/// - `"none"` → [`NotificationSound::None`]
/// - `"custom_file"` → [`NotificationSound::Custom`] when a non-empty path is
///   configured, else [`NotificationSound::None`]
/// - any other value → [`NotificationSound::System`] (named system sound)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationSound {
    Default,
    None,
    Custom(String),
    System(String),
}

/// Settings key (Swift `NotificationSoundSettings.key`).
pub const KEY: &str = "notificationSound";
/// Default settings value (Swift `defaultValue`).
pub const DEFAULT_VALUE: &str = "default";
/// Sentinel value selecting a custom file (Swift `customFileValue`).
pub const CUSTOM_FILE_VALUE: &str = "custom_file";
/// Sentinel value for silence.
pub const NONE_VALUE: &str = "none";

/// The (label, value) options surfaced in settings.
///
/// Verbatim from Swift `NotificationSoundSettings.systemSounds`
/// (`NotificationSoundSettings.swift` 75-93).
pub const SYSTEM_SOUNDS: &[(&str, &str)] = &[
    ("Default", "default"),
    ("Basso", "Basso"),
    ("Blow", "Blow"),
    ("Bottle", "Bottle"),
    ("Frog", "Frog"),
    ("Funk", "Funk"),
    ("Glass", "Glass"),
    ("Hero", "Hero"),
    ("Morse", "Morse"),
    ("Ping", "Ping"),
    ("Pop", "Pop"),
    ("Purr", "Purr"),
    ("Sosumi", "Sosumi"),
    ("Submarine", "Submarine"),
    ("Tink", "Tink"),
    ("Custom File...", CUSTOM_FILE_VALUE),
    ("None", NONE_VALUE),
];

/// Trim-and-empty-check a raw custom-file path.
///
/// Mirrors Swift `normalizedCustomFilePath` (`NotificationSoundSettings.swift`
/// 442-446): trims leading/trailing whitespace and newlines; empty → `None`.
pub fn normalized_custom_file_path(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve the selected sound from the raw settings value.
///
/// DEVIATION from Swift `sound()`: this is the pure value mapping only — it does
/// not perform the staging-availability / system-sound-file-existence checks
/// (Swift returns `.default` as a fallback when a system sound can't be staged,
/// and `nil` when a custom file isn't staged yet). Those filesystem checks are
/// deferred to the WS2 staging layer.
pub fn sound(value: &str, custom_file_path: Option<&str>) -> NotificationSound {
    match value {
        DEFAULT_VALUE => NotificationSound::Default,
        NONE_VALUE => NotificationSound::None,
        CUSTOM_FILE_VALUE => match normalized_custom_file_path(custom_file_path) {
            Some(path) => NotificationSound::Custom(path),
            None => NotificationSound::None,
        },
        other => NotificationSound::System(other.to_string()),
    }
}

/// Whether the selection ultimately plays a sound.
///
/// Mirrors Swift `usesSystemSound` (`NotificationSoundSettings.swift` 122-132):
/// `"none"` → false; `"custom_file"` → true only when a path is configured;
/// everything else → true.
pub fn uses_system_sound(value: &str, custom_file_path: Option<&str>) -> bool {
    match value {
        NONE_VALUE => false,
        CUSTOM_FILE_VALUE => normalized_custom_file_path(custom_file_path).is_some(),
        _ => true,
    }
}

/// Whether the selection is explicitly silent.
///
/// Mirrors Swift `isSilent` (`NotificationSoundSettings.swift` 134-136).
pub fn is_silent(value: &str) -> bool {
    value == NONE_VALUE
}

/// Whether the custom-file option is selected.
///
/// Mirrors Swift `isCustomFileSelected` (`NotificationSoundSettings.swift`
/// 138-140).
pub fn is_custom_file_selected(value: &str) -> bool {
    value == CUSTOM_FILE_VALUE
}

/// Whether an active OS Focus / Do-Not-Disturb mode should silence the
/// fallback sound.
///
/// CONTRACT (documented, query deferred): this fails open — any read/parse
/// error of the OS state returns `false` so sound keeps working. Mirrors the
/// fail-open behavior of Swift `isSuppressedByActiveFocus`
/// (`NotificationSoundSettings.swift` 293-309). The Windows implementation
/// (`SHQueryUserNotificationState`) is deferred to WS2; this stub always
/// returns the fail-open `false`.
pub fn is_suppressed_by_active_focus() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_sounds_table_matches_swift() {
        assert_eq!(SYSTEM_SOUNDS.len(), 17);
        assert_eq!(SYSTEM_SOUNDS.first(), Some(&("Default", "default")));
        assert_eq!(SYSTEM_SOUNDS.last(), Some(&("None", "none")));
        assert!(SYSTEM_SOUNDS.iter().any(|(_, v)| *v == "custom_file"));
    }

    #[test]
    fn sound_maps_each_case() {
        assert_eq!(sound("default", None), NotificationSound::Default);
        assert_eq!(sound("none", None), NotificationSound::None);
        assert_eq!(
            sound("Glass", None),
            NotificationSound::System("Glass".into())
        );
        assert_eq!(
            sound("custom_file", Some("/path/to.wav")),
            NotificationSound::Custom("/path/to.wav".into())
        );
        // custom selected but no path → silent
        assert_eq!(sound("custom_file", None), NotificationSound::None);
        assert_eq!(sound("custom_file", Some("   ")), NotificationSound::None);
    }

    #[test]
    fn uses_system_sound_matches_swift() {
        assert!(uses_system_sound("default", None));
        assert!(uses_system_sound("Glass", None));
        assert!(!uses_system_sound("none", None));
        assert!(uses_system_sound("custom_file", Some("/a.wav")));
        assert!(!uses_system_sound("custom_file", None));
    }

    #[test]
    fn silent_and_custom_predicates() {
        assert!(is_silent("none"));
        assert!(!is_silent("default"));
        assert!(is_custom_file_selected("custom_file"));
        assert!(!is_custom_file_selected("default"));
    }

    #[test]
    fn normalized_path_trims_and_rejects_empty() {
        assert_eq!(normalized_custom_file_path(None), None);
        assert_eq!(normalized_custom_file_path(Some("  \n")), None);
        assert_eq!(
            normalized_custom_file_path(Some("  /a/b.wav \n")),
            Some("/a/b.wav".into())
        );
    }

    #[test]
    fn focus_gate_fails_open() {
        assert!(!is_suppressed_by_active_focus());
    }
}
