//! A precomputed dotted-key path into a JSON object tree.
//!
//! Direct port of the pure core of
//! `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Stores/JSONPath.swift:1-144`
//! (the `JSONPath` struct plus the file-scope recursive helpers
//! `assignAtPath` / `removeAtPath`).
//!
//! The cmux JSON config file is a tree of nested objects
//! (`{"app": {"appearance": "dark"}}`). [`JsonPath`] represents one leaf
//! address (`"app.appearance"` or `shortcuts.bindings["workspace.new"]`) as a
//! list of components, split once at construction so hot read/write paths do no
//! string-splitting per call.
//! Operations preserve sibling values at every level; [`JsonPath::remove`]
//! also prunes parent objects that become empty after the removal.
//!
//! # Type mapping
//!
//! Swift's untyped `[String: Any]` object tree maps to
//! [`serde_json::Value`]; an object node is [`serde_json::Value::Object`]
//! holding a [`serde_json::Map`]. Swift's `x as? [String: Any]` narrowing
//! maps to [`Value::as_object`] / [`Value::as_object_mut`].
//!
//! # Error model (sanctioned parity divergence)
//!
//! Swift's `init(dottedPath:)` uses `precondition` (a debug-build trap): the
//! call sites are compile-time-static setting ids (`JSONKey.swift:37-41`), so
//! a malformed path is a programmer error, never runtime input. We mirror the
//! *runtime* behavior two ways, both faithful because real inputs are always
//! valid:
//! - [`JsonPath::parse`] is a fallible constructor returning
//!   [`JsonPathError`] — the recommended form for any dynamic input.
//! - [`JsonPath::from_dotted`] is the trap-equivalent for static-id call
//!   sites: it panics on a malformed path, mirroring the Swift precondition.
//!
//! # What is left out
//!
//! Everything in `JSONConfigStore.swift` (file/disk I/O, JSONC sanitize,
//! `FileWatcher`, the actor cache) and `JSONKey` / `SettingCodable`
//! encode/decode are separate concerns and are not ported here.

use std::fmt;

use serde_json::{Map, Value};

/// A precomputed dotted-key path into a JSON object tree.
///
/// Mirrors the Swift `JSONPath` struct (`JSONPath.swift:12`). Constructed via
/// [`JsonPath::parse`] (fallible) or [`JsonPath::from_dotted`] (trapping),
/// after which [`components`](JsonPath::components) is always non-empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsonPath {
    /// The path segments, in order. For `"app.appearance"` this is
    /// `["app", "appearance"]`. (Swift `JSONPath.swift:15`.)
    pub components: Vec<String>,
}

/// The reason a dotted path could not be parsed into a [`JsonPath`].
///
/// Swift traps on both conditions via `precondition`
/// (`JSONPath.swift:24` and `:30-33`); we surface them as a recoverable error
/// so dynamic inputs need not panic. The [`fmt::Display`] messages mirror the
/// Swift precondition messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPathError {
    /// The dotted path was the empty string. (Swift `JSONPath.swift:24`.)
    Empty,
    /// The dotted path had a leading dot, trailing dot, or consecutive dots,
    /// yielding an empty component. Carries the offending input.
    /// (Swift `JSONPath.swift:30-33`.)
    EmptyComponent(String),
    /// The path used bracket syntax but did not contain a valid JSON string key
    /// followed by `]`, or had a stray separator.
    Malformed(String),
}

impl fmt::Display for JsonPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("JSONPath requires a non-empty dotted path"),
            Self::EmptyComponent(path) => write!(
                f,
                "JSONPath contains an empty component (leading/trailing dot or consecutive dots): {path}"
            ),
            Self::Malformed(path) => write!(f, "JSONPath is malformed: {path}"),
        }
    }
}

impl std::error::Error for JsonPathError {}

impl JsonPath {
    /// Creates a [`JsonPath`] from a dotted string, rejecting malformed input.
    ///
    /// Plain paths are split on `'.'` **without** omitting empty subsequences,
    /// matching Swift's `split(separator:omittingEmptySubsequences: false)`
    /// (`JSONPath.swift:29`). Empty input and any empty component (leading /
    /// trailing / consecutive dots, e.g. `".x"`, `"x."`, `"app..x"`) are
    /// rejected, mirroring the two Swift preconditions (`JSONPath.swift:24`,
    /// `:30-33`).
    ///
    /// As a Windows settings bridge extension, bracket-quoted keys may be used
    /// when a literal component itself contains dots, e.g.
    /// `shortcuts.bindings["workspace.new"]`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonPathError::Empty`] for empty input, or
    /// [`JsonPathError::EmptyComponent`] when any segment is empty.
    pub fn parse(dotted: &str) -> Result<Self, JsonPathError> {
        if dotted.is_empty() {
            return Err(JsonPathError::Empty);
        }
        let components = parse_components(dotted)?;
        Ok(Self { components })
    }

    /// Creates a [`JsonPath`] from a compile-time-static dotted id, panicking
    /// on malformed input.
    ///
    /// This is the trap-equivalent of the Swift `init(dottedPath:)`
    /// precondition (`JSONPath.swift:23-35`), for call sites like
    /// `JSONKey.swift:40` where the id is a static string and a malformed
    /// path is a programmer error. Prefer [`JsonPath::parse`] for any input
    /// that is not statically known.
    ///
    /// # Panics
    ///
    /// Panics if `dotted` is empty or contains an empty component.
    #[must_use]
    pub fn from_dotted(dotted: &str) -> Self {
        Self::parse(dotted).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Returns the value at this path inside `root`, or `None` when any
    /// segment is missing or has the wrong type.
    ///
    /// The final node may be any JSON type — a non-leaf object is returned as
    /// is. (Swift `lookup(in:)`, `JSONPath.swift:39-49`.) Swift's `root` is a
    /// typed `[String: Any]` dictionary; here `root` is any [`Value`], so a
    /// non-object root yields `None` (the first narrowing fails), a faithful
    /// extension of the Swift contract where the root is always an object.
    #[must_use]
    pub fn lookup<'a>(&self, root: &'a Value) -> Option<&'a Value> {
        // Swift guards `!components.isEmpty` (JSONPath.swift:40); an empty
        // path returns nil rather than the whole root. Constructed paths are
        // never empty, but honor the guard for hand-built values.
        if self.components.is_empty() {
            return None;
        }
        let mut cursor = root;
        for component in &self.components {
            cursor = cursor.as_object()?.get(component)?;
        }
        Some(cursor)
    }

    /// Assigns `value` at this path inside `root`, creating intermediate
    /// objects as needed. Sibling values are preserved at every level.
    ///
    /// Where a path segment exists but maps to a non-object value, it is
    /// overwritten with a fresh empty object before the descent continues —
    /// *write wins* (Swift `assignAtPath`, `JSONPath.swift:106`). Swift's
    /// `root` is always an object; a non-object `root` here is a no-op (the
    /// Swift-impossible case), documented rather than coerced.
    /// (Swift `assign(_:in:)`, `JSONPath.swift:53-56`.)
    pub fn assign(&self, root: &mut Value, value: Value) {
        // Swift guard `!components.isEmpty` (JSONPath.swift:54).
        if self.components.is_empty() {
            return;
        }
        if let Some(map) = root.as_object_mut() {
            assign_at_path(&self.components, value, map);
        }
    }

    /// Removes the leaf at this path from `root`. Parent objects that become
    /// empty as a result are also removed.
    ///
    /// If any intermediate segment is missing or maps to a non-object value,
    /// the call is a no-op (Swift `removeAtPath` guard, `JSONPath.swift:136`).
    /// A non-object `root` is likewise a no-op.
    /// (Swift `remove(in:)`, `JSONPath.swift:60-63`.)
    pub fn remove(&self, root: &mut Value) {
        // Swift guard `!components.isEmpty` (JSONPath.swift:61).
        if self.components.is_empty() {
            return;
        }
        if let Some(map) = root.as_object_mut() {
            remove_at_path(&self.components, map);
        }
    }
}

fn parse_components(path: &str) -> Result<Vec<String>, JsonPathError> {
    let mut components = Vec::new();
    let mut token = String::new();
    let mut chars = path.char_indices().peekable();
    let mut expects_component = true;

    while let Some((index, ch)) = chars.next() {
        match ch {
            '.' => {
                if expects_component || token.is_empty() {
                    return Err(JsonPathError::EmptyComponent(path.to_owned()));
                }
                components.push(std::mem::take(&mut token));
                expects_component = true;
            }
            '[' => {
                if !token.is_empty() {
                    components.push(std::mem::take(&mut token));
                }
                let (component, closing_index) = parse_bracket_component(path, index)?;
                components.push(component);
                expects_component = false;
                while let Some(&(next_index, next_ch)) = chars.peek() {
                    if next_index <= closing_index {
                        chars.next();
                        continue;
                    }
                    if next_ch == '.' {
                        chars.next();
                        expects_component = true;
                    } else if next_ch != '[' {
                        return Err(JsonPathError::Malformed(path.to_owned()));
                    }
                    break;
                }
            }
            ']' => return Err(JsonPathError::Malformed(path.to_owned())),
            _ => {
                token.push(ch);
                expects_component = false;
            }
        }
    }

    if !token.is_empty() {
        components.push(token);
    } else if expects_component {
        return Err(JsonPathError::EmptyComponent(path.to_owned()));
    }

    Ok(components)
}

fn parse_bracket_component(
    path: &str,
    open_index: usize,
) -> Result<(String, usize), JsonPathError> {
    let bracket_body = &path[open_index + 1..];
    let Some(first) = bracket_body.chars().next() else {
        return Err(JsonPathError::Malformed(path.to_owned()));
    };
    if first != '"' {
        return Err(JsonPathError::Malformed(path.to_owned()));
    }

    let mut escaped = false;
    for (relative_index, ch) in bracket_body.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let literal_end = relative_index + ch.len_utf8();
                let literal = &bracket_body[..literal_end];
                let after_literal = &bracket_body[literal_end..];
                if !after_literal.starts_with(']') {
                    return Err(JsonPathError::Malformed(path.to_owned()));
                }
                let component: String = serde_json::from_str(literal)
                    .map_err(|_| JsonPathError::Malformed(path.to_owned()))?;
                return Ok((component, open_index + 1 + literal_end));
            }
            _ => {}
        }
    }

    Err(JsonPathError::Malformed(path.to_owned()))
}

// ---------------------------------------------------------------------------
// Recursive helpers
//
// Direct port of the file-scope `assignAtPath` / `removeAtPath`
// (JSONPath.swift:80-143). They take a `&[String]` slice and descend via
// `split_first()` — the Rust analogue of Swift's non-allocating
// `ArraySlice.dropFirst()`. Writing or removing a path of depth `n` is O(n).
// They operate directly on the [`Map`] at the current depth rather than a
// `JsonPath`, so they never read the full stored path.
// ---------------------------------------------------------------------------

/// Writes `value` at the leaf identified by `components` inside `map`,
/// creating intermediate object levels along the way.
///
/// Sibling values at each level are preserved. A segment that exists but is
/// not an object is overwritten with a fresh empty object before descending —
/// write wins. (Swift `assignAtPath`, `JSONPath.swift:96-109`.)
fn assign_at_path(components: &[String], value: Value, map: &mut Map<String, Value>) {
    let Some((head, rest)) = components.split_first() else {
        return;
    };
    if rest.is_empty() {
        map.insert(head.clone(), value);
        return;
    }
    // `dictionary[head] as? [String: Any] ?? [:]` (JSONPath.swift:106):
    // reuse an existing child object, else start fresh — overwriting any
    // non-object value that was there (write wins).
    let child = map
        .entry(head.clone())
        .or_insert_with(|| Value::Object(Map::new()));
    if !child.is_object() {
        *child = Value::Object(Map::new());
    }
    let child = child
        .as_object_mut()
        .expect("child was just ensured to be an object");
    assign_at_path(rest, value, child);
}

/// Removes the leaf identified by `components` from `map` and prunes parent
/// objects that become empty as a result.
///
/// A missing or non-object intermediate segment makes the call a no-op. After
/// unwinding, each level whose child object became empty removes that child's
/// entry too. (Swift `removeAtPath`, `JSONPath.swift:127-143`.)
fn remove_at_path(components: &[String], map: &mut Map<String, Value>) {
    let Some((head, rest)) = components.split_first() else {
        return;
    };
    if rest.is_empty() {
        map.remove(head);
        return;
    }
    // `guard var child = dictionary[head] as? [String: Any]` (JSONPath.swift:136):
    // missing or non-object intermediate => nothing to remove.
    let Some(child) = map.get_mut(head).and_then(Value::as_object_mut) else {
        return;
    };
    remove_at_path(rest, child);
    if child.is_empty() {
        map.remove(head);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- parse: empty / leading / trailing / double-dot rejection (a) -------

    /// Empty input traps in Swift (`JSONPath.swift:24`); here it is
    /// [`JsonPathError::Empty`].
    #[test]
    fn parse_rejects_empty_input() {
        assert_eq!(JsonPath::parse(""), Err(JsonPathError::Empty));
    }

    /// Leading / trailing / consecutive dots yield an empty component and are
    /// rejected. (Swift `JSONPath.swift:30-33`.)
    #[test]
    fn parse_rejects_empty_components() {
        assert_eq!(
            JsonPath::parse(".leading"),
            Err(JsonPathError::EmptyComponent(".leading".to_owned()))
        );
        assert_eq!(
            JsonPath::parse("trailing."),
            Err(JsonPathError::EmptyComponent("trailing.".to_owned()))
        );
        assert_eq!(
            JsonPath::parse("app..appearance"),
            Err(JsonPathError::EmptyComponent("app..appearance".to_owned()))
        );
    }

    /// A well-formed dotted path splits into its components, keeping order.
    #[test]
    fn parse_splits_dotted_path() {
        let path = JsonPath::parse("app.appearance").expect("valid path");
        assert_eq!(
            path.components,
            vec!["app".to_owned(), "appearance".to_owned()]
        );
    }

    /// Bracket-quoted components are a Windows bridge extension for map keys
    /// that contain dots, such as shortcut action ids.
    #[test]
    fn parse_accepts_bracket_quoted_components_with_dots() {
        let path =
            JsonPath::parse(r#"shortcuts.bindings["workspace.new"]"#).expect("valid bracket path");
        assert_eq!(
            path.components,
            vec![
                "shortcuts".to_owned(),
                "bindings".to_owned(),
                "workspace.new".to_owned()
            ]
        );
    }

    #[test]
    fn parse_accepts_json_escaped_bracket_components() {
        let path = JsonPath::parse(r#"shortcuts.bindings["workspace.\"new\""]"#)
            .expect("valid escaped path");
        assert_eq!(
            path.components,
            vec![
                "shortcuts".to_owned(),
                "bindings".to_owned(),
                "workspace.\"new\"".to_owned()
            ]
        );
    }

    #[test]
    fn parse_rejects_malformed_bracket_components() {
        assert_eq!(
            JsonPath::parse(r#"shortcuts.bindings[workspace.new]"#),
            Err(JsonPathError::Malformed(
                r#"shortcuts.bindings[workspace.new]"#.to_owned()
            ))
        );
        assert_eq!(
            JsonPath::parse(r#"shortcuts.bindings["workspace.new""#),
            Err(JsonPathError::Malformed(
                r#"shortcuts.bindings["workspace.new""#.to_owned()
            ))
        );
    }

    /// A single-segment path parses to one component.
    #[test]
    fn parse_single_component() {
        let path = JsonPath::parse("app").expect("valid path");
        assert_eq!(path.components, vec!["app".to_owned()]);
    }

    /// [`JsonPath::from_dotted`] mirrors the Swift precondition trap on bad
    /// input. (Swift `JSONPath.swift:23-35`.)
    #[test]
    #[should_panic(expected = "empty component")]
    fn from_dotted_panics_on_malformed_path() {
        let _ = JsonPath::from_dotted("app..appearance");
    }

    /// [`JsonPath::from_dotted`] builds a path from a valid static id.
    #[test]
    fn from_dotted_builds_valid_path() {
        assert_eq!(
            JsonPath::from_dotted("automation.socketPassword").components,
            vec!["automation".to_owned(), "socketPassword".to_owned()]
        );
    }

    // --- lookup: missing / wrong-type / non-leaf object (b) -----------------

    /// Lookup returns the leaf value at a nested path.
    #[test]
    fn lookup_returns_leaf_value() {
        let root = json!({ "automation": { "socketPassword": "hunter2" } });
        let path = JsonPath::parse("automation.socketPassword").expect("valid");
        assert_eq!(path.lookup(&root), Some(&json!("hunter2")));
    }

    /// Lookup can return a non-leaf object node, not just a scalar.
    #[test]
    fn lookup_returns_non_leaf_object() {
        let root = json!({ "app": { "appearance": "dark" } });
        let path = JsonPath::parse("app").expect("valid");
        assert_eq!(path.lookup(&root), Some(&json!({ "appearance": "dark" })));
    }

    /// Lookup is `None` when a key along the path is absent.
    #[test]
    fn lookup_none_when_key_missing() {
        let root = json!({ "app": {} });
        let path = JsonPath::parse("app.appearance").expect("valid");
        assert_eq!(path.lookup(&root), None);
    }

    /// Lookup is `None` when an intermediate segment is not an object.
    #[test]
    fn lookup_none_when_intermediate_wrong_type() {
        let root = json!({ "app": "dark" });
        let path = JsonPath::parse("app.appearance").expect("valid");
        assert_eq!(path.lookup(&root), None);
    }

    /// A non-object root yields `None` (Swift's root is always an object).
    #[test]
    fn lookup_none_when_root_not_object() {
        let root = json!("scalar");
        let path = JsonPath::parse("app").expect("valid");
        assert_eq!(path.lookup(&root), None);
    }

    // --- assign: write-wins, sibling preservation, creation (c)(e) ----------

    /// Assign into an empty root creates all intermediate objects — the pure
    /// core of Swift `roundTripsNestedKey` (`JSONConfigStoreTests.swift:21-31`).
    #[test]
    fn assign_creates_intermediate_objects() {
        let mut root = json!({});
        let path = JsonPath::parse("automation.socketPassword").expect("valid");
        path.assign(&mut root, json!("hunter2"));
        assert_eq!(
            root,
            json!({ "automation": { "socketPassword": "hunter2" } })
        );
        assert_eq!(path.lookup(&root), Some(&json!("hunter2")));
    }

    #[test]
    fn assign_bracket_component_preserves_literal_dots() {
        let mut root = json!({});
        let path = JsonPath::parse(r#"shortcuts.bindings["workspace.new"]"#).expect("valid");
        path.assign(&mut root, json!("cmd+t"));
        assert_eq!(
            root,
            json!({ "shortcuts": { "bindings": { "workspace.new": "cmd+t" } } })
        );
    }

    /// Assign overwrites a primitive intermediate with a fresh object — write
    /// wins. (Swift `JSONPath.swift:106`.)
    #[test]
    fn assign_write_wins_over_primitive_intermediate() {
        let mut root = json!({ "app": "dark" });
        let path = JsonPath::parse("app.appearance").expect("valid");
        path.assign(&mut root, json!("light"));
        assert_eq!(root, json!({ "app": { "appearance": "light" } }));
    }

    /// Assign preserves sibling values at every level.
    #[test]
    fn assign_preserves_siblings_each_level() {
        let mut root = json!({ "app": { "appearance": "dark" }, "other": "keep" });
        let path = JsonPath::parse("app.newKey").expect("valid");
        path.assign(&mut root, json!("value"));
        assert_eq!(
            root,
            json!({ "app": { "appearance": "dark", "newKey": "value" }, "other": "keep" })
        );
    }

    /// Assign overwrites an existing leaf in place.
    #[test]
    fn assign_overwrites_existing_leaf() {
        let mut root = json!({ "app": { "appearance": "dark" } });
        let path = JsonPath::parse("app.appearance").expect("valid");
        path.assign(&mut root, json!("light"));
        assert_eq!(root, json!({ "app": { "appearance": "light" } }));
    }

    /// A single-component path writes directly into the root, keeping siblings.
    #[test]
    fn assign_single_component_writes_directly() {
        let mut root = json!({ "keep": "x" });
        let path = JsonPath::parse("top").expect("valid");
        path.assign(&mut root, json!(1));
        assert_eq!(root, json!({ "keep": "x", "top": 1 }));
    }

    /// A non-object root is a no-op (Swift-impossible case).
    #[test]
    fn assign_noop_when_root_not_object() {
        let mut root = json!("scalar");
        let path = JsonPath::parse("app").expect("valid");
        path.assign(&mut root, json!(1));
        assert_eq!(root, json!("scalar"));
    }

    // --- remove: pruning, no-op guards, sibling preservation (d)(e) ---------

    /// Remove deletes the leaf and prunes the now-empty parent — the pure core
    /// of Swift `resetRemovesEntryAndPrunesEmptyParents`
    /// (`JSONConfigStoreTests.swift:33-42`).
    #[test]
    fn remove_prunes_empty_parents() {
        let mut root = json!({ "automation": { "socketPassword": "hunter2" } });
        let path = JsonPath::parse("automation.socketPassword").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({}));
    }

    /// Remove keeps a parent that still has other children.
    #[test]
    fn remove_keeps_nonempty_parent() {
        let mut root = json!({ "automation": { "socketPassword": "h", "other": "keep" } });
        let path = JsonPath::parse("automation.socketPassword").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({ "automation": { "other": "keep" } }));
    }

    /// Remove preserves a sibling branch untouched.
    #[test]
    fn remove_preserves_sibling_branch() {
        let mut root =
            json!({ "automation": { "socketPassword": "h" }, "app": { "appearance": "dark" } });
        let path = JsonPath::parse("automation.socketPassword").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({ "app": { "appearance": "dark" } }));
    }

    /// Remove prunes multiple levels that all become empty on unwind.
    #[test]
    fn remove_prunes_multiple_empty_levels() {
        let mut root = json!({ "a": { "b": { "c": 1 } } });
        let path = JsonPath::parse("a.b.c").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({}));
    }

    /// Remove is a no-op when an intermediate segment is missing.
    #[test]
    fn remove_noop_when_intermediate_missing() {
        let mut root = json!({ "app": { "appearance": "dark" } });
        let path = JsonPath::parse("automation.socketPassword").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({ "app": { "appearance": "dark" } }));
    }

    /// Remove is a no-op when an intermediate segment is not an object.
    /// (Swift `JSONPath.swift:136` guard.)
    #[test]
    fn remove_noop_when_intermediate_non_object() {
        let mut root = json!({ "app": "dark" });
        let path = JsonPath::parse("app.appearance").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({ "app": "dark" }));
    }

    /// Removing a missing leaf leaves the (still non-empty) parent intact.
    #[test]
    fn remove_missing_leaf_is_noop() {
        let mut root = json!({ "app": { "appearance": "dark" } });
        let path = JsonPath::parse("app.missing").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({ "app": { "appearance": "dark" } }));
    }

    /// A single-component remove deletes the top-level key, keeping siblings.
    #[test]
    fn remove_single_component() {
        let mut root = json!({ "top": 1, "keep": 2 });
        let path = JsonPath::parse("top").expect("valid");
        path.remove(&mut root);
        assert_eq!(root, json!({ "keep": 2 }));
    }

    // --- assign -> remove round-trip yields empty root (f) ------------------

    /// Assigning then removing the same deep path restores the empty root.
    #[test]
    fn assign_then_remove_yields_empty_root() {
        let mut root = json!({});
        let path = JsonPath::parse("a.b.c").expect("valid");
        path.assign(&mut root, json!(42));
        assert_eq!(root, json!({ "a": { "b": { "c": 42 } } }));
        path.remove(&mut root);
        assert_eq!(root, json!({}));
    }

    /// The [`JsonPathError`] Display messages mirror the Swift precondition
    /// text. (Swift `JSONPath.swift:24`, `:32`.)
    #[test]
    fn error_display_matches_swift_messages() {
        assert_eq!(
            JsonPathError::Empty.to_string(),
            "JSONPath requires a non-empty dotted path"
        );
        assert_eq!(
            JsonPathError::EmptyComponent("app..x".to_owned()).to_string(),
            "JSONPath contains an empty component (leading/trailing dot or consecutive dots): app..x"
        );
    }
}
