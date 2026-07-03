//! Pane identity and the `CanvasPane` value type.
//!
//! Ports `CanvasPaneID.swift`, `CanvasPanelID.swift`, and `CanvasPane.swift`.
//!
//! DIVERGENCE: Foundation's `UUID` is not available, so this module hand-rolls
//! a minimal 16-byte `Uuid`. Its `Codable` shape matches Swift exactly (an
//! uppercase `XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` string), and `Ord` on the
//! byte array is bit-for-bit equivalent to Swift's `uuidString` string ordering
//! (fixed-width uppercase hex with dashes at constant positions sorts the same
//! as the underlying bytes), preserving the `Comparable` tie-break contract.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

/// A minimal 16-byte UUID standing in for Foundation's `UUID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// The all-zero UUID.
    pub const NIL: Uuid = Uuid([0; 16]);

    /// Creates a UUID from its 16 raw bytes (in `uuidString` order).
    pub fn from_bytes(bytes: [u8; 16]) -> Uuid {
        Uuid(bytes)
    }

    /// The raw bytes, in `uuidString` order.
    pub fn bytes(&self) -> [u8; 16] {
        self.0
    }

    /// The uppercase canonical string form, matching Swift's `UUID.uuidString`.
    pub fn uuid_string(&self) -> String {
        let b = &self.0;
        format!(
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
            b[14], b[15]
        )
    }

    /// Parses a canonical UUID string (with dashes; case-insensitive).
    pub fn parse(s: &str) -> Option<Uuid> {
        let mut bytes = [0u8; 16];
        let mut idx = 0usize;
        let mut hi: Option<u8> = None;
        for ch in s.chars() {
            if ch == '-' {
                continue;
            }
            let nibble = ch.to_digit(16)? as u8;
            match hi {
                None => hi = Some(nibble),
                Some(h) => {
                    if idx >= 16 {
                        return None;
                    }
                    bytes[idx] = (h << 4) | nibble;
                    idx += 1;
                    hi = None;
                }
            }
        }
        if idx == 16 && hi.is_none() {
            Some(Uuid(bytes))
        } else {
            None
        }
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.uuid_string())
    }
}

impl Serialize for Uuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.uuid_string())
    }
}

impl<'de> Deserialize<'de> for Uuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        struct UuidVisitor;
        impl Visitor<'_> for UuidVisitor {
            type Value = Uuid;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a canonical UUID string")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Uuid, E> {
                Uuid::parse(value).ok_or_else(|| de::Error::custom("invalid UUID string"))
            }
        }
        deserializer.deserialize_str(UuidVisitor)
    }
}

/// A stable identifier for one pane on the canvas. Port of `CanvasPaneID.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanvasPaneID {
    /// The underlying panel UUID.
    #[serde(rename = "rawValue")]
    pub raw_value: Uuid,
}

impl CanvasPaneID {
    /// Creates a pane identifier from a panel UUID.
    pub fn new(raw_value: Uuid) -> CanvasPaneID {
        CanvasPaneID { raw_value }
    }
}

/// A stable identifier for one panel (tab). Port of `CanvasPanelID.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanvasPanelID {
    /// The underlying panel UUID.
    #[serde(rename = "rawValue")]
    pub raw_value: Uuid,
}

impl CanvasPanelID {
    /// Creates a panel identifier from a panel UUID.
    pub fn new(raw_value: Uuid) -> CanvasPanelID {
        CanvasPanelID { raw_value }
    }
}

use crate::geometry::CanvasRect;

/// One pane on the canvas. Port of `CanvasPane.swift`.
///
/// Z-order is not stored here; it is the pane's position inside
/// `CanvasLayout::panes` (back to front). `panel_ids` and `selected_panel_id`
/// are `private(set)` in Swift and mutated only through the pane's own methods.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasPane {
    /// The pane identifier.
    pub id: CanvasPaneID,
    /// The pane frame in canvas coordinates.
    pub frame: CanvasRect,
    /// The hosted panels (tabs), left to right. Never empty in a layout.
    #[serde(rename = "panelIds")]
    panel_ids: Vec<CanvasPanelID>,
    /// The selected tab. Always a member of `panel_ids`.
    #[serde(rename = "selectedPanelId")]
    selected_panel_id: CanvasPanelID,
}

impl CanvasPane {
    /// Creates a single-tab pane from a panel, reusing the panel UUID as the
    /// pane identifier. This is the shape every pane starts in.
    pub fn new(id: CanvasPaneID, frame: CanvasRect) -> CanvasPane {
        CanvasPane::with_panels(
            id,
            frame,
            vec![CanvasPanelID::new(id.raw_value)],
            CanvasPanelID::new(id.raw_value),
        )
    }

    /// Creates a pane with an explicit tab list (persistence restore).
    ///
    /// `selected_panel_id` falls back to the first panel when not a member of
    /// `panel_ids`. Panics on an empty tab list, matching the Swift
    /// `precondition`.
    pub fn with_panels(
        id: CanvasPaneID,
        frame: CanvasRect,
        panel_ids: Vec<CanvasPanelID>,
        selected_panel_id: CanvasPanelID,
    ) -> CanvasPane {
        assert!(
            !panel_ids.is_empty(),
            "A canvas pane must host at least one panel"
        );
        let selected = if panel_ids.contains(&selected_panel_id) {
            selected_panel_id
        } else {
            panel_ids[0]
        };
        CanvasPane {
            id,
            frame,
            panel_ids,
            selected_panel_id: selected,
        }
    }

    /// The hosted panels (tabs), left to right.
    pub fn panel_ids(&self) -> &[CanvasPanelID] {
        &self.panel_ids
    }

    /// The selected tab.
    pub fn selected_panel_id(&self) -> CanvasPanelID {
        self.selected_panel_id
    }

    /// Whether this pane hosts the given panel.
    pub fn contains(&self, panel_id: CanvasPanelID) -> bool {
        self.panel_ids.contains(&panel_id)
    }

    /// Selects a tab. Selecting a panel this pane does not host is a no-op.
    pub(crate) fn select(&mut self, panel_id: CanvasPanelID) {
        if !self.panel_ids.contains(&panel_id) {
            return;
        }
        self.selected_panel_id = panel_id;
    }

    /// Inserts a panel at `index` (clamped; `None` appends) and selects it when
    /// asked. Inserting a panel already present only re-selects it.
    pub(crate) fn insert(&mut self, panel_id: CanvasPanelID, index: Option<i64>, select: bool) {
        if !self.panel_ids.contains(&panel_id) {
            let count = self.panel_ids.len() as i64;
            let clamped = index.unwrap_or(count).max(0).min(count) as usize;
            self.panel_ids.insert(clamped, panel_id);
        }
        if select {
            self.selected_panel_id = panel_id;
        }
    }

    /// Removes a panel, moving selection to the nearest remaining neighbor.
    /// Returns `false` when the panel was the last one (the caller must remove
    /// the whole pane instead).
    pub(crate) fn remove_panel(&mut self, panel_id: CanvasPanelID) -> bool {
        let index = match self.panel_ids.iter().position(|p| *p == panel_id) {
            Some(index) => index,
            None => return true,
        };
        if self.panel_ids.len() <= 1 {
            return false;
        }
        self.panel_ids.remove(index);
        if self.selected_panel_id == panel_id {
            self.selected_panel_id = self.panel_ids[index.min(self.panel_ids.len() - 1)];
        }
        true
    }
}
