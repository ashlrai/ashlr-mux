use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

#[cfg(feature = "ts")]
use ts_rs::TS;

// NOTE (WS1 / cmux core-types): the `#[cfg_attr(feature = "ts", ...)]` derives
// below are the only thing the optional `ts` feature adds. They are inert in the
// default build (ts-rs is an optional dep), so `cargo test`/`clippy` for the
// default configuration are unaffected. `#[ts(export)]` makes each type emit a
// `.ts` file into `TS_RS_EXPORT_DIR` when the `export_bindings_*` tests run.

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPaneLayoutSnapshot {
    /// Stable pane identity (the canonical bonsplit pane id). Optional for
    /// wire/back-compatibility: older snapshots decode without it and the
    /// stateful desktop layer can mint one exactly once during restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pane_id: Option<String>,
    pub panel_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub selected_panel_id: Option<String>,
    /// The kind of surface this pane hosts: a terminal shell (the default, when
    /// absent) or a canonical agent session (`"agent"`). Mirrors the macOS model
    /// where a pane's surface is a terminal *or* an agent-session — the web
    /// renderer branches on this to mount a `TerminalSurface` or the reused
    /// agent-session app. Absent = terminal, so existing snapshots decode
    /// unchanged and the wire stays backward-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surface_kind: Option<String>,
    /// The markdown file currently bound to this pane's markdown surface, when
    /// any. Pane-local on purpose: the Windows port swaps a pane between
    /// terminal/agent/markdown/diff surfaces, so the markdown viewer needs a
    /// stable place to remember which file to reopen when the pane returns to
    /// `"markdown"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub markdown_file_path: Option<String>,
    /// The plain-text file currently bound to this pane's file editor surface,
    /// when any. Separate from `markdown_file_path` so rendered markdown and
    /// editable text files can be restored independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub file_path: Option<String>,
    /// The diff-viewer session token currently bound to this pane's diff
    /// surface, when any. Pane-local for the same reason as
    /// `markdown_file_path`: the Windows port swaps a pane between multiple
    /// surfaces and needs to remember which live diff session to reopen when
    /// returning to `"diff"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub diff_viewer_token: Option<String>,
    /// The diff-viewer request path to navigate within `diff_viewer_token`,
    /// usually `/index.html`. Stored alongside the token so restored panes can
    /// reopen the same entry page rather than assuming the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub diff_viewer_request_path: Option<String>,
    /// The browser URL currently bound to this pane's browser surface. This is
    /// pane-local in the Windows port because the surface kind lives on the
    /// pane, not on separate per-panel BrowserPanel objects yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_url: Option<String>,
    /// Optional loopback proxy endpoint for the pane-local browser surface.
    /// Remote workspaces can publish `http://host:port` or `socks5://host:port`
    /// here so the Tauri/WebView child is created with matching proxy routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_proxy_url: Option<String>,
    /// Back navigation history for the pane-local browser surface. The current
    /// Windows implementation stores history as simple URL stacks on the pane;
    /// the last entry is the next URL to visit when going back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_back_history: Option<Vec<String>>,
    /// Forward navigation history for the pane-local browser surface. The last
    /// entry is the next URL to visit when going forward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_forward_history: Option<Vec<String>>,
    /// Whether the browser omnibar/toolbar is visible. Absent defaults to
    /// visible for back-compatibility, matching the port's original always-on
    /// browser chrome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_omnibar_visible: Option<bool>,
    /// Whether browser focus mode is active for this pane. Focus mode is a
    /// pane-local browser UI state in canonical cmux; absent defaults to off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_focus_mode_active: Option<bool>,
    /// Whether the browser developer-tools drawer is visible. Absent defaults
    /// to hidden, matching canonical BrowserPanel snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_developer_tools_visible: Option<bool>,
    /// Lightweight devtools lane selected by palette commands (`"inspector"`,
    /// `"console"`, or `"react"`). The drawer is still pane-local browser
    /// state, and unknown/future strings are tolerated by the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_developer_tools_panel: Option<String>,
    /// The browser surface zoom factor. Canonical cmux persists pageZoom on
    /// BrowserPanel snapshots; the Windows port stores the equivalent with the
    /// pane-local browser state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser_page_zoom: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum SessionSplitOrientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionSplitLayoutSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub split_id: Option<String>,
    pub orientation: SessionSplitOrientation,
    pub divider_position: f64,
    pub first: Box<SessionWorkspaceLayoutSnapshot>,
    pub second: Box<SessionWorkspaceLayoutSnapshot>,
}

// `SessionWorkspaceLayoutSnapshot` carries hand-written `Serialize`/`Deserialize`
// impls (the `{type, pane|split}` adjacently-tagged wire shape), so ts-rs cannot
// derive a matching `TS`. The manual `TS` impl below (also feature-gated) mirrors
// that exact wire shape. See the `impl TS` block further down.
// Keep the persisted wire variants direct: boxing `Pane` would churn the public
// snapshot API solely to satisfy an in-memory size heuristic.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum SessionWorkspaceLayoutSnapshot {
    Pane(SessionPaneLayoutSnapshot),
    Split(SessionSplitLayoutSnapshot),
}

impl Serialize for SessionWorkspaceLayoutSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            Self::Pane(pane) => {
                map.serialize_entry("type", "pane")?;
                map.serialize_entry("pane", pane)?;
            }
            Self::Split(split) => {
                map.serialize_entry("type", "split")?;
                map.serialize_entry("split", split)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SessionWorkspaceLayoutSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("layout snapshot must be an object"))?;
        match object.get("type").and_then(serde_json::Value::as_str) {
            Some("pane") => {
                let pane = object
                    .get("pane")
                    .cloned()
                    .ok_or_else(|| D::Error::custom("missing pane payload"))?;
                Ok(Self::Pane(
                    serde_json::from_value(pane).map_err(D::Error::custom)?,
                ))
            }
            Some("split") => {
                let split = object
                    .get("split")
                    .cloned()
                    .ok_or_else(|| D::Error::custom("missing split payload"))?;
                Ok(Self::Split(
                    serde_json::from_value(split).map_err(D::Error::custom)?,
                ))
            }
            Some(other) => Err(D::Error::custom(format!(
                "unsupported layout node type: {other}"
            ))),
            None => Err(D::Error::custom("missing layout type")),
        }
    }
}

// Manual `TS` impl for the hand-serialized tagged union. ts-rs derive can't
// express the variant-named content key (`pane`/`split`), so we describe the
// shape by hand. The `.ts` file itself is written by `ts_export::export_manual_
// bindings`; this impl exists so dependent types (which reference this one)
// compile under `--features ts`, import it correctly, and inline it accurately.
#[cfg(feature = "ts")]
impl TS for SessionWorkspaceLayoutSnapshot {
    type WithoutGenerics = Self;

    fn ident() -> String {
        "SessionWorkspaceLayoutSnapshot".to_owned()
    }

    fn name() -> String {
        "SessionWorkspaceLayoutSnapshot".to_owned()
    }

    fn inline() -> String {
        "{ type: \"pane\", pane: SessionPaneLayoutSnapshot } \
         | { type: \"split\", split: SessionSplitLayoutSnapshot }"
            .to_owned()
    }

    fn decl() -> String {
        format!("type {} = {};", Self::ident(), Self::inline())
    }

    fn decl_concrete() -> String {
        Self::decl()
    }

    fn inline_flattened() -> String {
        panic!("SessionWorkspaceLayoutSnapshot cannot be flattened")
    }

    fn visit_dependencies(v: &mut impl ts_rs::TypeVisitor)
    where
        Self: 'static,
    {
        v.visit::<SessionPaneLayoutSnapshot>();
        v.visit::<SessionSplitLayoutSnapshot>();
    }

    fn output_path() -> Option<&'static std::path::Path> {
        Some(std::path::Path::new("SessionWorkspaceLayoutSnapshot.ts"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionCanvasPaneSnapshot {
    pub panel_id: String,
    // i64 → `number` (not ts-rs's default `bigint`): the JSON wire shape is a
    // plain number, and the web side consumes it via `JSON.parse` as `number`.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub x: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub y: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub width: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub height: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub selected_panel_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelTitleSnapshot {
    pub panel_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelPinSnapshot {
    pub panel_id: String,
    pub is_pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelUnreadSnapshot {
    pub panel_id: String,
    pub is_unread: bool,
    /// Unix timestamp (seconds) when the panel became unread. Optional for
    /// backward compatibility with snapshots written before unread ordering was
    /// persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub unread_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelTerminalStartupSnapshot {
    pub panel_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_terminal_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_terminal_input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_terminal_environment: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct AgentLaunchCommandSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub launcher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub executable_path: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub environment: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionRestorableAgentSnapshot {
    pub kind: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub launch_command: Option<AgentLaunchCommandSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub resume_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fork_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelRestorableAgentSnapshot {
    pub panel_id: String,
    pub snapshot: SessionRestorableAgentSnapshot,
}

/// Canonical surface resume binding as stored by `surface.resume.set`
/// (Workspace.setSurfaceResumeBinding, Workspace.swift:4719-4727) and rendered
/// with explicit nulls by `surfaceResumeBindingPayload`
/// (ControlCommandCoordinator+Surface3.swift:147-167).
///
/// No `Eq`: `updated_at` is a double epoch timestamp
/// (Date().timeIntervalSince1970, TerminalController+ControlSurfaceContext4.swift:170-179).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionSurfaceResumeBindingSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub kind: Option<String>,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub checkpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub environment: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_resume: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub approval_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub approval_record_id: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: f64,
}

/// One `surface_id -> binding` row of the canonical per-workspace
/// `surfaceResumeBindingsByPanelId` map, following the keyed-list persistence
/// pattern of `SessionPanelRestorableAgentSnapshot`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionSurfaceResumeBindingRecordSnapshot {
    pub surface_id: String,
    pub binding: SessionSurfaceResumeBindingSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionGitBranchSnapshot {
    pub branch: String,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelGitBranchSnapshot {
    pub panel_id: String,
    pub branch: String,
    pub is_dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum SessionPullRequestStatusSnapshot {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelPullRequestSnapshot {
    pub panel_id: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub number: i64,
    pub label: String,
    pub url: String,
    pub status: SessionPullRequestStatusSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub branch: Option<String>,
    pub is_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelListeningPortsSnapshot {
    pub panel_id: String,
    #[serde(default)]
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelTtySnapshot {
    pub panel_id: String,
    pub tty: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "camelCase"))]
#[serde(rename_all = "camelCase")]
pub enum SessionPanelShellActivityStateSnapshot {
    Unknown,
    PromptIdle,
    CommandRunning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPanelShellActivitySnapshot {
    pub panel_id: String,
    pub state: SessionPanelShellActivityStateSnapshot,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceAgentPidSnapshot {
    pub key: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub pid: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceRemoteDaemonSnapshot {
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceRemoteProxySnapshot {
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceRemoteSnapshot {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub destination: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub local_proxy_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub persistent_daemon_slot: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_ssh_options: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub daemon: Option<SessionWorkspaceRemoteDaemonSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub proxy: Option<SessionWorkspaceRemoteProxySnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected_ports: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forwarded_ports: Vec<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicted_ports: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub active_terminal_sessions: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceSidebarProgressSnapshot {
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceSidebarStatusSnapshot {
    pub key: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub priority: Option<i64>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceSidebarMetadataSnapshot {
    pub key: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub priority: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub format: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceSidebarMetadataBlockSnapshot {
    pub key: String,
    pub markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub priority: Option<i64>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceSidebarLogEntrySnapshot {
    pub level: String,
    pub message: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_at: i64,
}

// No `Eq`: `layout` reaches `SessionSplitLayoutSnapshot::divider_position` (f64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_id: Option<String>,
    pub process_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_title_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub current_directory: Option<String>,
    /// Optional command used for the first terminal surface in this workspace.
    /// This mirrors macOS `initialTerminalCommand` and is required by
    /// restore/fork/external-open flows that launch something more specific than
    /// the default interactive shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_terminal_command: Option<String>,
    /// Optional text injected into the first terminal after its PTY opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_terminal_input: Option<String>,
    /// Environment overrides for the initial terminal process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_terminal_environment: Option<std::collections::BTreeMap<String, String>>,
    /// Persistent environment inherited by every terminal surface created in
    /// this workspace. This is distinct from the initial surface's overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_environment: Option<std::collections::BTreeMap<String, String>>,
    // `layout` has `#[serde(default)]` but NO `skip_serializing_if`, so it is
    // serialized as `"layout": null` when absent — keep it nullable, not optional.
    #[serde(default)]
    pub layout: Option<SessionWorkspaceLayoutSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout_mode: Option<String>,
    /// The panel id currently zoomed to fill the workspace split area, if any.
    /// Omitted when no pane zoom is active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub zoomed_panel_id: Option<String>,
    /// The panel whose pane currently owns keyboard focus. Canonical persists
    /// this independently from each pane's selected tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub focused_panel_id: Option<String>,
    /// Authoritative, ordered-by-pane surface records.  Old snapshots omit
    /// this field and are migrated from the layout/parallel metadata at load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surfaces: Option<Vec<SessionSurfaceSnapshot>>,
    /// Directory reports received before a matching remote surface arrives.
    /// These are consumed exactly once by lifecycle reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pending_remote_pwds: Option<Vec<SessionPendingRemotePwdSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub pending_surface_pwds: Option<Vec<SessionPendingSurfacePwdSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_titles: Option<Vec<SessionPanelTitleSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_pins: Option<Vec<SessionPanelPinSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_unreads: Option<Vec<SessionPanelUnreadSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub restorable_agent_snapshots: Option<Vec<SessionPanelRestorableAgentSnapshot>>,
    /// Per-terminal-surface resume bindings keyed by `surface_id`, mirroring
    /// canonical `Workspace.surfaceResumeBindingsByPanelId`
    /// (Workspace.swift:4719-4738 at pinned e1825d40d). Bindings persist in
    /// session snapshots (SessionPersistence.swift:1388-1413) and are rendered
    /// in `surface.list` terminal rows as `resume_binding`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surface_resume_bindings: Option<Vec<SessionSurfaceResumeBindingRecordSnapshot>>,
    /// Workspace-level git branch fallback used only when no panel reports a
    /// branch. Mirrors canonical `Workspace.gitBranch` as consumed by the
    /// sidebar badge projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub git_branch: Option<SessionGitBranchSnapshot>,
    /// Per-panel git branch state keyed by `panel_id`. The ordered panel ids
    /// still come from the layout tree, so this field only carries facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_git_branches: Option<Vec<SessionPanelGitBranchSnapshot>>,
    /// Per-panel pull request state keyed by `panel_id`. Stale rows are kept so
    /// the UI can render canonical secondary-stale badges while the poller
    /// refreshes inactive panels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_pull_requests: Option<Vec<SessionPanelPullRequestSnapshot>>,
    /// Remote workspace connection/proxy metadata. The macOS app owns the live
    /// broker; the Windows/Tauri port persists the same control-plane facts so
    /// browser panes can be rebound to a published local proxy endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub remote: Option<SessionWorkspaceRemoteSnapshot>,
    /// Optional sidebar progress reported by agents/CLI integrations. Mirrors
    /// canonical `set_progress`/`clear_progress` workspace metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar_progress: Option<SessionWorkspaceSidebarProgressSnapshot>,
    /// Sidebar status pills reported by CLI/agent integrations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar_status_entries: Option<Vec<SessionWorkspaceSidebarStatusSnapshot>>,
    /// Rich custom sidebar metadata entries reported by CLI/agent integrations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar_metadata_entries: Option<Vec<SessionWorkspaceSidebarMetadataSnapshot>>,
    /// Freeform custom sidebar markdown metadata blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar_metadata_blocks: Option<Vec<SessionWorkspaceSidebarMetadataBlockSnapshot>>,
    /// Recent sidebar log entries reported by CLI/agent integrations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar_log_entries: Option<Vec<SessionWorkspaceSidebarLogEntrySnapshot>>,
    /// Aggregate listening ports for this workspace, sorted and deduplicated.
    /// Mirrors canonical `Workspace.listeningPorts`; per-panel contributors
    /// live in `panel_listening_ports`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub listening_ports: Option<Vec<u16>>,
    /// Workspace-level listening ports owned by live agent process trees. Kept
    /// separate from `panel_listening_ports` so panel recomputes do not erase
    /// non-terminal agent facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub agent_listening_ports: Option<Vec<u16>>,
    /// Workspace-scoped agent root PIDs registered by shell/agent hooks.
    /// Scanning their descendant process trees feeds `agent_listening_ports`,
    /// which is then rendered by the existing sidebar port badge path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub agent_pids: Option<Vec<SessionWorkspaceAgentPidSnapshot>>,
    /// Per-panel listening ports keyed by `panel_id`, matching canonical
    /// `Workspace.surfaceListeningPorts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_listening_ports: Option<Vec<SessionPanelListeningPortsSnapshot>>,
    /// Per-panel terminal TTY names reported by shell/SSH integration. This is
    /// used by legacy hook/session mapping and debug terminal compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_ttys: Option<Vec<SessionPanelTtySnapshot>>,
    /// Per-panel shell activity state as reported by shell integration prompt
    /// markers. Mirrors canonical `Workspace.panelShellActivityStates`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_shell_activity: Option<Vec<SessionPanelShellActivitySnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub panel_terminal_startups: Option<Vec<SessionPanelTerminalStartupSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub canvas_panes: Option<Vec<SessionCanvasPaneSnapshot>>,
    // `group_id` mirrors macOS `SessionWorkspaceSnapshot.groupId: UUID? = nil`
    // (SessionPersistence.swift:1834): optional UUID rendered as a string, omitted
    // when nil (Swift's synthesized `encodeIfPresent`). Same wire shape as
    // `workspace_id`/`anchor_workspace_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub group_id: Option<String>,
    // `is_pinned` mirrors macOS `SessionWorkspaceSnapshot.isPinned: Bool`
    // (SessionPersistence.swift:1833). The live model emits it unconditionally, but
    // to keep every existing fixture byte-identical (none pin a workspace) we model
    // it as an omit-when-absent `Option<bool>`, matching the group snapshot's
    // `is_pinned` (session.rs) and the `surface_kind` precedent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub is_pinned: Option<bool>,
}

/// Persisted per-surface state. Runtime handles are intentionally absent: they
/// are rebound after restore using `generation` as the stale-callback fence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionSurfaceSnapshot {
    pub surface_id: String,
    pub pane_id: String,
    #[serde(default = "default_surface_generation")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub generation: u64,
    pub kind: SessionSurfaceKindSnapshot,
    #[serde(default)]
    pub metadata: SessionSurfaceMetadataSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub terminal_startup: Option<SessionSurfaceTerminalStartupSnapshot>,
}

fn default_surface_generation() -> u64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, tag = "type", rename_all = "snake_case")
)]
pub enum SessionSurfaceKindSnapshot {
    Terminal,
    Browser {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        profile: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        proxy_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        back_history: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        forward_history: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        omnibar_visible: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        focus_mode_active: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        developer_tools_visible: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        developer_tools_panel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
        page_zoom: Option<f64>,
    },
    AgentSession {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        renderer: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        working_directory: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        lifecycle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        restorable_agent: Option<Box<SessionRestorableAgentSnapshot>>,
    },
    Markdown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        path: Option<String>,
    },
    File {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        path: Option<String>,
    },
    Diff {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        request_path: Option<String>,
    },
    ProjectSidebar,
    RightSidebarTool,
    RemoteTerminal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional))]
        remote_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional, type = "unknown"))]
        remote_context: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
        arrival_generation: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPendingRemotePwdSnapshot {
    pub remote_session_id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPendingSurfacePwdSnapshot {
    pub surface_id: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub generation: u64,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionSurfaceMetadataSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unread: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub unread_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub reported_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub directory_provenance: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionSurfaceTerminalStartupSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub initial_input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub environment: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tmux_start_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub remote_pty_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub resume_binding: Option<Box<SessionRestorableAgentSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWorkspaceGroupSnapshot {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub anchor_workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub anchor_member_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub is_pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub custom_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon_symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionTabManagerSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub selected_workspace_index: Option<i64>,
    #[serde(default)]
    pub workspaces: Vec<SessionWorkspaceSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_groups: Option<Vec<SessionWorkspaceGroupSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionWindowSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub window_id: Option<String>,
    /// Stable selected workspace identity, retained even when the legacy index
    /// is stale during restore/routing reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub selected_workspace_id: Option<String>,
    /// The window-scoped Dock uses the same pane/surface persistence schema as
    /// workspaces while remaining a distinct lifecycle container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dock: Option<SessionDockSnapshot>,
    pub tab_manager: SessionTabManagerSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionDockSnapshot {
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<SessionWorkspaceLayoutSnapshot>,
    #[serde(default)]
    pub surfaces: Vec<SessionSurfaceSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub focused_surface_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct AppSessionSnapshot {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub version: i64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_at: i64,
    #[serde(default)]
    pub windows: Vec<SessionWindowSnapshot>,
}

pub const SESSION_SNAPSHOT_SCHEMA_VERSION: i64 = 1;

pub fn decode_session(bytes: &[u8]) -> Result<AppSessionSnapshot, serde_json::Error> {
    serde_json::from_slice(bytes)
}

pub fn encode_session(snapshot: &AppSessionSnapshot) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(snapshot)
}

// ---------------------------------------------------------------------------
// TypeScript binding export (feature = "ts")
// ---------------------------------------------------------------------------
//
// ts-rs derives `TS` (and thus an `export_bindings_*` test) for every struct/enum
// marked `#[ts(export)]` above. Running `cargo test -p cmux-core --features ts`
// with `TS_RS_EXPORT_DIR` set writes one `<TypeName>.ts` per type into that dir.
//
// `SessionWorkspaceLayoutSnapshot` has hand-written serde impls (the
// `{type, pane|split}` adjacently-tagged-with-variant-named-content wire shape),
// which ts-rs cannot express via derive. The test below writes that one type's
// `.ts` by hand so the shape stays byte-exact with the Rust serializer, and emits
// a barrel `index.ts` re-exporting everything. See `REPORT_WS1.md`.
#[cfg(all(test, feature = "ts"))]
mod ts_export {
    use super::*;
    use std::path::PathBuf;
    use ts_rs::TS;

    fn export_dir() -> PathBuf {
        PathBuf::from(
            std::env::var("TS_RS_EXPORT_DIR")
                .expect("set TS_RS_EXPORT_DIR to the bindings output directory"),
        )
    }

    /// Hand-written binding for the manually-serialized tagged union, plus the
    /// barrel file. Lives here (not in ts-rs derive) because the wire shape uses
    /// a variant-named content key (`pane`/`split`) that derive can't emit.
    #[test]
    fn export_manual_bindings() {
        let dir = export_dir();
        std::fs::create_dir_all(&dir).expect("create export dir");

        // The tagged union: `{ type: "pane", pane: ... } | { type: "split", split: ... }`.
        // `first`/`second` of the split node are this same type (recursive).
        //
        // This MUST be byte-identical to what ts-rs emits for this type via its
        // `TS` impl `decl()` (the single-line `inline()` form). ts-rs also writes
        // this same file when it exports a dependent type, so under parallel
        // `cargo test` the two writers race; if they disagree the output becomes
        // machine-dependent (whichever test finishes last wins) and the
        // core-types drift check fails on some runners but not others. Keeping
        // the bytes identical makes the race outcome irrelevant.
        let union = "\
// This file was generated by cmux-core (cargo test --features ts). Do not edit.
import type { SessionPaneLayoutSnapshot } from \"./SessionPaneLayoutSnapshot\";
import type { SessionSplitLayoutSnapshot } from \"./SessionSplitLayoutSnapshot\";

export type SessionWorkspaceLayoutSnapshot = { type: \"pane\", pane: SessionPaneLayoutSnapshot } | { type: \"split\", split: SessionSplitLayoutSnapshot };
";
        std::fs::write(dir.join("SessionWorkspaceLayoutSnapshot.ts"), union)
            .expect("write union binding");

        // Barrel re-exporting every generated type for ergonomic imports.
        let barrel = "\
// This file was generated by cmux-core (cargo test --features ts). Do not edit.
export type { AppSessionSnapshot } from \"./AppSessionSnapshot\";
export type { AgentLaunchCommandSnapshot } from \"./AgentLaunchCommandSnapshot\";
export type { SessionCanvasPaneSnapshot } from \"./SessionCanvasPaneSnapshot\";
export type { SessionPanelListeningPortsSnapshot } from \"./SessionPanelListeningPortsSnapshot\";
export type { SessionPanelRestorableAgentSnapshot } from \"./SessionPanelRestorableAgentSnapshot\";
export type { SessionPanelShellActivitySnapshot } from \"./SessionPanelShellActivitySnapshot\";
export type { SessionPanelShellActivityStateSnapshot } from \"./SessionPanelShellActivityStateSnapshot\";
export type { SessionPanelTerminalStartupSnapshot } from \"./SessionPanelTerminalStartupSnapshot\";
export type { SessionPanelTtySnapshot } from \"./SessionPanelTtySnapshot\";
export type { SessionPaneLayoutSnapshot } from \"./SessionPaneLayoutSnapshot\";
export type { SessionSplitLayoutSnapshot } from \"./SessionSplitLayoutSnapshot\";
export type { SessionRestorableAgentSnapshot } from \"./SessionRestorableAgentSnapshot\";
export type { SessionSplitOrientation } from \"./SessionSplitOrientation\";
export type { SessionTabManagerSnapshot } from \"./SessionTabManagerSnapshot\";
export type { SessionWindowSnapshot } from \"./SessionWindowSnapshot\";
export type { SessionWorkspaceGroupSnapshot } from \"./SessionWorkspaceGroupSnapshot\";
export type { SessionWorkspaceLayoutSnapshot } from \"./SessionWorkspaceLayoutSnapshot\";
export type { SessionWorkspaceAgentPidSnapshot } from \"./SessionWorkspaceAgentPidSnapshot\";
export type { SessionWorkspaceSidebarLogEntrySnapshot } from \"./SessionWorkspaceSidebarLogEntrySnapshot\";
export type { SessionWorkspaceSidebarMetadataBlockSnapshot } from \"./SessionWorkspaceSidebarMetadataBlockSnapshot\";
export type { SessionWorkspaceSidebarMetadataSnapshot } from \"./SessionWorkspaceSidebarMetadataSnapshot\";
export type { SessionWorkspaceSidebarProgressSnapshot } from \"./SessionWorkspaceSidebarProgressSnapshot\";
export type { SessionWorkspaceSidebarStatusSnapshot } from \"./SessionWorkspaceSidebarStatusSnapshot\";
export type { SessionWorkspaceSnapshot } from \"./SessionWorkspaceSnapshot\";

// --- WS3 extension point -------------------------------------------------
// The keyboard `Action` id catalog is owned by WS3 (crates/cmux-core/src/
// shortcuts.rs). When that lands, add its `#[ts(export)]` type here, e.g.:
//   export type { ActionId } from \"./ActionId\";
// No other file needs to change; this barrel is the single import surface.
";
        std::fs::write(dir.join("index.ts"), barrel).expect("write barrel");

        // Touch the derived types so a missing derive fails the export run.
        let _ = SessionPaneLayoutSnapshot::name();
        let _ = SessionSplitLayoutSnapshot::name();
        let _ = AgentLaunchCommandSnapshot::name();
        let _ = SessionRestorableAgentSnapshot::name();
        let _ = SessionPanelTerminalStartupSnapshot::name();
        let _ = SessionPanelListeningPortsSnapshot::name();
        let _ = SessionPanelTtySnapshot::name();
        let _ = SessionPanelShellActivitySnapshot::name();
        let _ = SessionPanelShellActivityStateSnapshot::name();
        let _ = SessionWorkspaceAgentPidSnapshot::name();
        let _ = SessionWorkspaceSidebarLogEntrySnapshot::name();
        let _ = SessionWorkspaceSidebarMetadataBlockSnapshot::name();
        let _ = SessionWorkspaceSidebarMetadataSnapshot::name();
        let _ = SessionWorkspaceSidebarProgressSnapshot::name();
        let _ = SessionWorkspaceSidebarStatusSnapshot::name();
        let _ = AppSessionSnapshot::name();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_union_round_trips() {
        let snapshot = SessionWorkspaceLayoutSnapshot::Split(SessionSplitLayoutSnapshot {
            split_id: None,
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                SessionPaneLayoutSnapshot {
                    pane_id: None,
                    panel_ids: vec!["A".into()],
                    selected_panel_id: Some("A".into()),
                    surface_kind: None,
                    markdown_file_path: None,
                    file_path: None,
                    diff_viewer_token: None,
                    diff_viewer_request_path: None,
                    browser_url: None,
                    browser_proxy_url: None,
                    browser_back_history: None,
                    browser_forward_history: None,
                    browser_omnibar_visible: None,
                    browser_focus_mode_active: None,
                    browser_developer_tools_visible: None,
                    browser_developer_tools_panel: None,
                    browser_page_zoom: None,
                },
            )),
            second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                SessionPaneLayoutSnapshot {
                    pane_id: None,
                    panel_ids: vec!["B".into()],
                    selected_panel_id: Some("B".into()),
                    surface_kind: None,
                    markdown_file_path: None,
                    file_path: None,
                    diff_viewer_token: None,
                    diff_viewer_request_path: None,
                    browser_url: None,
                    browser_proxy_url: None,
                    browser_back_history: None,
                    browser_forward_history: None,
                    browser_omnibar_visible: None,
                    browser_focus_mode_active: None,
                    browser_developer_tools_visible: None,
                    browser_developer_tools_panel: None,
                    browser_page_zoom: None,
                },
            )),
        });

        let json = serde_json::to_value(&snapshot).expect("serialize");
        assert_eq!(json["type"], "split");
        assert!(json.get("split").is_some());

        let decoded: SessionWorkspaceLayoutSnapshot =
            serde_json::from_value(json).expect("deserialize");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn workspace_snapshot_defaults_group_and_pinned_to_none() {
        let snapshot = SessionWorkspaceSnapshot::default();
        assert_eq!(snapshot.custom_description, None);
        assert_eq!(snapshot.custom_color, None);
        assert_eq!(snapshot.group_id, None);
        assert_eq!(snapshot.is_pinned, None);
        assert_eq!(snapshot.zoomed_panel_id, None);
        assert_eq!(snapshot.panel_titles, None);
        assert_eq!(snapshot.panel_pins, None);
        assert_eq!(snapshot.panel_unreads, None);
        assert_eq!(snapshot.restorable_agent_snapshots, None);
        assert_eq!(snapshot.git_branch, None);
        assert_eq!(snapshot.panel_git_branches, None);
        assert_eq!(snapshot.panel_pull_requests, None);
        assert_eq!(snapshot.sidebar_progress, None);
        assert_eq!(snapshot.sidebar_status_entries, None);
        assert_eq!(snapshot.sidebar_metadata_entries, None);
        assert_eq!(snapshot.sidebar_metadata_blocks, None);
        assert_eq!(snapshot.sidebar_log_entries, None);
        assert_eq!(snapshot.listening_ports, None);
        assert_eq!(snapshot.agent_listening_ports, None);
        assert_eq!(snapshot.agent_pids, None);
        assert_eq!(snapshot.panel_listening_ports, None);
        assert_eq!(snapshot.panel_ttys, None);
        assert_eq!(snapshot.panel_shell_activity, None);
        assert_eq!(snapshot.panel_terminal_startups, None);
        assert_eq!(snapshot.initial_terminal_command, None);
        assert_eq!(snapshot.initial_terminal_input, None);
        assert_eq!(snapshot.initial_terminal_environment, None);
    }

    #[test]
    fn workspace_snapshot_omits_description_group_and_pinned_when_none() {
        // Parity oracle: mirrors Swift's `encodeIfPresent` for `groupId` and keeps
        // existing (unpinned, ungrouped) fixtures byte-identical — neither key may
        // appear in the JSON when absent.
        let snapshot = SessionWorkspaceSnapshot {
            process_title: "zsh".into(),
            ..Default::default()
        };
        let json = serde_json::to_value(&snapshot).expect("serialize");
        let object = json.as_object().expect("object");
        assert!(
            !object.contains_key("custom_description"),
            "custom_description must be omitted when None"
        );
        assert!(
            !object.contains_key("custom_color"),
            "custom_color must be omitted when None"
        );
        assert!(
            !object.contains_key("group_id"),
            "group_id must be omitted when None"
        );
        assert!(
            !object.contains_key("is_pinned"),
            "is_pinned must be omitted when None"
        );
        assert!(
            !object.contains_key("zoomed_panel_id"),
            "zoomed_panel_id must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_titles"),
            "panel_titles must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_pins"),
            "panel_pins must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_unreads"),
            "panel_unreads must be omitted when None"
        );
        assert!(
            !object.contains_key("restorable_agent_snapshots"),
            "restorable_agent_snapshots must be omitted when None"
        );
        assert!(
            !object.contains_key("git_branch"),
            "git_branch must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_git_branches"),
            "panel_git_branches must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_pull_requests"),
            "panel_pull_requests must be omitted when None"
        );
        assert!(
            !object.contains_key("sidebar_progress"),
            "sidebar_progress must be omitted when None"
        );
        assert!(
            !object.contains_key("sidebar_status_entries"),
            "sidebar_status_entries must be omitted when None"
        );
        assert!(
            !object.contains_key("sidebar_metadata_entries"),
            "sidebar_metadata_entries must be omitted when None"
        );
        assert!(
            !object.contains_key("sidebar_metadata_blocks"),
            "sidebar_metadata_blocks must be omitted when None"
        );
        assert!(
            !object.contains_key("sidebar_log_entries"),
            "sidebar_log_entries must be omitted when None"
        );
        assert!(
            !object.contains_key("listening_ports"),
            "listening_ports must be omitted when None"
        );
        assert!(
            !object.contains_key("agent_listening_ports"),
            "agent_listening_ports must be omitted when None"
        );
        assert!(
            !object.contains_key("agent_pids"),
            "agent_pids must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_listening_ports"),
            "panel_listening_ports must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_ttys"),
            "panel_ttys must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_shell_activity"),
            "panel_shell_activity must be omitted when None"
        );
        assert!(
            !object.contains_key("panel_terminal_startups"),
            "panel_terminal_startups must be omitted when None"
        );
        assert!(
            !object.contains_key("initial_terminal_command"),
            "initial_terminal_command must be omitted when None"
        );
        assert!(
            !object.contains_key("initial_terminal_input"),
            "initial_terminal_input must be omitted when None"
        );
        assert!(
            !object.contains_key("initial_terminal_environment"),
            "initial_terminal_environment must be omitted when None"
        );
    }

    #[test]
    fn workspace_snapshot_round_trips_group_and_pinned() {
        let snapshot = SessionWorkspaceSnapshot {
            process_title: "zsh".into(),
            custom_description: Some("alpha\nbeta".into()),
            custom_color: Some("#C0392B".into()),
            group_id: Some("11111111-2222-3333-4444-555555555555".into()),
            is_pinned: Some(true),
            zoomed_panel_id: Some("surface-1".into()),
            panel_titles: Some(vec![SessionPanelTitleSnapshot {
                panel_id: "surface-1".into(),
                custom_title: Some("api logs".into()),
            }]),
            panel_pins: Some(vec![SessionPanelPinSnapshot {
                panel_id: "surface-1".into(),
                is_pinned: true,
            }]),
            panel_unreads: Some(vec![SessionPanelUnreadSnapshot {
                panel_id: "surface-1".into(),
                is_unread: true,
                unread_at: Some(42),
            }]),
            restorable_agent_snapshots: Some(vec![SessionPanelRestorableAgentSnapshot {
                panel_id: "surface-1".into(),
                snapshot: SessionRestorableAgentSnapshot {
                    kind: "codex".into(),
                    session_id: "session-1".into(),
                    working_directory: Some("C:/repo".into()),
                    launch_command: Some(AgentLaunchCommandSnapshot {
                        launcher: None,
                        executable_path: Some("codex".into()),
                        arguments: vec!["codex".into()],
                        working_directory: Some("C:/repo".into()),
                        environment: Some(std::collections::BTreeMap::from([(
                            "CODEX_HOME".into(),
                            "C:/codex".into(),
                        )])),
                        source: Some("provider.start".into()),
                    }),
                    resume_command: Some("codex resume session-1".into()),
                    fork_command: Some("codex resume session-1 --fork".into()),
                },
            }]),
            panel_terminal_startups: Some(vec![SessionPanelTerminalStartupSnapshot {
                panel_id: "surface-1".into(),
                initial_terminal_command: None,
                initial_terminal_input: Some("codex fork session-1\r\n".into()),
                initial_terminal_environment: Some(std::collections::BTreeMap::from([(
                    "CMUX_AGENT_FORK".into(),
                    "1".into(),
                )])),
            }]),
            listening_ports: Some(vec![3000, 5173]),
            agent_listening_ports: Some(vec![7000]),
            agent_pids: Some(vec![SessionWorkspaceAgentPidSnapshot {
                key: "codex.session-1".into(),
                pid: 1234,
                updated_at: 99,
            }]),
            panel_listening_ports: Some(vec![SessionPanelListeningPortsSnapshot {
                panel_id: "surface-1".into(),
                ports: vec![5173, 3000],
            }]),
            panel_ttys: Some(vec![SessionPanelTtySnapshot {
                panel_id: "surface-1".into(),
                tty: "ttys004".into(),
                updated_at: 101,
            }]),
            initial_terminal_command: Some("ssh".into()),
            initial_terminal_input: Some("echo ready\r".into()),
            initial_terminal_environment: Some(std::collections::BTreeMap::from([(
                "CMUX_FORK".into(),
                "1".into(),
            )])),
            ..Default::default()
        };
        let json = serde_json::to_value(&snapshot).expect("serialize");
        assert_eq!(json["custom_description"], "alpha\nbeta");
        assert_eq!(json["custom_color"], "#C0392B");
        assert_eq!(json["group_id"], "11111111-2222-3333-4444-555555555555");
        assert_eq!(json["is_pinned"], true);
        assert_eq!(json["zoomed_panel_id"], "surface-1");
        assert_eq!(json["panel_titles"][0]["panel_id"], "surface-1");
        assert_eq!(json["panel_titles"][0]["custom_title"], "api logs");
        assert_eq!(json["panel_pins"][0]["panel_id"], "surface-1");
        assert_eq!(json["panel_pins"][0]["is_pinned"], true);
        assert_eq!(json["panel_unreads"][0]["panel_id"], "surface-1");
        assert_eq!(json["panel_unreads"][0]["is_unread"], true);
        assert_eq!(json["panel_unreads"][0]["unread_at"], 42);
        assert_eq!(
            json["restorable_agent_snapshots"][0]["panel_id"],
            "surface-1"
        );
        assert_eq!(
            json["restorable_agent_snapshots"][0]["snapshot"]["kind"],
            "codex"
        );
        assert_eq!(
            json["restorable_agent_snapshots"][0]["snapshot"]["session_id"],
            "session-1"
        );
        assert_eq!(
            json["restorable_agent_snapshots"][0]["snapshot"]["launch_command"]["environment"]
                ["CODEX_HOME"],
            "C:/codex"
        );
        assert_eq!(
            json["restorable_agent_snapshots"][0]["snapshot"]["fork_command"],
            "codex resume session-1 --fork"
        );
        assert_eq!(json["panel_terminal_startups"][0]["panel_id"], "surface-1");
        assert_eq!(
            json["panel_terminal_startups"][0]["initial_terminal_input"],
            "codex fork session-1\r\n"
        );
        assert_eq!(
            json["panel_terminal_startups"][0]["initial_terminal_environment"]["CMUX_AGENT_FORK"],
            "1"
        );
        assert_eq!(json["listening_ports"], serde_json::json!([3000, 5173]));
        assert_eq!(json["agent_listening_ports"], serde_json::json!([7000]));
        assert_eq!(
            json["agent_pids"][0],
            serde_json::json!({
                "key": "codex.session-1",
                "pid": 1234,
                "updated_at": 99,
            })
        );
        assert_eq!(
            json["panel_listening_ports"][0],
            serde_json::json!({
                "panel_id": "surface-1",
                "ports": [5173, 3000],
            })
        );
        assert_eq!(
            json["panel_ttys"][0],
            serde_json::json!({
                "panel_id": "surface-1",
                "tty": "ttys004",
                "updated_at": 101,
            })
        );
        assert_eq!(json["initial_terminal_command"], "ssh");
        assert_eq!(json["initial_terminal_input"], "echo ready\r");
        assert_eq!(json["initial_terminal_environment"]["CMUX_FORK"], "1");

        let decoded: SessionWorkspaceSnapshot = serde_json::from_value(json).expect("deserialize");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn workspace_snapshot_decodes_absent_description_group_and_pinned_as_none() {
        // Raw JSON lacking both keys (i.e. every existing fixture) must decode to
        // `None`/`None`, proving wire back-compatibility.
        let raw = serde_json::json!({ "process_title": "zsh", "layout": null });
        let decoded: SessionWorkspaceSnapshot = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(decoded.custom_description, None);
        assert_eq!(decoded.custom_color, None);
        assert_eq!(decoded.group_id, None);
        assert_eq!(decoded.is_pinned, None);
        assert_eq!(decoded.panel_titles, None);
        assert_eq!(decoded.panel_pins, None);
        assert_eq!(decoded.panel_unreads, None);
        assert_eq!(decoded.restorable_agent_snapshots, None);
        assert_eq!(decoded.sidebar_progress, None);
        assert_eq!(decoded.sidebar_status_entries, None);
        assert_eq!(decoded.sidebar_metadata_entries, None);
        assert_eq!(decoded.sidebar_metadata_blocks, None);
        assert_eq!(decoded.sidebar_log_entries, None);
        assert_eq!(decoded.panel_terminal_startups, None);
        assert_eq!(decoded.listening_ports, None);
        assert_eq!(decoded.agent_listening_ports, None);
        assert_eq!(decoded.agent_pids, None);
        assert_eq!(decoded.panel_listening_ports, None);
        assert_eq!(decoded.panel_ttys, None);
        assert_eq!(decoded.initial_terminal_command, None);
        assert_eq!(decoded.initial_terminal_input, None);
        assert_eq!(decoded.initial_terminal_environment, None);
    }

    #[test]
    fn panel_unread_snapshot_decodes_absent_unread_at_as_none() {
        let raw = serde_json::json!({ "panel_id": "surface-1", "is_unread": true });
        let decoded: SessionPanelUnreadSnapshot = serde_json::from_value(raw).expect("deserialize");
        assert_eq!(decoded.unread_at, None);
    }
}
