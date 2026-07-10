//! Pure notification-sound value selection.
//!
//! Ported from `cmux/Sources/NotificationSoundSettings.swift` — the
//! `systemSounds` table (75-93) and the value switch shared by `sound()`,
//! `usesSystemSound`, `isSilent`, and `isCustomFileSelected` (95-140).
//!
//! Playback and custom-file staging are host seams (M10 GUI + WS2):
//! `NSSound`/`afconvert` transcode to CAF on macOS, while Windows stages WAVs
//! under `%LOCALAPPDATA%`. The pure Windows toast mapping lives here so both
//! the toast XML builder and tests share one substitution table.

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

/// Windows toast audio selection derived from [`NotificationSound`].
///
/// Windows exposes a small fixed set of toast system sounds through
/// `ms-winsoundevent:` URIs. The macOS sound names have no exact Windows
/// equivalent, so named sounds are mapped through
/// [`WINDOWS_TOAST_SOUND_SUBSTITUTIONS`]. Custom paths are carried through for
/// the later staging layer, which will translate user files to toast-safe WAV
/// URIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsToastAudio {
    /// Play the default Windows toast notification sound.
    Default,
    /// Emit `<audio silent="true" />`.
    Silent,
    /// Use one of Windows' built-in `ms-winsoundevent:` notification sounds.
    Event(&'static str),
    /// A user-selected file path before staging/transcoding.
    CustomFile(String),
}

impl WindowsToastAudio {
    /// `src` attribute for `<audio>`, when this audio choice has one.
    pub fn src(&self) -> Option<&str> {
        match self {
            Self::Default => Some(WINDOWS_TOAST_SOUND_DEFAULT),
            Self::Silent => None,
            Self::Event(src) => Some(src),
            Self::CustomFile(path) => Some(path),
        }
    }

    /// Whether the toast XML should set `silent="true"`.
    pub fn is_silent(&self) -> bool {
        matches!(self, Self::Silent)
    }

    /// Complete `<audio .../>` element for Windows toast XML.
    pub fn toast_xml_audio_element(&self) -> String {
        if self.is_silent() {
            return r#"<audio silent="true"/>"#.to_owned();
        }
        let src = self.src().unwrap_or(WINDOWS_TOAST_SOUND_DEFAULT);
        format!(r#"<audio src="{}"/>"#, escape_xml_attr(src))
    }
}

fn escape_xml_attr(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Settings key (Swift `NotificationSoundSettings.key`).
pub const KEY: &str = "notificationSound";
/// Default settings value (Swift `defaultValue`).
pub const DEFAULT_VALUE: &str = "default";
/// Sentinel value selecting a custom file (Swift `customFileValue`).
pub const CUSTOM_FILE_VALUE: &str = "custom_file";
/// Sentinel value for silence.
pub const NONE_VALUE: &str = "none";

/// Windows toast audio URI for the default notification sound.
pub const WINDOWS_TOAST_SOUND_DEFAULT: &str = "ms-winsoundevent:Notification.Default";
/// Windows toast audio URI for the instant-message notification sound.
pub const WINDOWS_TOAST_SOUND_IM: &str = "ms-winsoundevent:Notification.IM";
/// Windows toast audio URI for the mail notification sound.
pub const WINDOWS_TOAST_SOUND_MAIL: &str = "ms-winsoundevent:Notification.Mail";
/// Windows toast audio URI for the reminder notification sound.
pub const WINDOWS_TOAST_SOUND_REMINDER: &str = "ms-winsoundevent:Notification.Reminder";
/// Windows toast audio URI for the SMS notification sound.
pub const WINDOWS_TOAST_SOUND_SMS: &str = "ms-winsoundevent:Notification.SMS";

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

/// Best-effort substitutions from macOS named alert sounds to Windows toast
/// system sounds.
///
/// Windows does not ship analogues for the 15 named macOS sounds. This table is
/// intentionally small and stable: bright/short sounds map to `IM`, message-like
/// sounds map to `SMS`/`Mail`, and lower/longer attention sounds map to
/// `Reminder`. Unknown future names fail open to
/// [`WINDOWS_TOAST_SOUND_DEFAULT`].
pub const WINDOWS_TOAST_SOUND_SUBSTITUTIONS: &[(&str, &str)] = &[
    ("Basso", WINDOWS_TOAST_SOUND_REMINDER),
    ("Blow", WINDOWS_TOAST_SOUND_SMS),
    ("Bottle", WINDOWS_TOAST_SOUND_IM),
    ("Frog", WINDOWS_TOAST_SOUND_IM),
    ("Funk", WINDOWS_TOAST_SOUND_SMS),
    ("Glass", WINDOWS_TOAST_SOUND_DEFAULT),
    ("Hero", WINDOWS_TOAST_SOUND_REMINDER),
    ("Morse", WINDOWS_TOAST_SOUND_SMS),
    ("Ping", WINDOWS_TOAST_SOUND_IM),
    ("Pop", WINDOWS_TOAST_SOUND_IM),
    ("Purr", WINDOWS_TOAST_SOUND_DEFAULT),
    ("Sosumi", WINDOWS_TOAST_SOUND_MAIL),
    ("Submarine", WINDOWS_TOAST_SOUND_REMINDER),
    ("Tink", WINDOWS_TOAST_SOUND_IM),
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

/// Map a macOS notification-sound setting into a Windows toast audio choice.
pub fn windows_toast_audio(value: &str, custom_file_path: Option<&str>) -> WindowsToastAudio {
    match sound(value, custom_file_path) {
        NotificationSound::Default => WindowsToastAudio::Default,
        NotificationSound::None => WindowsToastAudio::Silent,
        NotificationSound::Custom(path) => WindowsToastAudio::CustomFile(path),
        NotificationSound::System(name) => {
            WindowsToastAudio::Event(windows_toast_sound_for_system_sound(&name))
        }
    }
}

/// Windows `ms-winsoundevent:` URI for a named macOS system sound.
pub fn windows_toast_sound_for_system_sound(name: &str) -> &'static str {
    WINDOWS_TOAST_SOUND_SUBSTITUTIONS
        .iter()
        .find_map(|(mac_name, windows_sound)| (*mac_name == name).then_some(*windows_sound))
        .unwrap_or(WINDOWS_TOAST_SOUND_DEFAULT)
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

/// Raw Windows `QUNS_QUIET_TIME` value returned by
/// `SHQueryUserNotificationState`.
///
/// Kept as an integer so the pure mapper remains testable on every platform
/// without pulling Windows bindings into non-Windows builds.
pub const WINDOWS_QUNS_QUIET_TIME: i32 = 6;

/// Whether a Windows user-notification state should suppress cmux's direct
/// fallback sound.
///
/// The port plan calls out `QUNS_QUIET_TIME` as the Focus Assist / Quiet Hours
/// signal. Every other state, including unknown future values, fails open so a
/// flaky OS query does not permanently silence notifications.
pub fn windows_notification_state_suppresses_sound(state: i32) -> bool {
    state == WINDOWS_QUNS_QUIET_TIME
}

/// Whether an active OS Focus / Do-Not-Disturb mode should silence the
/// fallback sound.
///
/// CONTRACT: this fails open — any read/parse/query error of the OS state
/// returns `false` so sound keeps working. Mirrors the fail-open behavior of
/// Swift `isSuppressedByActiveFocus` (`NotificationSoundSettings.swift`
/// 293-309).
pub fn is_suppressed_by_active_focus() -> bool {
    is_suppressed_by_active_focus_impl()
}

#[cfg(windows)]
fn is_suppressed_by_active_focus_impl() -> bool {
    use windows::Win32::UI::Shell::SHQueryUserNotificationState;

    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => windows_notification_state_suppresses_sound(state.0),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn is_suppressed_by_active_focus_impl() -> bool {
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
    fn windows_substitution_table_covers_named_mac_sounds() {
        let named_mac_sounds: Vec<&str> = SYSTEM_SOUNDS
            .iter()
            .map(|(_, value)| *value)
            .filter(|value| !matches!(*value, DEFAULT_VALUE | CUSTOM_FILE_VALUE | NONE_VALUE))
            .collect();
        assert_eq!(
            named_mac_sounds.len(),
            WINDOWS_TOAST_SOUND_SUBSTITUTIONS.len()
        );
        for name in named_mac_sounds {
            let mapped = windows_toast_sound_for_system_sound(name);
            assert!(
                mapped.starts_with("ms-winsoundevent:Notification."),
                "{name} mapped to unsupported Windows toast sound {mapped}"
            );
        }
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
    fn windows_toast_audio_maps_each_case() {
        assert_eq!(
            windows_toast_audio("default", None),
            WindowsToastAudio::Default
        );
        assert_eq!(windows_toast_audio("none", None), WindowsToastAudio::Silent);
        assert_eq!(
            windows_toast_audio("Ping", None),
            WindowsToastAudio::Event(WINDOWS_TOAST_SOUND_IM)
        );
        assert_eq!(
            windows_toast_audio("custom_file", Some(" C:/sounds/cmux.wav ")),
            WindowsToastAudio::CustomFile("C:/sounds/cmux.wav".to_owned())
        );
        assert_eq!(
            windows_toast_audio("custom_file", None),
            WindowsToastAudio::Silent
        );
        assert_eq!(
            windows_toast_audio("UnknownFutureSound", None),
            WindowsToastAudio::Event(WINDOWS_TOAST_SOUND_DEFAULT)
        );
    }

    #[test]
    fn windows_toast_audio_exposes_xml_audio_attributes() {
        assert_eq!(
            WindowsToastAudio::Default.src(),
            Some(WINDOWS_TOAST_SOUND_DEFAULT)
        );
        assert!(!WindowsToastAudio::Default.is_silent());
        assert_eq!(WindowsToastAudio::Silent.src(), None);
        assert!(WindowsToastAudio::Silent.is_silent());
        assert_eq!(
            WindowsToastAudio::CustomFile("file:///C:/cmux.wav".to_owned()).src(),
            Some("file:///C:/cmux.wav")
        );
    }

    #[test]
    fn windows_toast_audio_builds_xml_element() {
        assert_eq!(
            WindowsToastAudio::Default.toast_xml_audio_element(),
            r#"<audio src="ms-winsoundevent:Notification.Default"/>"#
        );
        assert_eq!(
            WindowsToastAudio::Silent.toast_xml_audio_element(),
            r#"<audio silent="true"/>"#
        );
        assert_eq!(
            WindowsToastAudio::CustomFile(r#"file:///C:/cmux/a&"b.wav"#.to_owned())
                .toast_xml_audio_element(),
            r#"<audio src="file:///C:/cmux/a&amp;&quot;b.wav"/>"#
        );
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
    fn windows_quiet_time_suppresses_sound() {
        assert!(windows_notification_state_suppresses_sound(
            WINDOWS_QUNS_QUIET_TIME
        ));
        for state in [1, 2, 3, 4, 5, 7, 99] {
            assert!(!windows_notification_state_suppresses_sound(state));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn focus_gate_fails_open() {
        assert!(!is_suppressed_by_active_focus());
    }
}
