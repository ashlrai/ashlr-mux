use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPaneLayoutSnapshot {
    pub panel_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_panel_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionSplitOrientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSplitLayoutSnapshot {
    pub orientation: SessionSplitOrientation,
    pub divider_position: f64,
    pub first: Box<SessionWorkspaceLayoutSnapshot>,
    pub second: Box<SessionWorkspaceLayoutSnapshot>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCanvasPaneSnapshot {
    pub panel_id: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_panel_id: Option<String>,
}

// No `Eq`: `layout` reaches `SessionSplitLayoutSnapshot::divider_position` (f64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionWorkspaceSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub process_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_directory: Option<String>,
    #[serde(default)]
    pub layout: Option<SessionWorkspaceLayoutSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas_panes: Option<Vec<SessionCanvasPaneSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionWorkspaceGroupSnapshot {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_member_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_symbol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionTabManagerSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_workspace_index: Option<i64>,
    #[serde(default)]
    pub workspaces: Vec<SessionWorkspaceSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_groups: Option<Vec<SessionWorkspaceGroupSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionWindowSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    pub tab_manager: SessionTabManagerSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppSessionSnapshot {
    pub version: i64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_union_round_trips() {
        let snapshot = SessionWorkspaceLayoutSnapshot::Split(SessionSplitLayoutSnapshot {
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(SessionPaneLayoutSnapshot {
                panel_ids: vec!["A".into()],
                selected_panel_id: Some("A".into()),
            })),
            second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(SessionPaneLayoutSnapshot {
                panel_ids: vec!["B".into()],
                selected_panel_id: Some("B".into()),
            })),
        });

        let json = serde_json::to_value(&snapshot).expect("serialize");
        assert_eq!(json["type"], "split");
        assert!(json.get("split").is_some());

        let decoded: SessionWorkspaceLayoutSnapshot =
            serde_json::from_value(json).expect("deserialize");
        assert_eq!(decoded, snapshot);
    }
}
