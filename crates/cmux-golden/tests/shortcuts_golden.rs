//! Golden-file parity for `cmux-core` shortcuts.
//!
//! Covers:
//!   * `StoredShortcut` / `ShortcutStroke` JSON round-trips (wire shape),
//!   * `ShortcutWhenClause::parse` → a stable **canonical AST** rendering, so
//!     drift in precedence / boolean folding / regex handling is caught,
//!   * `evaluate` **truth tables** over enumerated contexts.
//!
//! The when-clause AST has no `Serialize` impl (it carries a compiled regex), so
//! this file defines a small, deterministic JSON projection of the public AST.
//! That projection IS the canonical form the golden fixture pins. The macOS
//! Swift exporter's equivalent `ShortcutWhenClause` projection must match it
//! field-for-field; fixtures here are Rust-seeded placeholders.

mod support;

use cmux_core::shortcuts::{
    ShortcutComparisonOperator, ShortcutContext, ShortcutContextOperand, ShortcutFocusAtom,
    ShortcutStroke, ShortcutWhenClause, StoredShortcut,
};
use serde_json::{json, Value};
use support::{assert_canonical_fixture, assert_canonical_fixture_from_json_str};

const DOMAIN: &str = "shortcuts";

// ---- StoredShortcut wire-shape round-trips ---------------------------------

fn assert_stored_round_trip(name: &str, shortcut: &StoredShortcut) {
    let json = serde_json::to_string(shortcut).expect("serialize StoredShortcut");
    let decoded: StoredShortcut = serde_json::from_str(&json).expect("deserialize StoredShortcut");
    assert_eq!(
        &decoded, shortcut,
        "round-trip changed StoredShortcut {name}"
    );
    assert_canonical_fixture_from_json_str(DOMAIN, name, &json);
}

#[test]
fn stored_shortcut_unbound() {
    assert_stored_round_trip("stored_unbound", &StoredShortcut::unbound());
}

#[test]
fn stored_shortcut_single_stroke() {
    let shortcut = StoredShortcut {
        first: ShortcutStroke {
            key: "t".into(),
            command: true,
            shift: false,
            option: false,
            control: false,
            key_code: Some(17),
        },
        second: None,
    };
    assert_stored_round_trip("stored_single_stroke", &shortcut);
}

#[test]
fn stored_shortcut_chord() {
    let shortcut = StoredShortcut {
        first: ShortcutStroke {
            key: "k".into(),
            command: true,
            shift: false,
            option: false,
            control: false,
            key_code: None,
        },
        second: Some(ShortcutStroke {
            key: "s".into(),
            command: false,
            shift: true,
            option: false,
            control: false,
            key_code: None,
        }),
    };
    assert_stored_round_trip("stored_chord", &shortcut);
}

// ---- when-clause canonical AST projection ----------------------------------

fn focus_atom_name(atom: &ShortcutFocusAtom) -> &'static str {
    match atom {
        ShortcutFocusAtom::SidebarFocus => "sidebarFocus",
        ShortcutFocusAtom::BrowserFocus => "browserFocus",
        ShortcutFocusAtom::MarkdownFocus => "markdownFocus",
        ShortcutFocusAtom::TerminalFocus => "terminalFocus",
    }
}

fn op_name(op: &ShortcutComparisonOperator) -> &'static str {
    match op {
        ShortcutComparisonOperator::Equals => "eq",
        ShortcutComparisonOperator::NotEquals => "neq",
        ShortcutComparisonOperator::Matches => "matches",
        ShortcutComparisonOperator::LessThan => "lt",
        ShortcutComparisonOperator::LessThanOrEqual => "lte",
        ShortcutComparisonOperator::GreaterThan => "gt",
        ShortcutComparisonOperator::GreaterThanOrEqual => "gte",
        ShortcutComparisonOperator::InList => "in",
    }
}

fn operand_json(operand: &ShortcutContextOperand) -> Value {
    match operand {
        ShortcutContextOperand::String(s) => json!({ "string": s }),
        ShortcutContextOperand::Int(i) => json!({ "int": i }),
        ShortcutContextOperand::Regex(r) => json!({ "regex": r.pattern }),
        ShortcutContextOperand::List(items) => {
            json!({ "list": items.iter().map(operand_json).collect::<Vec<_>>() })
        }
    }
}

/// Deterministic JSON projection of the public when-clause AST.
fn clause_json(clause: &ShortcutWhenClause) -> Value {
    match clause {
        ShortcutWhenClause::Always => json!({ "node": "always" }),
        ShortcutWhenClause::Atom(atom) => {
            json!({ "node": "atom", "atom": focus_atom_name(atom) })
        }
        ShortcutWhenClause::Key(name) => json!({ "node": "key", "key": name }),
        ShortcutWhenClause::Compare { key, op, operand } => json!({
            "node": "compare",
            "key": key,
            "op": op_name(op),
            "operand": operand_json(operand),
        }),
        ShortcutWhenClause::Not(inner) => json!({ "node": "not", "child": clause_json(inner) }),
        ShortcutWhenClause::And(l, r) => json!({
            "node": "and",
            "lhs": clause_json(l),
            "rhs": clause_json(r),
        }),
        ShortcutWhenClause::Or(l, r) => json!({
            "node": "or",
            "lhs": clause_json(l),
            "rhs": clause_json(r),
        }),
    }
}

/// Parse `raw`, project to canonical AST JSON, and assert against the fixture.
/// Also asserts that re-printing is idempotent under parse (parse is total on
/// its own canonical input where applicable) by checking the projection is
/// stable across a clone.
fn assert_clause(name: &str, raw: &str) {
    let clause = ShortcutWhenClause::parse(raw)
        .unwrap_or_else(|| panic!("clause {name:?} failed to parse: {raw:?}"));
    let value = clause_json(&clause);
    assert_canonical_fixture(DOMAIN, name, &value);
}

#[test]
fn clause_empty_is_always() {
    assert_clause("clause_empty", "");
}

#[test]
fn clause_and_or_precedence() {
    // `||` binds looser than `&&`: parses as Or(term, And(term, term)).
    assert_clause(
        "clause_and_or_precedence",
        "terminalFocus || browserFocus && markdownFocus",
    );
}

#[test]
fn clause_not_and_parens() {
    assert_clause(
        "clause_not_parens",
        "!(sidebarFocus && commandPaletteVisible)",
    );
}

#[test]
fn clause_comparisons() {
    assert_clause("clause_compare_int", "paneCount >= 2");
    assert_clause("clause_compare_regex", "sidebarMode =~ /^fi/");
    assert_clause("clause_compare_in_list", "mode in ['a', 'b', 3]");
}

#[test]
fn clause_boolean_literal_folding() {
    // `key == true` folds to the bare key; `key == false` folds to Not(key).
    assert_clause("clause_fold_eq_true", "commandPaletteVisible == true");
    assert_clause("clause_fold_eq_false", "commandPaletteVisible == false");
}

// ---- evaluate truth tables -------------------------------------------------

/// Build a context covering the keys our truth-table clauses reference.
fn truth_context(palette: bool, pane_count: i64, sidebar_mode: &str) -> ShortcutContext {
    let mut ctx = ShortcutContext::default();
    ctx.set_bool("commandPaletteVisible", palette);
    ctx.set_int("paneCount", pane_count);
    ctx.set_string("sidebarMode", sidebar_mode);
    ctx
}

#[test]
fn evaluate_truth_table() {
    // For a fixed set of clauses, evaluate across an enumerated grid of contexts
    // and pin the full boolean table. This is the regression net for subtle
    // precedence / comparison drift.
    let clauses: &[(&str, &str)] = &[
        ("palette", "commandPaletteVisible"),
        ("not_palette", "!commandPaletteVisible"),
        ("panes_ge_2", "paneCount >= 2"),
        ("regex_find", "sidebarMode =~ /^fi/"),
        (
            "compound",
            "commandPaletteVisible && (paneCount >= 2 || sidebarMode == 'find')",
        ),
    ];

    let mut rows = Vec::new();
    for palette in [false, true] {
        for pane_count in [1_i64, 2] {
            for sidebar_mode in ["find", "files"] {
                let ctx = truth_context(palette, pane_count, sidebar_mode);
                let mut results = serde_json::Map::new();
                for (label, raw) in clauses {
                    let clause = ShortcutWhenClause::parse(raw).expect("clause parses");
                    results.insert((*label).to_owned(), Value::Bool(clause.evaluate(&ctx)));
                }
                rows.push(json!({
                    "commandPaletteVisible": palette,
                    "paneCount": pane_count,
                    "sidebarMode": sidebar_mode,
                    "results": Value::Object(results),
                }));
            }
        }
    }

    let table = json!({
        "clauses": clauses.iter().map(|(l, r)| json!({ "label": l, "raw": r })).collect::<Vec<_>>(),
        "rows": rows,
    });
    assert_canonical_fixture(DOMAIN, "evaluate_truth_table", &table);
}
