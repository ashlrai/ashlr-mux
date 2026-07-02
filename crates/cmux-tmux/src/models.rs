//! Plain data models for the remote-tmux control-mode family.
//!
//! Ported from the canonical macOS Swift sources:
//! `RemoteTmuxLayoutNode.swift`, `RemoteTmuxLayoutContent.swift`,
//! `RemoteTmuxSession.swift`, and `RemoteTmuxControlMessage.swift`.

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The content of a ``RemoteTmuxLayoutNode``: a leaf pane or a split.
///
/// Ported from `RemoteTmuxLayoutContent.swift`.
///
// DIVERGENCE: Swift stores geometry/pane ids as `Int` (64-bit on macOS). We use
// `i64` to match that width exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteTmuxLayoutContent {
    /// A leaf pane, identified by its numeric tmux pane id (the `%N` without the
    /// leading `%`).
    Pane(i64),

    /// A left-to-right split of child nodes.
    Horizontal(Vec<RemoteTmuxLayoutNode>),

    /// A top-to-bottom split of child nodes.
    Vertical(Vec<RemoteTmuxLayoutNode>),
}

/// A node in a tmux window's pane-layout tree, parsed from a tmux
/// `#{window_layout}` / `%layout-change` string by
/// [`crate::raw_layout::parse`].
///
/// Each node carries its geometry (`width`/`height`/`x`/`y`, in terminal cells)
/// and is either a leaf pane or a split containing child nodes, mirroring tmux's
/// layout semantics: `horizontal` children are arranged left→right, `vertical`
/// children top→bottom.
///
/// The JSON shape (one of `pane`/`horizontal`/`vertical` is present):
/// ```json
/// { "width": 80, "height": 24, "x": 0, "y": 0,
///   "horizontal": [ { "width": 40, "height": 24, "x": 0, "y": 0, "pane": 1 },
///                   { "width": 40, "height": 24, "x": 40, "y": 0, "pane": 2 } ] }
/// ```
///
/// Ported from `RemoteTmuxLayoutNode.swift`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteTmuxLayoutNode {
    /// Width of the node in terminal cells.
    pub width: i64,
    /// Height of the node in terminal cells.
    pub height: i64,
    /// X offset from the window's top-left, in cells.
    pub x: i64,
    /// Y offset from the window's top-left, in cells.
    pub y: i64,
    /// The node's content: a leaf pane or a split.
    pub content: RemoteTmuxLayoutContent,
}

impl RemoteTmuxLayoutNode {
    pub fn new(width: i64, height: i64, x: i64, y: i64, content: RemoteTmuxLayoutContent) -> Self {
        Self {
            width,
            height,
            x,
            y,
            content,
        }
    }

    /// All pane ids in this subtree, in depth-first left-to-right order — the
    /// natural order to create matching cmux splits.
    ///
    /// Mirrors Swift `paneIDsInOrder`.
    pub fn pane_ids_in_order(&self) -> Vec<i64> {
        match &self.content {
            RemoteTmuxLayoutContent::Pane(id) => vec![*id],
            RemoteTmuxLayoutContent::Horizontal(children)
            | RemoteTmuxLayoutContent::Vertical(children) => {
                children.iter().flat_map(|c| c.pane_ids_in_order()).collect()
            }
        }
    }
}

// The Swift `Codable` conformance flattens `content` into sibling keys
// (`pane`/`horizontal`/`vertical`) rather than a nested object. Reproduce that
// exact JSON shape here so persisted layouts round-trip byte-compatibly.
impl Serialize for RemoteTmuxLayoutNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("width", &self.width)?;
        map.serialize_entry("height", &self.height)?;
        map.serialize_entry("x", &self.x)?;
        map.serialize_entry("y", &self.y)?;
        match &self.content {
            RemoteTmuxLayoutContent::Pane(id) => map.serialize_entry("pane", id)?,
            RemoteTmuxLayoutContent::Horizontal(children) => {
                map.serialize_entry("horizontal", children)?
            }
            RemoteTmuxLayoutContent::Vertical(children) => {
                map.serialize_entry("vertical", children)?
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RemoteTmuxLayoutNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;

        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = RemoteTmuxLayoutNode;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a tmux layout node object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut width: Option<i64> = None;
                let mut height: Option<i64> = None;
                let mut x: Option<i64> = None;
                let mut y: Option<i64> = None;
                let mut pane: Option<i64> = None;
                let mut horizontal: Option<Vec<RemoteTmuxLayoutNode>> = None;
                let mut vertical: Option<Vec<RemoteTmuxLayoutNode>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "width" => width = Some(map.next_value()?),
                        "height" => height = Some(map.next_value()?),
                        "x" => x = Some(map.next_value()?),
                        "y" => y = Some(map.next_value()?),
                        "pane" => pane = Some(map.next_value()?),
                        "horizontal" => horizontal = Some(map.next_value()?),
                        "vertical" => vertical = Some(map.next_value()?),
                        // Unknown keys are ignored (Swift's keyed container also
                        // tolerates extra keys).
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                let width = width.ok_or_else(|| de::Error::missing_field("width"))?;
                let height = height.ok_or_else(|| de::Error::missing_field("height"))?;
                let x = x.ok_or_else(|| de::Error::missing_field("x"))?;
                let y = y.ok_or_else(|| de::Error::missing_field("y"))?;

                let content = if let Some(id) = pane {
                    RemoteTmuxLayoutContent::Pane(id)
                } else if let Some(children) = horizontal {
                    RemoteTmuxLayoutContent::Horizontal(children)
                } else if let Some(children) = vertical {
                    RemoteTmuxLayoutContent::Vertical(children)
                } else {
                    return Err(de::Error::custom(
                        "layout node missing pane/horizontal/vertical",
                    ));
                };

                Ok(RemoteTmuxLayoutNode {
                    width,
                    height,
                    x,
                    y,
                    content,
                })
            }
        }

        deserializer.deserialize_map(NodeVisitor)
    }
}

/// A tmux session discovered on a remote host.
///
/// Mirrors the fields cmux requests from `tmux list-sessions`. The `id` is
/// tmux's native session id (e.g. `$2`), which is stable for the lifetime of
/// the remote tmux server and is what cmux keys its sidebar workspace on.
///
/// Ported from `RemoteTmuxSession.swift`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTmuxSession {
    /// tmux's native session id, e.g. `$2`.
    pub id: String,

    /// The session name, e.g. `main`.
    pub name: String,

    /// Number of windows in the session.
    #[serde(rename = "windowCount")]
    pub window_count: i64,

    /// Whether any client is currently attached to the session.
    pub attached: bool,

    /// Session creation time as a Unix timestamp, when reported by tmux.
    #[serde(rename = "createdUnix")]
    pub created_unix: Option<i64>,
}

impl RemoteTmuxSession {
    pub fn new(
        id: String,
        name: String,
        window_count: i64,
        attached: bool,
        created_unix: Option<i64>,
    ) -> Self {
        Self {
            id,
            name,
            window_count,
            attached,
            created_unix,
        }
    }
}

/// A single parsed message from a remote tmux control-mode (`tmux -CC`) stream.
///
/// Produced by [`crate::control_stream::RemoteTmuxControlStreamParser`]. Command
/// responses (the output between a `%begin`/`%end` pair) are coalesced into a
/// single [`RemoteTmuxControlMessage::CommandResult`] carrying the lines in
/// between; everything else is an out-of-band notification.
///
/// Ported from `RemoteTmuxControlMessage.swift`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteTmuxControlMessage {
    /// The `ESC P 1000 p` handshake that opens control mode.
    Enter,

    /// Control mode ended (`%exit`), with tmux's optional reason.
    Exit { reason: Option<String> },

    /// `%output %<pane> <data>` — terminal output for a pane. `data` is already
    /// octal-unescaped to its raw bytes, ready to feed into a display surface.
    ///
    // DIVERGENCE: Swift carries the payload as Foundation `Data`; the faithful
    // Rust equivalent is `Vec<u8>` (kept byte-oriented, never round-tripped
    // through `String`, exactly as the Swift parser requires).
    Output { pane_id: i64, data: Vec<u8> },

    /// `%session-changed $<id> <name>` — the attached session changed.
    SessionChanged { session_id: i64, name: String },

    /// `%session-renamed [session-id] <name>` — the current session was renamed.
    /// tmux emits this for `rename-session` — distinct from `%session-changed`
    /// (which fires when the attached session switches). `name` preserves the
    /// documented name-only interpretation; `id_bearing_name` is the alternative
    /// interpretation when the first field looks like a tmux session id.
    SessionRenamed {
        session_id: Option<i64>,
        name: String,
        id_bearing_name: Option<String>,
    },

    /// `%sessions-changed` — the set of sessions changed (re-list to refresh).
    SessionsChanged,

    /// `%window-add @<id>` — a window was added to the attached session.
    WindowAdd { window_id: i64 },

    /// `%window-close @<id>` / `%unlinked-window-close @<id>` — a window closed.
    WindowClose { window_id: i64 },

    /// `%window-renamed @<id> <name>` — a window was renamed.
    WindowRenamed { window_id: i64, name: String },

    /// `%layout-change @<id> <layout> …` — a window's pane layout changed.
    /// `layout` is the raw tmux layout string (parse with
    /// [`crate::raw_layout::parse`]).
    LayoutChange { window_id: i64, layout: String },

    /// `%window-pane-changed @<id> %<pane>` — the active pane in a window changed.
    WindowPaneChanged { window_id: i64, pane_id: i64 },

    /// `%session-window-changed $<sid> @<wid>` — the active window in a session
    /// changed.
    SessionWindowChanged { session_id: i64, window_id: i64 },

    /// `%subscription-changed <name> … : <value>` — a `refresh-client -B`
    /// subscription's value changed. cmux subscribes per-pane
    /// `pane_current_path` for live working-directory tracking. Parsed leniently:
    /// `name` is the first field and `value` is everything after the first ` : `
    /// separator, so the version-variable middle fields
    /// (session/window/pane/flags) are ignored.
    SubscriptionChanged { name: String, value: String },

    /// The coalesced output of one command block (`%begin`…`%end`/`%error`).
    CommandResult {
        command_number: i64,
        lines: Vec<String>,
        is_error: bool,
    },

    /// The control stream became unsafe to keep parsing, for example because an
    /// unterminated line or command block exceeded the parser's memory budget.
    StreamError(String),

    /// A recognized notification cmux does not act on (kept for diagnostics).
    IgnoredNotification(String),

    /// A line that could not be classified.
    Unparsed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_ids_in_order_depth_first_left_to_right() {
        // 120x40 horizontal split of [pane 4, vertical[pane 5, pane 8]]
        let node = RemoteTmuxLayoutNode::new(
            120,
            40,
            0,
            0,
            RemoteTmuxLayoutContent::Horizontal(vec![
                RemoteTmuxLayoutNode::new(60, 40, 0, 0, RemoteTmuxLayoutContent::Pane(4)),
                RemoteTmuxLayoutNode::new(
                    59,
                    40,
                    61,
                    0,
                    RemoteTmuxLayoutContent::Vertical(vec![
                        RemoteTmuxLayoutNode::new(59, 20, 61, 0, RemoteTmuxLayoutContent::Pane(5)),
                        RemoteTmuxLayoutNode::new(59, 19, 61, 21, RemoteTmuxLayoutContent::Pane(8)),
                    ]),
                ),
            ]),
        );
        assert_eq!(node.pane_ids_in_order(), vec![4, 5, 8]);
    }

    #[test]
    fn layout_node_json_shape_roundtrips() {
        let node = RemoteTmuxLayoutNode::new(
            80,
            24,
            0,
            0,
            RemoteTmuxLayoutContent::Horizontal(vec![
                RemoteTmuxLayoutNode::new(40, 24, 0, 0, RemoteTmuxLayoutContent::Pane(1)),
                RemoteTmuxLayoutNode::new(40, 24, 40, 0, RemoteTmuxLayoutContent::Pane(2)),
            ]),
        );
        let json = serde_json::to_value(&node).unwrap();
        // Flattened shape: `horizontal` is a sibling of geometry, not nested.
        assert!(json.get("horizontal").is_some());
        assert!(json.get("pane").is_none());
        let back: RemoteTmuxLayoutNode = serde_json::from_value(json).unwrap();
        assert_eq!(back, node);
    }

    #[test]
    fn layout_node_pane_decodes() {
        let json = r#"{"width":10,"height":5,"x":1,"y":2,"pane":7}"#;
        let node: RemoteTmuxLayoutNode = serde_json::from_str(json).unwrap();
        assert_eq!(node.content, RemoteTmuxLayoutContent::Pane(7));
    }

    #[test]
    fn layout_node_missing_content_key_fails() {
        let json = r#"{"width":10,"height":5,"x":1,"y":2}"#;
        let result: Result<RemoteTmuxLayoutNode, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn session_json_uses_camel_case_keys() {
        let session = RemoteTmuxSession::new("$2".into(), "main".into(), 3, true, Some(1000));
        let json = serde_json::to_value(&session).unwrap();
        assert!(json.get("windowCount").is_some());
        assert!(json.get("createdUnix").is_some());
        let back: RemoteTmuxSession = serde_json::from_value(json).unwrap();
        assert_eq!(back, session);
    }
}
