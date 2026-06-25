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
        // TODO(macOS): construct the same AppSessionSnapshot values as
        // tests/session_golden.rs (full_modern_snapshot, legacy_pre_canvas_*,
        // legacy_no_layout_snapshot, empty_snapshot) using the app's
        // AppSessionSnapshot Codable, then:
        //
        //   try writeEncodable(outDir, "session", "full_modern_snapshot", snapshot)
        //
        // AppSessionSnapshot lives in the app target (Sources/SessionPersistence.swift).
        // If it is not importable from this SPM executable, mirror the snapshot
        // Codables into a tiny shared module, or run this exporter as an XCTest
        // inside the app target instead (see README "Alternative: XCTest host").
    }

    // MARK: - shortcuts

    static func exportShortcuts(into outDir: URL) throws {
        // StoredShortcut round-trips: Codable, write directly.
        // TODO(macOS): match tests/shortcuts_golden.rs inputs exactly.
        //   try writeEncodable(outDir, "shortcuts", "stored_unbound", StoredShortcut.unbound)
        //   try writeEncodable(outDir, "shortcuts", "stored_single_stroke", ...)
        //   try writeEncodable(outDir, "shortcuts", "stored_chord", ...)

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
        // TODO(macOS): build the same clause list + context grid + write
        //   "evaluate_truth_table" using ShortcutContext + clause.evaluate.
    }

    // MARK: - osc133

    static func exportOSC133(into outDir: URL) throws {
        // TerminalCommandBlock is Codable with the same snake_case keys.
        // Replay each transcript from tests/osc133_golden.rs through
        // OSC133CommandParser and write parser.blocks.
        //   var p = OSC133CommandParser(); p.consume(transcript)
        //   try writeEncodable(outDir, "osc133", name, p.blocks)
        // TODO(macOS): port the transcript builders (mark()/esc()) and names.
    }

    // MARK: - ipc

    static func exportIPC(into outDir: URL) throws {
        // Reproduce parseProj() from tests/ipc_golden.rs: run
        // ControlRequestParser().request(fromLine:) and project Ok/Err to the
        // same shapes (outcome/id/method/params or outcome/code[/id]).
        // TODO(macOS): port the fixture lines + projection.
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
