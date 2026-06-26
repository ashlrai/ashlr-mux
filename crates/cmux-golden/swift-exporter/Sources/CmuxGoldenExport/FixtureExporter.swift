import Foundation
import CmuxControlSocket
import CmuxAgentChat
import CmuxSettings

// Authoritative macOS fixture exporter for the WS6 golden harness.
//
// Usage:  swift run cmux-golden-export <out-dir>
//
// Writes <out-dir>/<domain>/<name>.json for every fixture, in the SAME canonical
// form as `cmux_golden::canonical_json_string` (sorted keys, uppercase UUIDs,
// serde_json-style 2-space pretty). The Rust `cmux-golden` tests then assert the
// Rust ports produce byte-identical output.
//
// The set of fixtures and their projection shapes MUST stay in lockstep with the
// Rust test files:
//   - tests/session_golden.rs
//   - tests/shortcuts_golden.rs   (when-clause AST projection; truth table)
//   - tests/osc133_golden.rs
//   - tests/ipc_golden.rs         (ControlRequest / error projection)
//
// This file is a REFERENCE skeleton: the macOS engineer fills the per-fixture
// inputs to match the Rust tests exactly. The marked TODOs are the only manual
// work; the canonicalizer and IO are complete.

@main
struct FixtureExporter {
    static func main() throws {
        let args = CommandLine.arguments
        guard args.count == 2 else {
            FileHandle.standardError.write(Data("usage: cmux-golden-export <out-dir>\n".utf8))
            exit(2)
        }
        let outDir = URL(fileURLWithPath: args[1], isDirectory: true)

        try exportSession(into: outDir)
        try exportShortcuts(into: outDir)
        try exportOSC133(into: outDir)
        try exportIPC(into: outDir)
    }

    // MARK: - IO

    static func writeFixture(_ outDir: URL, _ domain: String, _ name: String, _ json: String) throws {
        let dir = outDir.appendingPathComponent(domain, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("\(name).json")
        try (json + "\n").write(to: file, atomically: true, encoding: .utf8)
    }

    static func writeEncodable<T: Encodable>(
        _ outDir: URL, _ domain: String, _ name: String, _ value: T
    ) throws {
        try writeFixture(outDir, domain, name, try Canonicalizer.renderEncodable(value))
    }

    static func writeTree(
        _ outDir: URL, _ domain: String, _ name: String, _ tree: Any
    ) throws {
        try writeFixture(outDir, domain, name, Canonicalizer.render(tree))
    }

    // MARK: - session (Codable via JSONEncoder → canonicalize)

    static func exportSession(into outDir: URL) throws {
        // The session snapshot Codables are mirrored in SessionMirror.swift (see the
        // header there for the structural decision). The snapshot values below match
        // tests/session_golden.rs one-to-one.

        // --- full_modern_snapshot: every optional field exercised --------------
        let split = SessionWorkspaceLayoutSnapshot.split(
            SessionSplitLayoutSnapshot(
                orientation: .horizontal,
                dividerPosition: 0.5,
                first: .pane(
                    SessionPaneLayoutSnapshot(
                        panelIds: ["panel-a", "panel-b"],
                        selectedPanelId: "panel-a"
                    )
                ),
                second: .split(
                    SessionSplitLayoutSnapshot(
                        orientation: .vertical,
                        dividerPosition: 0.25,
                        first: .pane(
                            SessionPaneLayoutSnapshot(panelIds: ["panel-c"], selectedPanelId: nil)
                        ),
                        second: .pane(
                            SessionPaneLayoutSnapshot(
                                panelIds: ["panel-d"],
                                selectedPanelId: "panel-d"
                            )
                        )
                    )
                )
            )
        )

        let modernWorkspace = SessionWorkspaceSnapshot(
            workspaceId: "3F2504E0-4F89-41D3-9A0C-0305E82C3301",
            processTitle: "zsh",
            customTitle: "editor",
            customTitleSource: "user",
            currentDirectory: "/home/u/proj",
            layout: split,
            layoutMode: "split",
            canvasPanes: [
                SessionCanvasPaneSnapshot(
                    panelId: "panel-a",
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                    panelIds: ["panel-a"],
                    selectedPanelId: "panel-a"
                )
            ]
        )

        let group = SessionWorkspaceGroupSnapshot(
            id: "group-1",
            name: "Backend",
            isCollapsed: false,
            anchorWorkspaceId: "3F2504E0-4F89-41D3-9A0C-0305E82C3301",
            anchorMemberIndex: 0,
            isPinned: true,
            customColor: "#ff8800",
            iconSymbol: "server.rack"
        )

        let modern = AppSessionSnapshot(
            version: 1,
            createdAt: 1_718_900_000,
            windows: [
                SessionWindowSnapshot(
                    windowId: "window-1",
                    tabManager: SessionTabManagerSnapshot(
                        selectedWorkspaceIndex: 0,
                        workspaces: [modernWorkspace],
                        workspaceGroups: [group]
                    )
                )
            ]
        )
        try writeEncodable(outDir, "session", "full_modern_snapshot", modern)

        // --- legacy_pre_canvas_pre_tab_snapshot: pane layout, no newer fields --
        let legacyWorkspace = SessionWorkspaceSnapshot(
            processTitle: "bash",
            layout: .pane(SessionPaneLayoutSnapshot(panelIds: ["legacy-panel"]))
        )
        let legacy = AppSessionSnapshot(
            version: 1,
            createdAt: 1_600_000_000,
            windows: [
                SessionWindowSnapshot(
                    tabManager: SessionTabManagerSnapshot(workspaces: [legacyWorkspace])
                )
            ]
        )
        try writeEncodable(outDir, "session", "legacy_pre_canvas_pre_tab_snapshot", legacy)

        // --- legacy_no_layout_snapshot: layout itself absent → "layout": null --
        let noLayout = AppSessionSnapshot(
            version: 1,
            createdAt: 1_500_000_000,
            windows: [
                SessionWindowSnapshot(
                    tabManager: SessionTabManagerSnapshot(
                        workspaces: [SessionWorkspaceSnapshot(processTitle: "sh")]
                    )
                )
            ]
        )
        try writeEncodable(outDir, "session", "legacy_no_layout_snapshot", noLayout)

        // --- empty_snapshot: no windows ---------------------------------------
        let empty = AppSessionSnapshot(version: 1, createdAt: 0, windows: [])
        try writeEncodable(outDir, "session", "empty_snapshot", empty)
    }

    // MARK: - shortcuts

    static func exportShortcuts(into outDir: URL) throws {
        // StoredShortcut round-trips: real CmuxSettings Codable, write directly.
        // Inputs mirror tests/shortcuts_golden.rs exactly.
        try writeEncodable(outDir, "shortcuts", "stored_unbound", StoredShortcut.unbound)
        try writeEncodable(
            outDir, "shortcuts", "stored_single_stroke",
            StoredShortcut(first: ShortcutStroke(key: "t", command: true, keyCode: 17))
        )
        try writeEncodable(
            outDir, "shortcuts", "stored_chord",
            StoredShortcut(
                first: ShortcutStroke(key: "k", command: true),
                second: ShortcutStroke(key: "s", shift: true)
            )
        )

        // when-clause canonical AST projection (no Codable on the enum):
        // reproduce clauseJSON() from tests/shortcuts_golden.rs.
        func clauseTree(_ clause: ShortcutWhenClause) -> Any {
            switch clause {
            case .always:
                return ["node": "always"]
            case .atom(let atom):
                return ["node": "atom", "atom": atomName(atom)]
            case .key(let name):
                return ["node": "key", "key": name]
            case .compare(let key, let op, let operand):
                return ["node": "compare", "key": key, "op": opName(op), "operand": operandTree(operand)]
            case .not(let inner):
                return ["node": "not", "child": clauseTree(inner)]
            case .and(let l, let r):
                return ["node": "and", "lhs": clauseTree(l), "rhs": clauseTree(r)]
            case .or(let l, let r):
                return ["node": "or", "lhs": clauseTree(l), "rhs": clauseTree(r)]
            }
        }

        let clauseFixtures: [(String, String)] = [
            ("clause_empty", ""),
            ("clause_and_or_precedence", "terminalFocus || browserFocus && markdownFocus"),
            ("clause_not_parens", "!(sidebarFocus && commandPaletteVisible)"),
            ("clause_compare_int", "paneCount >= 2"),
            ("clause_compare_regex", "sidebarMode =~ /^fi/"),
            ("clause_compare_in_list", "mode in ['a', 'b', 3]"),
            ("clause_fold_eq_true", "commandPaletteVisible == true"),
            ("clause_fold_eq_false", "commandPaletteVisible == false"),
        ]
        for (name, raw) in clauseFixtures {
            guard let clause = ShortcutWhenClause.parse(raw) else {
                throw ExportError("clause failed to parse: \(raw)")
            }
            try writeTree(outDir, "shortcuts", name, clauseTree(clause))
        }

        // evaluate truth table — mirror the grid in tests/shortcuts_golden.rs.
        let truthClauses: [(String, String)] = [
            ("palette", "commandPaletteVisible"),
            ("not_palette", "!commandPaletteVisible"),
            ("panes_ge_2", "paneCount >= 2"),
            ("regex_find", "sidebarMode =~ /^fi/"),
            ("compound", "commandPaletteVisible && (paneCount >= 2 || sidebarMode == 'find')"),
        ]

        var rows: [Any] = []
        for palette in [false, true] {
            for paneCount in [1, 2] {
                for sidebarMode in ["find", "files"] {
                    var ctx = ShortcutContext()
                    ctx.setBool("commandPaletteVisible", palette)
                    ctx.setInt("paneCount", paneCount)
                    ctx.setString("sidebarMode", sidebarMode)

                    var results: [String: Any] = [:]
                    for (label, raw) in truthClauses {
                        guard let clause = ShortcutWhenClause.parse(raw) else {
                            throw ExportError("truth-table clause failed to parse: \(raw)")
                        }
                        results[label] = clause.evaluate(ctx)
                    }

                    rows.append([
                        "commandPaletteVisible": palette,
                        "paneCount": paneCount,
                        "sidebarMode": sidebarMode,
                        "results": results,
                    ] as [String: Any])
                }
            }
        }

        let table: [String: Any] = [
            "clauses": truthClauses.map { ["label": $0.0, "raw": $0.1] },
            "rows": rows,
        ]
        try writeTree(outDir, "shortcuts", "evaluate_truth_table", table)
    }

    // MARK: - osc133

    static func exportOSC133(into outDir: URL) throws {
        // Transcript builders mirror tests/osc133_golden.rs `esc`/`mark`.
        func esc(_ body: String) -> String { "\u{1b}]\(body)\u{07}" }
        func mark(_ kind: String) -> String { esc("133;\(kind)") }

        let transcripts: [(String, String)] = [
            (
                "happy_path",
                mark("A") + "user@host$ " + mark("B") + "echo hi" + mark("C") + "hi\n" + mark("D;0")
            ),
            (
                "nonzero_exit",
                mark("A") + mark("B") + "false" + mark("C") + mark("D;1")
            ),
            (
                "two_commands",
                mark("A") + mark("B") + "pwd" + mark("C") + "/home/u\n" + mark("D;0")
                    + mark("A") + mark("B") + "ls" + mark("C") + "a\nb\n" + mark("D;0")
            ),
            (
                "running_no_exit",
                mark("A") + mark("B") + "sleep 5" + mark("C") + "working"
            ),
            (
                "cr_fold",
                mark("A") + mark("B") + "dl" + mark("C") + "10%\r50%\r100%\n" + mark("D;0")
            ),
            (
                "crlf_lf",
                mark("A") + mark("B") + "x" + mark("C") + "line1\r\nline2\r\n" + mark("D;0")
            ),
            (
                "alt_screen_interactive",
                mark("A") + mark("B") + "vim" + mark("C") + "\u{1b}[?1049h"
            ),
            (
                "strips_noise",
                mark("A") + mark("B") + "x" + mark("C")
                    + "\u{1b}]0;my title\u{07}\u{1b}[31mred\u{1b}[0m" + "\n" + mark("D;0")
            ),
        ]

        for (name, transcript) in transcripts {
            var parser = OSC133CommandParser()
            parser.consume(transcript)
            try writeEncodable(outDir, "osc133", name, parser.blocks)
        }
    }

    // MARK: - ipc

    static func exportIPC(into outDir: URL) throws {
        // Reproduce parseProj() from tests/ipc_golden.rs: run
        // ControlRequestParser().request(fromLine:) and project Ok/Err to the
        // same shapes (outcome/id/method/params or outcome/code[/id]).
        let parser = ControlRequestParser()

        func parseProj(_ line: String) -> Any {
            switch parser.request(fromLine: line) {
            case .success(let request):
                return requestProj(request)
            case .failure(let error):
                return errorProj(error)
            }
        }

        try writeTree(
            outDir, "ipc", "ok_full_envelope",
            parseProj(#"{"id":7,"method":"system.ping","params":{"k":"v"}}"#)
        )
        try writeTree(
            outDir, "ipc", "ok_string_id",
            parseProj(#"{"id":"abc","method":"surface.list"}"#)
        )
        try writeTree(
            outDir, "ipc", "ok_null_id",
            parseProj(#"{"id":null,"method":"noop","params":{}}"#)
        )
        try writeTree(outDir, "ipc", "defect_invalid_json", parseProj("not json"))
        try writeTree(outDir, "ipc", "defect_not_an_object", parseProj("[1,2,3]"))
        try writeTree(
            outDir, "ipc", "defect_missing_method_with_id", parseProj(#"{"id":3}"#)
        )
        try writeTree(
            outDir, "ipc", "defect_missing_method_empty", parseProj(#"{"method":"   "}"#)
        )
        // invalidUTF8 cannot be reached via a String line (the byte-framing layer
        // owns that boundary), so project the variant itself — exactly as the Rust
        // test pins it.
        try writeTree(
            outDir, "ipc", "defect_invalid_utf8", errorProj(.invalidUTF8)
        )
    }

    // MARK: - ipc projection (match tests/ipc_golden.rs)

    /// `{ outcome: "ok", id: <proj|null>, method, params: <raw object> }`.
    static func requestProj(_ request: ControlRequest) -> Any {
        var tree: [String: Any] = [
            "outcome": "ok",
            "method": request.method,
            "params": request.params.mapValues(jsonValueRaw),
        ]
        tree["id"] = request.id.map(jsonValueProj) ?? NSNull()
        return tree
    }

    static func errorProj(_ error: ControlRequestParseError) -> Any {
        switch error {
        case .invalidUTF8:
            return ["outcome": "error", "code": "invalidUTF8"] as [String: Any]
        case .invalidJSON:
            return ["outcome": "error", "code": "invalidJSON"] as [String: Any]
        case .notAnObject:
            return ["outcome": "error", "code": "notAnObject"] as [String: Any]
        case .missingMethod(let id):
            var tree: [String: Any] = ["outcome": "error", "code": "missingMethod"]
            tree["id"] = id.map(jsonValueProj) ?? NSNull()
            return tree
        }
    }

    /// Tagged `{ kind, value }` projection of a `JSONValue` (matches `json_value_proj`).
    /// The `object` case carries the RAW object tree (not recursively projected),
    /// exactly like the Rust `Value::Object(map.clone())`.
    static func jsonValueProj(_ value: JSONValue) -> Any {
        switch value {
        case .null:
            return ["kind": "null"] as [String: Any]
        case .bool(let b):
            return ["kind": "bool", "value": b] as [String: Any]
        case .int(let i):
            return ["kind": "int", "value": i] as [String: Any]
        case .double(let d):
            return ["kind": "double", "value": d] as [String: Any]
        case .string(let s):
            return ["kind": "string", "value": s] as [String: Any]
        case .array(let items):
            return ["kind": "array", "value": items.map(jsonValueProj)] as [String: Any]
        case .object(let map):
            return ["kind": "object", "value": map.mapValues(jsonValueRaw)] as [String: Any]
        }
    }

    /// Natural JSON tree for a `JSONValue` (used for `params` and the `object` payload).
    static func jsonValueRaw(_ value: JSONValue) -> Any {
        switch value {
        case .null: return NSNull()
        case .bool(let b): return b
        case .int(let i): return i
        case .double(let d): return d
        case .string(let s): return s
        case .array(let items): return items.map(jsonValueRaw)
        case .object(let map): return map.mapValues(jsonValueRaw)
        }
    }

    // MARK: - projection helpers (match the Rust test projections)

    static func atomName(_ atom: ShortcutFocusAtom) -> String {
        switch atom {
        case .sidebarFocus: return "sidebarFocus"
        case .browserFocus: return "browserFocus"
        case .markdownFocus: return "markdownFocus"
        case .terminalFocus: return "terminalFocus"
        }
    }

    static func opName(_ op: ShortcutComparisonOperator) -> String {
        switch op {
        case .equals: return "eq"
        case .notEquals: return "neq"
        case .matches: return "matches"
        case .lessThan: return "lt"
        case .lessThanOrEqual: return "lte"
        case .greaterThan: return "gt"
        case .greaterThanOrEqual: return "gte"
        case .inList: return "in"
        }
    }

    static func operandTree(_ operand: ShortcutContextOperand) -> Any {
        switch operand {
        case .string(let s): return ["string": s]
        case .int(let i): return ["int": i]
        case .regex(let r): return ["regex": r.pattern]
        case .list(let items): return ["list": items.map(operandTree)]
        }
    }
}

struct ExportError: Error, CustomStringConvertible {
    let description: String
    init(_ msg: String) { description = msg }
}
