//! Windows notification delivery backend seam for cmux.
//!
//! This crate owns the data contracts immediately behind the platform-agnostic
//! notification store: effect gating, toast XML payload construction,
//! supersede tag/group metadata, and custom WAV staging. The WinRT/COM calls
//! that post the toast are intentionally a thin shell layer over these pure
//! plans.

use std::{
    fs,
    path::{Path, PathBuf},
};

use cmux_core::notifications::{
    sound::{windows_toast_audio, WindowsToastAudio},
    DeliveryDecision, TerminalNotification, TerminalNotificationPolicyEffects,
};
use sha2::{Digest, Sha256};

/// A stable app identity for toast delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAppIdentity {
    /// AppUserModelID used by Windows toast APIs.
    pub aumid: String,
    /// URI scheme used by toast activation payloads.
    pub activation_scheme: String,
}

impl NotificationAppIdentity {
    pub fn new(aumid: impl Into<String>, activation_scheme: impl Into<String>) -> Self {
        Self {
            aumid: aumid.into(),
            activation_scheme: activation_scheme.into(),
        }
    }
}

/// Side-effect plan derived from store policy effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliveryPlan {
    pub decision: DeliveryDecision,
    pub toast: Option<WindowsToastPayload>,
    pub run_command: bool,
    pub play_in_app_sound: bool,
}

/// A Windows ToastGeneric payload plus replacement metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsToastPayload {
    pub aumid: String,
    pub tag: String,
    pub group: String,
    pub launch: String,
    pub xml: String,
}

/// Abstraction implemented by the shell-specific WinRT delivery layer.
pub trait NotificationDelivery {
    type Error;

    fn deliver(&self, plan: &NotificationDeliveryPlan) -> Result<(), Self::Error>;
    fn clear(&self, tag: &str, group: &str) -> Result<(), Self::Error>;
}

#[cfg(windows)]
/// Windows WinRT toast sender.
///
/// This is intentionally thin: all policy decisions, payload XML, tag/group
/// replacement metadata, and activation URIs are produced by the pure
/// [`NotificationDeliveryPlan`]. The sender only registers the process AUMID,
/// converts XML to `XmlDocument`, and calls the Windows notification APIs.
#[derive(Debug, Clone)]
pub struct WindowsToastDelivery {
    identity: NotificationAppIdentity,
}

#[cfg(windows)]
#[derive(Debug, thiserror::Error)]
pub enum WindowsToastDeliveryError {
    #[error("notification plan has no toast payload to deliver")]
    MissingToast,
    #[error("failed to register AppUserModelID {aumid}: {source}")]
    RegisterAumid {
        aumid: String,
        source: windows::core::Error,
    },
    #[error("failed to parse toast XML: {source}")]
    Xml { source: windows::core::Error },
    #[error("failed to create Windows toast notification: {source}")]
    CreateToast { source: windows::core::Error },
    #[error("failed to show Windows toast for AUMID {aumid}: {source}")]
    Show {
        aumid: String,
        source: windows::core::Error,
    },
    #[error("failed to clear Windows toast tag {tag} group {group} for AUMID {aumid}: {source}")]
    Clear {
        aumid: String,
        tag: String,
        group: String,
        source: windows::core::Error,
    },
}

#[cfg(windows)]
impl WindowsToastDelivery {
    pub fn new(identity: NotificationAppIdentity) -> Self {
        Self { identity }
    }

    pub fn identity(&self) -> &NotificationAppIdentity {
        &self.identity
    }

    pub fn register_app_user_model_id(&self) -> Result<(), WindowsToastDeliveryError> {
        register_app_user_model_id(&self.identity.aumid)
    }
}

#[cfg(windows)]
impl NotificationDelivery for WindowsToastDelivery {
    type Error = WindowsToastDeliveryError;

    fn deliver(&self, plan: &NotificationDeliveryPlan) -> Result<(), Self::Error> {
        let toast = plan
            .toast
            .as_ref()
            .ok_or(WindowsToastDeliveryError::MissingToast)?;
        self.register_app_user_model_id()?;
        show_toast(toast)
    }

    fn clear(&self, tag: &str, group: &str) -> Result<(), Self::Error> {
        clear_toast(&self.identity.aumid, tag, group)
    }
}

#[cfg(windows)]
pub fn register_app_user_model_id(aumid: &str) -> Result<(), WindowsToastDeliveryError> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let aumid_h = HSTRING::from(aumid);
    unsafe { SetCurrentProcessExplicitAppUserModelID(&aumid_h) }.map_err(|source| {
        WindowsToastDeliveryError::RegisterAumid {
            aumid: aumid.to_owned(),
            source,
        }
    })
}

#[cfg(windows)]
fn show_toast(payload: &WindowsToastPayload) -> Result<(), WindowsToastDeliveryError> {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let document =
        XmlDocument::new().map_err(|source| WindowsToastDeliveryError::Xml { source })?;
    document
        .LoadXml(&HSTRING::from(&payload.xml))
        .map_err(|source| WindowsToastDeliveryError::Xml { source })?;
    let toast = ToastNotification::CreateToastNotification(&document)
        .map_err(|source| WindowsToastDeliveryError::CreateToast { source })?;
    toast
        .SetTag(&HSTRING::from(&payload.tag))
        .map_err(|source| WindowsToastDeliveryError::CreateToast { source })?;
    toast
        .SetGroup(&HSTRING::from(&payload.group))
        .map_err(|source| WindowsToastDeliveryError::CreateToast { source })?;

    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&payload.aumid))
            .map_err(|source| WindowsToastDeliveryError::Show {
                aumid: payload.aumid.clone(),
                source,
            })?;
    notifier
        .Show(&toast)
        .map_err(|source| WindowsToastDeliveryError::Show {
            aumid: payload.aumid.clone(),
            source,
        })
}

#[cfg(windows)]
fn clear_toast(aumid: &str, tag: &str, group: &str) -> Result<(), WindowsToastDeliveryError> {
    use windows::core::HSTRING;
    use windows::UI::Notifications::ToastNotificationManager;

    let history =
        ToastNotificationManager::History().map_err(|source| WindowsToastDeliveryError::Clear {
            aumid: aumid.to_owned(),
            tag: tag.to_owned(),
            group: group.to_owned(),
            source,
        })?;
    history
        .RemoveGroupedTagWithId(
            &HSTRING::from(tag),
            &HSTRING::from(group),
            &HSTRING::from(aumid),
        )
        .map_err(|source| WindowsToastDeliveryError::Clear {
            aumid: aumid.to_owned(),
            tag: tag.to_owned(),
            group: group.to_owned(),
            source,
        })
}

/// Build the complete delivery plan for one notification.
pub fn delivery_plan(
    identity: &NotificationAppIdentity,
    notification: &TerminalNotification,
    effects: &TerminalNotificationPolicyEffects,
    should_suppress_external_delivery: bool,
    sound_value: &str,
    custom_sound_file_path: Option<&str>,
) -> Option<NotificationDeliveryPlan> {
    let decision = cmux_core::notifications::policy::delivery_decision(
        effects,
        should_suppress_external_delivery,
    );
    match decision {
        DeliveryDecision::None => None,
        DeliveryDecision::Suppressed => Some(NotificationDeliveryPlan {
            decision,
            toast: None,
            run_command: false,
            play_in_app_sound: effects.sound,
        }),
        DeliveryDecision::Desktop => {
            let toast = effects.desktop.then(|| {
                let audio = if effects.sound {
                    windows_toast_audio(sound_value, custom_sound_file_path)
                } else {
                    WindowsToastAudio::Silent
                };
                WindowsToastPayload::new(identity, notification, audio)
            });
            Some(NotificationDeliveryPlan {
                decision,
                toast,
                run_command: effects.command,
                play_in_app_sound: effects.sound && !effects.desktop,
            })
        }
    }
}

impl WindowsToastPayload {
    pub fn new(
        identity: &NotificationAppIdentity,
        notification: &TerminalNotification,
        audio: WindowsToastAudio,
    ) -> Self {
        let key = supersede_key(notification);
        let launch = activation_uri(identity, notification);
        let xml = toast_xml(notification, &launch, &audio);
        Self {
            aumid: identity.aumid.clone(),
            tag: key.tag,
            group: key.group,
            launch,
            xml,
        }
    }
}

/// Toast replacement key. Mirrors the port plan's `tag = surfaceId`,
/// `group = tabId`; panel-only notifications use `panelId`, and tab-only
/// notifications fall back to their notification id so they do not collapse an
/// entire tab's unrelated banners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastSupersedeKey {
    pub tag: String,
    pub group: String,
}

pub fn supersede_key(notification: &TerminalNotification) -> ToastSupersedeKey {
    ToastSupersedeKey {
        tag: notification
            .surface_id
            .as_deref()
            .or(notification.panel_id.as_deref())
            .unwrap_or(&notification.id)
            .to_owned(),
        group: notification.tab_id.clone(),
    }
}

pub fn activation_uri(
    identity: &NotificationAppIdentity,
    notification: &TerminalNotification,
) -> String {
    let mut uri = format!(
        "{}://notification?id={}&tabId={}",
        identity.activation_scheme,
        percent_encode(&notification.id),
        percent_encode(&notification.tab_id)
    );
    if let Some(surface_id) = &notification.surface_id {
        uri.push_str("&surfaceId=");
        uri.push_str(&percent_encode(surface_id));
    }
    if let Some(panel_id) = &notification.panel_id {
        uri.push_str("&panelId=");
        uri.push_str(&percent_encode(panel_id));
    }
    uri
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActivation {
    pub notification_id: String,
    pub tab_id: String,
    pub surface_id: Option<String>,
    pub panel_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum NotificationActivationError {
    #[error("notification activation URI must use <scheme>://notification?...")]
    InvalidUri,
    #[error("notification activation URI used scheme {actual:?}, expected {expected:?}")]
    UnexpectedScheme { expected: String, actual: String },
    #[error("notification activation URI target must be notification, got {0:?}")]
    UnexpectedTarget(String),
    #[error("notification activation URI is missing a query string")]
    MissingQuery,
    #[error("notification activation query pair is malformed: {0:?}")]
    MalformedQueryPair(String),
    #[error("notification activation URI is missing required field {0}")]
    MissingField(&'static str),
    #[error("notification activation percent encoding is invalid near {0:?}")]
    InvalidPercentEncoding(String),
    #[error("notification activation percent encoding is not valid UTF-8")]
    InvalidUtf8PercentEncoding,
}

pub fn parse_activation_uri(
    uri: &str,
    expected_scheme: Option<&str>,
) -> Result<NotificationActivation, NotificationActivationError> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or(NotificationActivationError::InvalidUri)?;
    if let Some(expected) = expected_scheme {
        if scheme != expected {
            return Err(NotificationActivationError::UnexpectedScheme {
                expected: expected.to_owned(),
                actual: scheme.to_owned(),
            });
        }
    }

    let (target, query) = rest
        .split_once('?')
        .ok_or(NotificationActivationError::MissingQuery)?;
    if target != "notification" {
        return Err(NotificationActivationError::UnexpectedTarget(
            target.to_owned(),
        ));
    }

    let mut notification_id = None;
    let mut tab_id = None;
    let mut surface_id = None;
    let mut panel_id = None;

    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, raw_value) = pair
            .split_once('=')
            .ok_or_else(|| NotificationActivationError::MalformedQueryPair(pair.to_owned()))?;
        let value = percent_decode(raw_value)?;
        match key {
            "id" if !value.is_empty() => notification_id = Some(value),
            "tabId" if !value.is_empty() => tab_id = Some(value),
            "surfaceId" if !value.is_empty() => surface_id = Some(value),
            "panelId" if !value.is_empty() => panel_id = Some(value),
            _ => {}
        }
    }

    Ok(NotificationActivation {
        notification_id: notification_id.ok_or(NotificationActivationError::MissingField("id"))?,
        tab_id: tab_id.ok_or(NotificationActivationError::MissingField("tabId"))?,
        surface_id,
        panel_id,
    })
}

pub fn toast_xml(
    notification: &TerminalNotification,
    launch: &str,
    audio: &WindowsToastAudio,
) -> String {
    format!(
        r#"<toast launch="{}"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text><text>{}</text></binding></visual>{}</toast>"#,
        escape_xml_attr(launch),
        escape_xml_text(&notification.title),
        escape_xml_text(&notification.subtitle),
        escape_xml_text(&notification.body),
        audio.toast_xml_audio_element()
    )
}

/// A staged custom sound file that can be referenced from toast XML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedCustomSound {
    pub source_path: PathBuf,
    pub staged_path: PathBuf,
    pub toast_uri: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CustomSoundStageError {
    #[error("custom notification sounds currently require a WAV file before Windows staging")]
    TranscodeRequired,
    #[error("custom sound path has no file name: {0}")]
    MissingFileName(String),
    #[error("failed to create custom sound directory {path}: {source}")]
    CreateDir {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to stage custom sound {source_path} to {staged_path}: {source}")]
    Copy {
        source_path: String,
        staged_path: String,
        source: std::io::Error,
    },
}

/// Stage a toast-safe WAV into the app's local sound directory.
///
/// The macOS implementation transcodes arbitrary inputs to CAF. Windows needs a
/// similar media-transcode follow-up; until that shell-specific converter is
/// wired, this function accepts `.wav` and returns an explicit
/// [`CustomSoundStageError::TranscodeRequired`] for every other extension.
pub fn stage_custom_wav_sound(
    source_path: &Path,
    sounds_dir: &Path,
) -> Result<StagedCustomSound, CustomSoundStageError> {
    if !source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        return Err(CustomSoundStageError::TranscodeRequired);
    }

    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CustomSoundStageError::MissingFileName(source_path.display().to_string()))?;
    fs::create_dir_all(sounds_dir).map_err(|source| CustomSoundStageError::CreateDir {
        path: sounds_dir.display().to_string(),
        source,
    })?;

    let staged_name = format!(
        "{}-{}",
        short_hash(source_path),
        sanitize_file_name(file_name)
    );
    let staged_path = sounds_dir.join(staged_name);
    fs::copy(source_path, &staged_path).map_err(|source| CustomSoundStageError::Copy {
        source_path: source_path.display().to_string(),
        staged_path: staged_path.display().to_string(),
        source,
    })?;
    Ok(StagedCustomSound {
        source_path: source_path.to_path_buf(),
        toast_uri: file_uri(&staged_path),
        staged_path,
    })
}

fn short_hash(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sanitize_file_name(file_name: &str) -> String {
    file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file:///");
    let normalized = path.to_string_lossy().replace('\\', "/");
    uri.push_str(&percent_encode_path(&normalized));
    uri
}

fn escape_xml_text(value: &str) -> String {
    escape_xml_attr(value)
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

fn percent_encode(value: &str) -> String {
    percent_encode_with(value, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
    })
}

fn percent_encode_path(value: &str) -> String {
    percent_encode_with(value, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
    })
}

fn percent_encode_with(value: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if keep(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> Result<String, NotificationActivationError> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(NotificationActivationError::InvalidPercentEncoding(
                value[index..].to_owned(),
            ));
        }
        let high = hex_value(bytes[index + 1]);
        let low = hex_value(bytes[index + 2]);
        match (high, low) {
            (Some(high), Some(low)) => {
                decoded.push((high << 4) | low);
                index += 3;
            }
            _ => {
                return Err(NotificationActivationError::InvalidPercentEncoding(
                    value[index..index + 3].to_owned(),
                ));
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| NotificationActivationError::InvalidUtf8PercentEncoding)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification() -> TerminalNotification {
        TerminalNotification {
            id: "notification 1".to_owned(),
            tab_id: "tab-1".to_owned(),
            surface_id: Some("surface-1".to_owned()),
            panel_id: Some("panel-1".to_owned()),
            title: "Build & Ship".to_owned(),
            subtitle: "Workspace <alpha>".to_owned(),
            body: r#"Done "cleanly""#.to_owned(),
            created_at: 1,
            is_read: false,
            pane_flash: true,
            click_action: None,
        }
    }

    fn identity() -> NotificationAppIdentity {
        NotificationAppIdentity::new("Cmuxterm.Cmux.Dev", "cmux-dev")
    }

    #[test]
    fn supersede_key_prefers_surface_then_panel_then_notification_id() {
        let mut n = notification();
        assert_eq!(
            supersede_key(&n),
            ToastSupersedeKey {
                tag: "surface-1".to_owned(),
                group: "tab-1".to_owned(),
            }
        );
        n.surface_id = None;
        assert_eq!(supersede_key(&n).tag, "panel-1");
        n.panel_id = None;
        assert_eq!(supersede_key(&n).tag, "notification 1");
    }

    #[test]
    fn activation_uri_carries_notification_and_targets() {
        assert_eq!(
            activation_uri(&identity(), &notification()),
            "cmux-dev://notification?id=notification%201&tabId=tab-1&surfaceId=surface-1&panelId=panel-1"
        );
    }

    #[test]
    fn parse_activation_uri_round_trips_targets() {
        assert_eq!(
            parse_activation_uri(
                "cmux-dev://notification?id=notification%201&tabId=workspace%20A&surfaceId=surface%2F1&panelId=panel%3A1",
                Some("cmux-dev"),
            )
            .unwrap(),
            NotificationActivation {
                notification_id: "notification 1".to_owned(),
                tab_id: "workspace A".to_owned(),
                surface_id: Some("surface/1".to_owned()),
                panel_id: Some("panel:1".to_owned()),
            }
        );
    }

    #[test]
    fn parse_activation_uri_accepts_panel_only_targets() {
        assert_eq!(
            parse_activation_uri(
                "cmux-dev://notification?id=n1&tabId=workspace-1&panelId=panel-1",
                Some("cmux-dev"),
            )
            .unwrap()
            .panel_id
            .as_deref(),
            Some("panel-1")
        );
    }

    #[test]
    fn parse_activation_uri_rejects_wrong_scheme_or_target() {
        assert!(matches!(
            parse_activation_uri("other://notification?id=n1&tabId=w1", Some("cmux-dev")),
            Err(NotificationActivationError::UnexpectedScheme { .. })
        ));
        assert!(matches!(
            parse_activation_uri("cmux-dev://workspace?id=n1&tabId=w1", Some("cmux-dev")),
            Err(NotificationActivationError::UnexpectedTarget(_))
        ));
    }

    #[test]
    fn parse_activation_uri_requires_core_fields_and_valid_percent_encoding() {
        assert!(matches!(
            parse_activation_uri("cmux-dev://notification?id=n1", Some("cmux-dev")),
            Err(NotificationActivationError::MissingField("tabId"))
        ));
        assert!(matches!(
            parse_activation_uri("cmux-dev://notification?id=n1&tabId=%ZZ", Some("cmux-dev")),
            Err(NotificationActivationError::InvalidPercentEncoding(_))
        ));
    }

    #[test]
    fn toast_xml_escapes_text_and_includes_audio() {
        let xml = toast_xml(
            &notification(),
            "cmux-dev://notification?id=1&tabId=tab",
            &WindowsToastAudio::Silent,
        );
        assert!(xml.contains(r#"launch="cmux-dev://notification?id=1&amp;tabId=tab""#));
        assert!(xml.contains("<text>Build &amp; Ship</text>"));
        assert!(xml.contains("<text>Workspace &lt;alpha&gt;</text>"));
        assert!(xml.contains("<text>Done &quot;cleanly&quot;</text>"));
        assert!(xml.contains(r#"<audio silent="true"/>"#));
    }

    #[test]
    fn desktop_delivery_builds_toast_and_command_plan() {
        let plan = delivery_plan(
            &identity(),
            &notification(),
            &TerminalNotificationPolicyEffects::default(),
            false,
            "Ping",
            None,
        )
        .expect("default effects are deliverable");
        assert_eq!(plan.decision, DeliveryDecision::Desktop);
        assert!(plan.run_command);
        assert!(!plan.play_in_app_sound);
        let toast = plan.toast.expect("desktop effect builds toast");
        assert_eq!(toast.aumid, "Cmuxterm.Cmux.Dev");
        assert_eq!(toast.tag, "surface-1");
        assert_eq!(toast.group, "tab-1");
        assert!(toast.xml.contains(r#"ms-winsoundevent:Notification.IM"#));
    }

    #[test]
    fn suppressed_delivery_never_builds_toast_or_command() {
        let plan = delivery_plan(
            &identity(),
            &notification(),
            &TerminalNotificationPolicyEffects::default(),
            true,
            "default",
            None,
        )
        .expect("suppressed effects still produce in-app feedback");
        assert_eq!(plan.decision, DeliveryDecision::Suppressed);
        assert_eq!(plan.toast, None);
        assert!(!plan.run_command);
        assert!(plan.play_in_app_sound);
    }

    #[test]
    fn sound_only_desktop_plan_uses_in_app_sound_without_toast() {
        let effects = TerminalNotificationPolicyEffects {
            desktop: false,
            command: false,
            ..TerminalNotificationPolicyEffects::default()
        };
        let plan = delivery_plan(
            &identity(),
            &notification(),
            &effects,
            false,
            "default",
            None,
        )
        .unwrap();
        assert_eq!(plan.toast, None);
        assert!(!plan.run_command);
        assert!(plan.play_in_app_sound);
    }

    #[test]
    fn no_deliverable_effect_returns_none() {
        let effects = TerminalNotificationPolicyEffects {
            desktop: false,
            sound: false,
            command: false,
            ..TerminalNotificationPolicyEffects::default()
        };
        assert_eq!(
            delivery_plan(
                &identity(),
                &notification(),
                &effects,
                false,
                "default",
                None
            ),
            None
        );
    }

    #[test]
    fn stages_wav_files_with_file_uri() {
        let root = std::env::temp_dir().join(format!("cmux-notify-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("my sound.wav");
        fs::write(&source, b"RIFF....WAVEfmt ").unwrap();
        let staged = stage_custom_wav_sound(&source, &root.join("sounds")).unwrap();
        assert!(staged.staged_path.exists());
        assert_eq!(
            staged.staged_path.parent(),
            Some(root.join("sounds").as_path())
        );
        assert!(staged.toast_uri.starts_with("file:///"));
        assert!(staged.toast_uri.ends_with("my_sound.wav"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_wav_custom_sound_requires_transcode() {
        let error = stage_custom_wav_sound(Path::new("alert.mp3"), Path::new("sounds"))
            .expect_err("mp3 must not be silently staged as toast-safe");
        assert!(matches!(error, CustomSoundStageError::TranscodeRequired));
    }
}
