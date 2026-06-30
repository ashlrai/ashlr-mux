use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

#[cfg(feature = "ts")]
use ts_rs::TS;

// NOTE (WS1 / cmux core-types): the `#[cfg_attr(feature = "ts", ...)]` derives
// below are the only thing the optional `ts` feature adds. They are inert in the
// default build (ts-rs is an optional dep), so `cargo test`/`clippy` for the
// default configuration are unaffected. `#[ts(export)]` makes each type emit a
// `.ts` file into `TS_RS_EXPORT_DIR` when the `export_bindings_*` tests run.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct SessionPaneLayoutSnapshot {
    pub panel_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub selected_panel_id: Option<String>,
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
    pub orientation: SessionSplitOrientation,
    pub divider_position: f64,
    pub first: Box<SessionWorkspaceLayoutSnapshot>,
    pub second: Box<SessionWorkspaceLayoutSnapshot>,
}

// `SessionWorkspaceLayoutSnapshot` carries hand-written `Serialize`/`Deserialize`
// impls (the `{type, pane|split}` adjacently-tagged wire shape), so ts-rs cannot
// derive a matching `TS`. The manual `TS` impl below (also feature-gated) mirrors
// that exact wire shape. See the `impl TS` block further down.
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
    pub current_directory: Option<String>,
    // `layout` has `#[serde(default)]` but NO `skip_serializing_if`, so it is
    // serialized as `"layout": null` when absent — keep it nullable, not optional.
    #[serde(default)]
    pub layout: Option<SessionWorkspaceLayoutSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub canvas_panes: Option<Vec<SessionCanvasPaneSnapshot>>,
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
    pub tab_manager: SessionTabManagerSnapshot,
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
export type { SessionCanvasPaneSnapshot } from \"./SessionCanvasPaneSnapshot\";
export type { SessionPaneLayoutSnapshot } from \"./SessionPaneLayoutSnapshot\";
export type { SessionSplitLayoutSnapshot } from \"./SessionSplitLayoutSnapshot\";
export type { SessionSplitOrientation } from \"./SessionSplitOrientation\";
export type { SessionTabManagerSnapshot } from \"./SessionTabManagerSnapshot\";
export type { SessionWindowSnapshot } from \"./SessionWindowSnapshot\";
export type { SessionWorkspaceGroupSnapshot } from \"./SessionWorkspaceGroupSnapshot\";
export type { SessionWorkspaceLayoutSnapshot } from \"./SessionWorkspaceLayoutSnapshot\";
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
        let _ = AppSessionSnapshot::name();
    }
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
