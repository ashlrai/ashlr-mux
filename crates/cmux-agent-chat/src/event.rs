//! Push-seam wire types: `AgentEvent` and its payload models.
//!
//! These serialize to the EXACT type-tagged, camelCase objects the renderer
//! consumes — see `webviews/src/agent-session/shared/types.ts`
//! (`AgentEvent`, `AgentSessionTheme`, `AgentSessionRateLimitRow`).

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts")]
use ts_rs::TS;

/// The agent provider identity (`codex | claude | opencode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ProviderId {
    Codex,
    Claude,
    Opencode,
}

impl ProviderId {
    /// Every provider, in the canonical `provider.list` order (Swift
    /// `AgentSessionProviderID.allCases`).
    pub const ALL: [ProviderId; 3] = [Self::Codex, Self::Claude, Self::Opencode];

    /// Parse from the renderer's raw string, returning `None` for unknown values.
    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            "opencode" => Some(Self::Opencode),
            _ => None,
        }
    }

    /// The raw wire string for this provider.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }

    /// The English display name (Swift `displayName` `defaultValue`s).
    ///
    /// Mirrors `AgentSessionProviderID.displayName` / the sibling
    /// `cmux_agent::AgentSessionProviderId::display_name`. The crate is headless
    /// and carries no localization catalog, so the English copy is used verbatim.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude Code",
            Self::Opencode => "OpenCode",
        }
    }

    /// The executable basename (Swift `executableName`, equal to the raw value).
    pub fn executable_name(&self) -> &'static str {
        self.as_str()
    }

    /// The transport kind string (Swift `transportKind`).
    pub fn transport_kind(&self) -> &'static str {
        match self {
            Self::Codex => "stdio-jsonrpc",
            Self::Claude => "stdio-jsonl",
            Self::Opencode => "http-loopback",
        }
    }

    /// The transport launch arguments (Swift `launchArguments`).
    pub fn launch_arguments(&self) -> Vec<String> {
        match self {
            Self::Codex => vec!["app-server".into(), "--listen".into(), "stdio://".into()],
            Self::Claude => vec![
                "-p".into(),
                "--output-format".into(),
                "stream-json".into(),
                "--input-format".into(),
                "stream-json".into(),
                "--include-partial-messages".into(),
                "--verbose".into(),
            ],
            Self::Opencode => vec![
                "serve".into(),
                "--hostname".into(),
                "127.0.0.1".into(),
                "--port".into(),
                "0".into(),
                "--print-logs".into(),
            ],
        }
    }

    /// Whether a session for this provider auto-starts (Swift
    /// `shouldAutoStartSession`: Codex/OpenCode yes, Claude no).
    pub fn should_auto_start_session(&self) -> bool {
        match self {
            Self::Codex | Self::Opencode => true,
            Self::Claude => false,
        }
    }

    /// Whether `provider.started` is emitted immediately on spawn.
    ///
    /// Faithful to `AgentSessionProcessStore.start`: every provider *except*
    /// OpenCode emits `provider.started` right after the process is launched
    /// (`if plan.provider != .opencode { emitStarted(...) }`). OpenCode defers
    /// its `provider.started` until the loopback HTTP session has been created
    /// (Swift `createOpenCodeSession`).
    pub fn emits_started_on_spawn(&self) -> bool {
        !matches!(self, Self::Opencode)
    }
}

/// The stdio stream an output chunk arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ProviderStream {
    Stdout,
    Stderr,
}

/// The kind of provider activity being reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ActivityKind {
    Command,
    FileChange,
    Other,
}

/// The lifecycle status of a provider activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum ActivityStatus {
    InProgress,
    Completed,
    Failed,
    Stopped,
}

/// Which rate-limit window a row describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum RateLimitRole {
    Primary,
    Secondary,
}

/// The theme palette handed to the renderer.
///
/// Mirrors `AgentSessionTheme` in `types.ts` field-for-field (camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct AgentSessionTheme {
    pub is_dark: bool,
    pub page_background: String,
    pub surface_background: String,
    pub surface_elevated_background: String,
    pub input_background: String,
    pub border: String,
    pub border_strong: String,
    pub text: String,
    pub muted_text: String,
    pub soft_text: String,
    pub accent: String,
    pub accent_soft: String,
    pub danger: String,
    pub shadow: String,
}

/// A single rate-limit row.
///
/// Mirrors `AgentSessionRateLimitRow` in `types.ts`; the trailing three fields
/// are optional and MUST be omitted (not `null`) when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct AgentSessionRateLimitRow {
    pub role: RateLimitRole,
    pub remaining_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub used_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub window_duration_mins: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub resets_at: Option<f64>,
}

/// The push-seam event union.
///
/// Serializes with a `"type"` discriminator to the exact wire objects in
/// `types.ts`. Optional activity fields (`detail`, `outputDelta`) are omitted
/// when absent rather than serialized as `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum AgentEvent {
    #[serde(rename = "app.theme")]
    AppTheme { theme: AgentSessionTheme },

    #[serde(rename = "app.rateLimitRows", rename_all = "camelCase")]
    AppRateLimitRows {
        rate_limit_rows: Vec<AgentSessionRateLimitRow>,
    },

    #[serde(rename = "provider.started", rename_all = "camelCase")]
    ProviderStarted {
        session_id: String,
        provider_id: ProviderId,
        executable_path: String,
        arguments: Vec<String>,
    },

    #[serde(rename = "provider.output", rename_all = "camelCase")]
    ProviderOutput {
        session_id: String,
        provider_id: ProviderId,
        stream: ProviderStream,
        text: String,
    },

    #[serde(rename = "provider.activity", rename_all = "camelCase")]
    ProviderActivity {
        session_id: String,
        provider_id: ProviderId,
        activity_id: String,
        kind: ActivityKind,
        status: ActivityStatus,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        output_delta: Option<String>,
    },

    #[serde(rename = "provider.turnComplete", rename_all = "camelCase")]
    ProviderTurnComplete {
        session_id: String,
        provider_id: ProviderId,
    },

    #[serde(rename = "provider.exit", rename_all = "camelCase")]
    ProviderExit {
        session_id: String,
        provider_id: ProviderId,
        status: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_id_round_trips() {
        for id in [ProviderId::Codex, ProviderId::Claude, ProviderId::Opencode] {
            assert_eq!(ProviderId::from_raw(id.as_str()), Some(id));
            assert_eq!(serde_json::to_value(id).unwrap(), json!(id.as_str()));
        }
    }

    #[test]
    fn app_theme_type_tag() {
        let theme = AgentSessionTheme {
            is_dark: true,
            page_background: "#000".into(),
            surface_background: "#111".into(),
            surface_elevated_background: "#222".into(),
            input_background: "#333".into(),
            border: "#444".into(),
            border_strong: "#555".into(),
            text: "#fff".into(),
            muted_text: "#aaa".into(),
            soft_text: "#bbb".into(),
            accent: "#0af".into(),
            accent_soft: "#08c".into(),
            danger: "#f00".into(),
            shadow: "#0008".into(),
        };
        let v = serde_json::to_value(AgentEvent::AppTheme { theme }).unwrap();
        assert_eq!(v["type"], json!("app.theme"));
        assert_eq!(v["theme"]["isDark"], json!(true));
        assert_eq!(v["theme"]["pageBackground"], json!("#000"));
    }

    #[test]
    fn provider_started_wire_shape() {
        let v = serde_json::to_value(AgentEvent::ProviderStarted {
            session_id: "s1".into(),
            provider_id: ProviderId::Codex,
            executable_path: "/bin/codex".into(),
            arguments: vec!["--flag".into()],
        })
        .unwrap();
        assert_eq!(v["type"], json!("provider.started"));
        assert_eq!(v["sessionId"], json!("s1"));
        assert_eq!(v["providerId"], json!("codex"));
        assert_eq!(v["executablePath"], json!("/bin/codex"));
        assert_eq!(v["arguments"], json!(["--flag"]));
    }

    #[test]
    fn activity_round_trip_with_optionals() {
        let event = AgentEvent::ProviderActivity {
            session_id: "s1".into(),
            provider_id: ProviderId::Claude,
            activity_id: "a1".into(),
            kind: ActivityKind::FileChange,
            status: ActivityStatus::InProgress,
            action: "edit".into(),
            detail: Some("main.rs".into()),
            output_delta: Some("+1".into()),
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], json!("provider.activity"));
        assert_eq!(v["kind"], json!("fileChange"));
        assert_eq!(v["status"], json!("inProgress"));
        assert_eq!(v["detail"], json!("main.rs"));
        assert_eq!(v["outputDelta"], json!("+1"));

        let back: AgentEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn activity_omits_absent_optionals() {
        let event = AgentEvent::ProviderActivity {
            session_id: "s1".into(),
            provider_id: ProviderId::Codex,
            activity_id: "a1".into(),
            kind: ActivityKind::Command,
            status: ActivityStatus::Completed,
            action: "run".into(),
            detail: None,
            output_delta: None,
        };
        let v = serde_json::to_value(&event).unwrap();
        let obj = v.as_object().unwrap();
        // Absent optionals MUST be omitted, not serialized as null.
        assert!(!obj.contains_key("detail"), "detail must be omitted");
        assert!(!obj.contains_key("outputDelta"), "outputDelta must be omitted");
        // Present keys are still emitted.
        assert_eq!(v["action"], json!("run"));
    }

    #[test]
    fn provider_exit_status_is_number() {
        let v = serde_json::to_value(AgentEvent::ProviderExit {
            session_id: "s1".into(),
            provider_id: ProviderId::Opencode,
            status: -1,
        })
        .unwrap();
        assert_eq!(v["type"], json!("provider.exit"));
        assert_eq!(v["status"], json!(-1));
    }

    #[test]
    fn rate_limit_rows_wire_shape_and_omission() {
        let event = AgentEvent::AppRateLimitRows {
            rate_limit_rows: vec![AgentSessionRateLimitRow {
                role: RateLimitRole::Primary,
                remaining_percent: 42.5,
                used_percent: None,
                window_duration_mins: None,
                resets_at: None,
            }],
        };
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], json!("app.rateLimitRows"));
        let row = &v["rateLimitRows"][0];
        assert_eq!(row["role"], json!("primary"));
        assert_eq!(row["remainingPercent"], json!(42.5));
        let obj = row.as_object().unwrap();
        assert!(!obj.contains_key("usedPercent"));
        assert!(!obj.contains_key("windowDurationMins"));
        assert!(!obj.contains_key("resetsAt"));
    }
}
