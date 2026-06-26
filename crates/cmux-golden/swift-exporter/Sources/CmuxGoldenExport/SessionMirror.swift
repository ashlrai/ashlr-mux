import Foundation

// Mirror of the `cmux-core::session` wire contract (crates/cmux-core/src/session.rs)
// for the golden-fixture exporter.
//
// SESSION STRUCTURAL DECISION (option a — "mirror the snapshot Codables"):
//
// The live macOS `AppSessionSnapshot` lives in the app target
// (`Sources/SessionPersistence.swift`) and is NOT reachable from this SPM
// executable. More importantly, its `JSONEncoder` output is a *different* wire
// shape from the Rust port: it uses camelCase keys (`createdAt`, `tabManager`,
// `processTitle`, …) and carries ~15 additional fields the port deliberately
// omits (`frame`, `display`, `sidebar`, `panels`, `statusEntries`, `logEntries`,
// `progress`, `gitBranch`, `remote`, `environment`, …). There is no importable
// Swift codec that emits the port's minimal snake_case shape (the snake_case
// `created_at`/`selected_workspace_index` strings elsewhere in the app belong to
// the control-event publishing layer, not session persistence).
//
// So the parity target for the session domain is the port's OWN documented wire
// contract. We reproduce that contract here as an independent, standalone Swift
// `Encodable` graph (snake_case `CodingKeys`, integer time/dimension fields,
// `layout` nullable-not-omitted, the `{type, pane|split}` tagged union). Running
// it through the shared `Canonicalizer` proves a Swift implementation of the
// on-disk contract emits byte-identical canonical JSON to the Rust port.
//
// (The live-app `AppSessionSnapshot` ↔ port divergence — camelCase + extra
// fields — is a pre-existing gap outside M1's golden scope; flagged for
// follow-up. M1 pins the port's contract, which is what these fixtures encode.)
//
// Parity-critical type choices that the canonicalizer is sensitive to:
//   * `version` / `created_at` / canvas `x,y,width,height` are `Int` (Rust i64).
//     A `Double` would render as e.g. `1718900000.0` via the canonicalizer's
//     integral-double `%.1f` path and diverge from serde_json's integer form.
//   * `divider_position` is `Double` (Rust f64) → `0.5` / `0.25`.
//   * `layout` uses an explicit `encode(..., forKey:)` (not `encodeIfPresent`) so
//     it serializes as `"layout": null` when absent — matching the Rust field's
//     `#[serde(default)]` WITHOUT `skip_serializing_if`. Every other optional
//     uses `encodeIfPresent` (key omitted), matching the Rust skip attributes.

enum SessionSplitOrientation: String, Encodable {
    case horizontal
    case vertical
}

struct SessionPaneLayoutSnapshot: Encodable {
    var panelIds: [String]
    var selectedPanelId: String? = nil

    enum CodingKeys: String, CodingKey {
        case panelIds = "panel_ids"
        case selectedPanelId = "selected_panel_id"
    }
}

struct SessionSplitLayoutSnapshot: Encodable {
    var orientation: SessionSplitOrientation
    var dividerPosition: Double
    var first: SessionWorkspaceLayoutSnapshot
    var second: SessionWorkspaceLayoutSnapshot

    enum CodingKeys: String, CodingKey {
        case orientation
        case dividerPosition = "divider_position"
        case first
        case second
    }
}

/// Tagged union mirroring the Rust hand-written serializer:
/// `{ "type": "pane", "pane": … }` / `{ "type": "split", "split": … }`.
indirect enum SessionWorkspaceLayoutSnapshot: Encodable {
    case pane(SessionPaneLayoutSnapshot)
    case split(SessionSplitLayoutSnapshot)

    enum CodingKeys: String, CodingKey {
        case type
        case pane
        case split
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .pane(let pane):
            try container.encode("pane", forKey: .type)
            try container.encode(pane, forKey: .pane)
        case .split(let split):
            try container.encode("split", forKey: .type)
            try container.encode(split, forKey: .split)
        }
    }
}

struct SessionCanvasPaneSnapshot: Encodable {
    var panelId: String
    var x: Int
    var y: Int
    var width: Int
    var height: Int
    var panelIds: [String]? = nil
    var selectedPanelId: String? = nil

    enum CodingKeys: String, CodingKey {
        case panelId = "panel_id"
        case x
        case y
        case width
        case height
        case panelIds = "panel_ids"
        case selectedPanelId = "selected_panel_id"
    }
}

struct SessionWorkspaceSnapshot: Encodable {
    var workspaceId: String? = nil
    var processTitle: String
    var customTitle: String? = nil
    var customTitleSource: String? = nil
    var currentDirectory: String? = nil
    /// Nullable-not-omitted: serialized as `"layout": null` when nil.
    var layout: SessionWorkspaceLayoutSnapshot? = nil
    var layoutMode: String? = nil
    var canvasPanes: [SessionCanvasPaneSnapshot]? = nil

    enum CodingKeys: String, CodingKey {
        case workspaceId = "workspace_id"
        case processTitle = "process_title"
        case customTitle = "custom_title"
        case customTitleSource = "custom_title_source"
        case currentDirectory = "current_directory"
        case layout
        case layoutMode = "layout_mode"
        case canvasPanes = "canvas_panes"
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encodeIfPresent(workspaceId, forKey: .workspaceId)
        try container.encode(processTitle, forKey: .processTitle)
        try container.encodeIfPresent(customTitle, forKey: .customTitle)
        try container.encodeIfPresent(customTitleSource, forKey: .customTitleSource)
        try container.encodeIfPresent(currentDirectory, forKey: .currentDirectory)
        // Explicit encode (not encodeIfPresent) → "layout": null when absent.
        try container.encode(layout, forKey: .layout)
        try container.encodeIfPresent(layoutMode, forKey: .layoutMode)
        try container.encodeIfPresent(canvasPanes, forKey: .canvasPanes)
    }
}

struct SessionWorkspaceGroupSnapshot: Encodable {
    var id: String
    var name: String
    var isCollapsed: Bool
    var anchorWorkspaceId: String? = nil
    var anchorMemberIndex: Int? = nil
    var isPinned: Bool? = nil
    var customColor: String? = nil
    var iconSymbol: String? = nil

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case isCollapsed = "is_collapsed"
        case anchorWorkspaceId = "anchor_workspace_id"
        case anchorMemberIndex = "anchor_member_index"
        case isPinned = "is_pinned"
        case customColor = "custom_color"
        case iconSymbol = "icon_symbol"
    }
}

struct SessionTabManagerSnapshot: Encodable {
    var selectedWorkspaceIndex: Int? = nil
    var workspaces: [SessionWorkspaceSnapshot]
    var workspaceGroups: [SessionWorkspaceGroupSnapshot]? = nil

    enum CodingKeys: String, CodingKey {
        case selectedWorkspaceIndex = "selected_workspace_index"
        case workspaces
        case workspaceGroups = "workspace_groups"
    }
}

struct SessionWindowSnapshot: Encodable {
    var windowId: String? = nil
    var tabManager: SessionTabManagerSnapshot

    enum CodingKeys: String, CodingKey {
        case windowId = "window_id"
        case tabManager = "tab_manager"
    }
}

struct AppSessionSnapshot: Encodable {
    var version: Int
    var createdAt: Int
    var windows: [SessionWindowSnapshot]

    enum CodingKeys: String, CodingKey {
        case version
        case createdAt = "created_at"
        case windows
    }
}
