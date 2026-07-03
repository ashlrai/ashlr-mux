//! cmux-sidebar-args — sidebar metadata argument parser.
//!
//! Headless port of `SidebarMetadataArgumentParser` and its target value types
//! from the canonical macOS `Packages/macOS/CmuxSidebar` package: a stateless
//! shell-like tokenizer, a `--key[=value]` option parser (stop-at-`--` and
//! no-stop variants), metadata-format / tab-target / optional-panel-id parsing,
//! and the ` -- ` metadata-block splitter. 100% pure (UUID + string trimming).
//!
//! Behavior is byte-identical to the Swift oracle, including the verbatim error
//! strings (with their Unicode em-dash and ASCII quotes) so the control-socket
//! wire format is preserved exactly.

use std::collections::HashMap;
use uuid::Uuid;

/// How a sidebar status entry's value text is rendered.
///
/// Raw values are a control-socket wire format; frozen. Ports
/// `Status/SidebarMetadataFormat.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMetadataFormat {
    /// Plain text.
    Plain,
    /// Markdown-rendered text.
    Markdown,
}

impl SidebarMetadataFormat {
    /// The frozen control-socket wire token for this format.
    pub fn raw_value(self) -> &'static str {
        match self {
            SidebarMetadataFormat::Plain => "plain",
            SidebarMetadataFormat::Markdown => "markdown",
        }
    }
}

/// Which tab a sidebar-metadata mutation or report command addresses.
///
/// Ports `Metadata/SidebarMutationTabTarget.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarMutationTabTarget {
    /// No `--tab` option was supplied; address the currently selected tab.
    Selected,
    /// `--tab=<uuid>`: address the workspace/tab with this identifier.
    Workspace(Uuid),
    /// `--tab=<n>`: address the tab at this zero-based index.
    Index(usize),
}

/// The outcome of parsing a `--tab` option into a [`SidebarMutationTabTarget`].
///
/// At most one of `target` and `error` is non-`None`. Ports
/// `Metadata/SidebarMutationTabTargetResolution.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarMutationTabTargetResolution {
    /// The parsed target, or `None` when the `--tab` option was malformed.
    pub target: Option<SidebarMutationTabTarget>,
    /// The error string to return verbatim, or `None` on success.
    pub error: Option<String>,
}

/// The outcome of parsing an optional `--panel`/`--surface` id option.
///
/// At most one of `panel_id` and `error` is non-`None`. Ports
/// `Metadata/SidebarOptionalPanelId.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarOptionalPanelId {
    /// The parsed panel id, or `None` when the option was absent or malformed.
    pub panel_id: Option<Uuid>,
    /// The verbatim error string, or `None` when the option was absent or valid.
    pub error: Option<String>,
}

/// The positional arguments and option dictionary parsed from an argument string.
///
/// The Swift oracle returns a `(positional, options)` tuple; a named struct is
/// used here to keep call sites unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedOptions {
    /// The positional arguments, in order.
    pub positional: Vec<String>,
    /// The `--key[=value]` options.
    pub options: HashMap<String, String>,
}

/// Stateless parser for sidebar-metadata and sidebar-mutation control-socket
/// commands.
///
/// All methods are pure transforms over the raw argument string. The parser
/// holds no state and reaches no app singletons; resolving a parsed target to a
/// concrete tab is the caller's job. Ports
/// `Metadata/SidebarMetadataArgumentParser.swift`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SidebarMetadataArgumentParser;

impl SidebarMetadataArgumentParser {
    /// Creates a parser. The parser is stateless; a fresh instance is cheap.
    pub fn new() -> Self {
        SidebarMetadataArgumentParser
    }

    /// Splits a raw argument string into tokens using shell-like quoting.
    ///
    /// Single and double quotes group tokens; inside a quote, the escapes
    /// `\n`, `\r`, `\t`, `\"`, `\'`, and `\\` are interpreted, and an unknown
    /// escape is preserved literally (the backslash is kept). Unescaped
    /// whitespace separates tokens. Empty input yields no tokens.
    pub fn tokenize(&self, args: &str) -> Vec<String> {
        let trimmed: Vec<char> = args.trim().chars().collect();
        if trimmed.is_empty() {
            return Vec::new();
        }

        let mut tokens: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut in_quote = false;
        let mut quote_char = '"';
        let mut i = 0;
        let n = trimmed.len();

        while i < n {
            let ch = trimmed[i];
            if in_quote {
                if ch == quote_char {
                    in_quote = false;
                    i += 1;
                    continue;
                }
                if ch == '\\' {
                    let next_index = i + 1;
                    if next_index < n {
                        let next = trimmed[next_index];
                        match next {
                            'n' => {
                                current.push('\n');
                                i = next_index + 1;
                                continue;
                            }
                            'r' => {
                                current.push('\r');
                                i = next_index + 1;
                                continue;
                            }
                            't' => {
                                current.push('\t');
                                i = next_index + 1;
                                continue;
                            }
                            '"' | '\'' | '\\' => {
                                current.push(next);
                                i = next_index + 1;
                                continue;
                            }
                            // Unknown escape: keep the backslash literally, then
                            // fall through so the backslash is appended below.
                            _ => {}
                        }
                    }
                }
                current.push(ch);
                i += 1;
                continue;
            }

            if ch == '\'' || ch == '"' {
                in_quote = true;
                quote_char = ch;
                i += 1;
                continue;
            }

            if ch.is_whitespace() {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                i += 1;
                continue;
            }

            current.push(ch);
            i += 1;
        }

        if !current.is_empty() {
            tokens.push(current);
        }
        tokens
    }

    /// Parses `--key[=value]` options and positional arguments, stopping option
    /// parsing at a bare `--` (everything after is positional).
    pub fn parse_options(&self, args: &str) -> ParsedOptions {
        let tokens = self.tokenize(args);
        let mut positional: Vec<String> = Vec::new();
        let mut options: HashMap<String, String> = HashMap::new();
        if tokens.is_empty() {
            return ParsedOptions {
                positional,
                options,
            };
        }

        let mut stop_parsing_options = false;
        let mut i = 0;
        while i < tokens.len() {
            if stop_parsing_options {
                positional.push(tokens[i].clone());
            } else if tokens[i].as_str() == "--" {
                stop_parsing_options = true;
            } else if tokens[i].starts_with("--") {
                i += insert_option(&tokens, i, &mut options);
            } else {
                positional.push(tokens[i].clone());
            }
            i += 1;
        }

        ParsedOptions {
            positional,
            options,
        }
    }

    /// Parses `--key[=value]` options and positional arguments, treating a bare
    /// `--` as a no-op separator that is dropped rather than a stop marker.
    pub fn parse_options_no_stop(&self, args: &str) -> ParsedOptions {
        let tokens = self.tokenize(args);
        let mut positional: Vec<String> = Vec::new();
        let mut options: HashMap<String, String> = HashMap::new();
        if tokens.is_empty() {
            return ParsedOptions {
                positional,
                options,
            };
        }

        let mut i = 0;
        while i < tokens.len() {
            if tokens[i].as_str() == "--" {
                i += 1;
                continue;
            }
            if tokens[i].starts_with("--") {
                i += insert_option(&tokens, i, &mut options);
            } else {
                positional.push(tokens[i].clone());
            }
            i += 1;
        }

        ParsedOptions {
            positional,
            options,
        }
    }

    /// Parses a metadata-format token into a [`SidebarMetadataFormat`].
    ///
    /// Accepts `plain`, `markdown`, and the `md` alias (case-insensitive).
    pub fn parse_metadata_format(&self, raw: &str) -> Option<SidebarMetadataFormat> {
        match raw.to_lowercase().as_str() {
            "plain" => Some(SidebarMetadataFormat::Plain),
            "markdown" | "md" => Some(SidebarMetadataFormat::Markdown),
            _ => None,
        }
    }

    /// Normalizes an optional option value: trims whitespace and maps an empty
    /// result to `None`.
    pub fn normalized_option_value(&self, value: Option<&str>) -> Option<String> {
        let value = value?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Parses the `--tab` option into a [`SidebarMutationTabTargetResolution`].
    ///
    /// Absent `--tab` resolves to [`SidebarMutationTabTarget::Selected`]. A UUID
    /// resolves to [`SidebarMutationTabTarget::Workspace`]; a non-negative
    /// integer resolves to [`SidebarMutationTabTarget::Index`]; anything else
    /// (including an empty value) yields the verbatim `ERROR: Tab not found`.
    pub fn parse_mutation_tab_target(
        &self,
        options: &HashMap<String, String>,
    ) -> SidebarMutationTabTargetResolution {
        if let Some(raw_tab_arg) = options.get("tab") {
            let tab_arg = raw_tab_arg.trim();
            if tab_arg.is_empty() {
                return SidebarMutationTabTargetResolution {
                    target: None,
                    error: Some("ERROR: Tab not found".to_string()),
                };
            }
            if let Some(tab_id) = parse_canonical_uuid(tab_arg) {
                return SidebarMutationTabTargetResolution {
                    target: Some(SidebarMutationTabTarget::Workspace(tab_id)),
                    error: None,
                };
            }
            // DIVERGENCE: Swift `Int(tabArg)` parses a signed value then rejects
            // it with `index >= 0`. Parse as i64 (not usize) so a negative value
            // reaches the same "Tab not found" branch instead of failing to parse
            // for a different reason — behavior is identical either way, but this
            // mirrors the oracle's control flow exactly.
            if let Ok(index) = tab_arg.parse::<i64>() {
                if index >= 0 {
                    return SidebarMutationTabTargetResolution {
                        target: Some(SidebarMutationTabTarget::Index(index as usize)),
                        error: None,
                    };
                }
            }
            return SidebarMutationTabTargetResolution {
                target: None,
                error: Some("ERROR: Tab not found".to_string()),
            };
        }
        SidebarMutationTabTargetResolution {
            target: Some(SidebarMutationTabTarget::Selected),
            error: None,
        }
    }

    /// Parses the optional `--panel`/`--surface` id option.
    ///
    /// `--surface` is honored as an alias when `--panel` is absent. An empty
    /// value yields `ERROR: Missing panel id — usage: <usage>`; a non-UUID value
    /// yields `ERROR: Invalid panel id '<raw>'`; an absent option yields a `None`
    /// id with no error. Error strings are returned verbatim.
    pub fn parse_optional_panel_id(
        &self,
        options: &HashMap<String, String>,
        usage: &str,
    ) -> SidebarOptionalPanelId {
        let raw_panel_arg = match options.get("panel").or_else(|| options.get("surface")) {
            Some(value) => value,
            None => {
                return SidebarOptionalPanelId {
                    panel_id: None,
                    error: None,
                };
            }
        };
        let panel_arg = raw_panel_arg.trim();
        if panel_arg.is_empty() {
            return SidebarOptionalPanelId {
                panel_id: None,
                error: Some(format!("ERROR: Missing panel id — usage: {usage}")),
            };
        }
        match parse_canonical_uuid(panel_arg) {
            Some(panel_id) => SidebarOptionalPanelId {
                panel_id: Some(panel_id),
                error: None,
            },
            // Uses the untrimmed `raw_panel_arg` verbatim, matching the oracle.
            None => SidebarOptionalPanelId {
                panel_id: None,
                error: Some(format!("ERROR: Invalid panel id '{raw_panel_arg}'")),
            },
        }
    }

    /// Splits a metadata-block argument string at the first ` -- ` separator into
    /// an options part and an optional trailing markdown part.
    pub fn split_metadata_block_args(&self, args: &str) -> (String, Option<String>) {
        match args.find(" -- ") {
            Some(pos) => {
                let options_part = args[..pos].to_string();
                // " -- " is 4 ASCII bytes.
                let markdown_part = args[pos + 4..].to_string();
                (options_part, Some(markdown_part))
            }
            None => (args.to_string(), None),
        }
    }
}

/// Inserts one `--key[=value]` option from `tokens[i]` (which is known to start
/// with `--`) into `options`. Returns the number of *extra* tokens consumed
/// (1 when the value was taken from the following token, otherwise 0).
fn insert_option(tokens: &[String], i: usize, options: &mut HashMap<String, String>) -> usize {
    let token = &tokens[i];
    if let Some(eq) = token.find('=') {
        // `token` starts with "--", so `eq >= 2` and byte index 2 is a char
        // boundary; slicing is safe.
        options.insert(token[2..eq].to_string(), token[eq + 1..].to_string());
        0
    } else {
        let key = token[2..].to_string();
        if i + 1 < tokens.len() && !tokens[i + 1].starts_with("--") {
            options.insert(key, tokens[i + 1].clone());
            1
        } else {
            options.insert(key, String::new());
            0
        }
    }
}

/// Parses a UUID restricted to the canonical hyphenated 8-4-4-4-12 form.
///
/// Swift's `UUID(uuidString:)` accepts *only* this form (it rejects the simple,
/// URN, and braced forms). `uuid::Uuid::parse_str` is more permissive, so the
/// shape is validated first to keep the invalid → error branches byte-identical
/// to the oracle.
fn parse_canonical_uuid(value: &str) -> Option<Uuid> {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return None;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            8 | 13 | 18 | 23 => {
                if *byte != b'-' {
                    return None;
                }
            }
            _ => {
                if !byte.is_ascii_hexdigit() {
                    return None;
                }
            }
        }
    }
    Uuid::parse_str(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> SidebarMetadataArgumentParser {
        SidebarMetadataArgumentParser::new()
    }

    fn opts(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // A fixed canonical UUID — no randomness, for deterministic tests.
    fn sample_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
    }

    #[test]
    fn tokenize_whitespace() {
        assert_eq!(parser().tokenize("  a   b\tc "), vec!["a", "b", "c"]);
        assert_eq!(parser().tokenize(""), Vec::<String>::new());
        assert_eq!(parser().tokenize("   "), Vec::<String>::new());
    }

    #[test]
    fn tokenize_quotes() {
        assert_eq!(parser().tokenize("'a b' c"), vec!["a b", "c"]);
        assert_eq!(parser().tokenize("\"x\\ny\""), vec!["x\ny"]);
        assert_eq!(parser().tokenize("\"a\\tb\\r\""), vec!["a\tb\r"]);
        assert_eq!(parser().tokenize("\"q\\\"q\""), vec!["q\"q"]);
        // Unknown escape is preserved literally (backslash kept).
        assert_eq!(parser().tokenize("\"a\\zb\""), vec!["a\\zb"]);
    }

    #[test]
    fn parse_options_basics() {
        let r = parser().parse_options("pos1 --a=1 --b 2 -- --c not-an-option");
        assert_eq!(r.positional, vec!["pos1", "--c", "not-an-option"]);
        assert_eq!(r.options, opts(&[("a", "1"), ("b", "2")]));
    }

    #[test]
    fn parse_options_empty_flag() {
        let r = parser().parse_options("--flag");
        assert_eq!(r.options.get("flag").map(String::as_str), Some(""));
        let r2 = parser().parse_options("--flag --next");
        assert_eq!(r2.options.get("flag").map(String::as_str), Some(""));
        assert_eq!(r2.options.get("next").map(String::as_str), Some(""));
    }

    #[test]
    fn parse_options_no_stop() {
        let r = parser().parse_options_no_stop("k v -- --a 1");
        assert_eq!(r.positional, vec!["k", "v"]);
        assert_eq!(r.options, opts(&[("a", "1")]));
    }

    #[test]
    fn metadata_format() {
        assert_eq!(
            parser().parse_metadata_format("plain"),
            Some(SidebarMetadataFormat::Plain)
        );
        assert_eq!(
            parser().parse_metadata_format("PLAIN"),
            Some(SidebarMetadataFormat::Plain)
        );
        assert_eq!(
            parser().parse_metadata_format("markdown"),
            Some(SidebarMetadataFormat::Markdown)
        );
        assert_eq!(
            parser().parse_metadata_format("md"),
            Some(SidebarMetadataFormat::Markdown)
        );
        assert_eq!(parser().parse_metadata_format("html"), None);
    }

    #[test]
    fn normalized_value() {
        assert_eq!(
            parser().normalized_option_value(Some("  x  ")),
            Some("x".to_string())
        );
        assert_eq!(parser().normalized_option_value(Some("   ")), None);
        assert_eq!(parser().normalized_option_value(None), None);
    }

    #[test]
    fn tab_target() {
        assert_eq!(
            parser().parse_mutation_tab_target(&HashMap::new()).target,
            Some(SidebarMutationTabTarget::Selected)
        );

        let uuid = sample_uuid();
        let by_uuid = parser().parse_mutation_tab_target(&opts(&[("tab", &uuid.to_string())]));
        assert_eq!(by_uuid.target, Some(SidebarMutationTabTarget::Workspace(uuid)));
        assert_eq!(by_uuid.error, None);

        let by_index = parser().parse_mutation_tab_target(&opts(&[("tab", "3")]));
        assert_eq!(by_index.target, Some(SidebarMutationTabTarget::Index(3)));

        let empty = parser().parse_mutation_tab_target(&opts(&[("tab", "  ")]));
        assert_eq!(empty.target, None);
        assert_eq!(empty.error, Some("ERROR: Tab not found".to_string()));

        let bad = parser().parse_mutation_tab_target(&opts(&[("tab", "nope")]));
        assert_eq!(bad.target, None);
        assert_eq!(bad.error, Some("ERROR: Tab not found".to_string()));

        let negative = parser().parse_mutation_tab_target(&opts(&[("tab", "-1")]));
        assert_eq!(negative.target, None);
        assert_eq!(negative.error, Some("ERROR: Tab not found".to_string()));
    }

    #[test]
    fn optional_panel_id() {
        let usage = "USAGE";
        assert_eq!(
            parser()
                .parse_optional_panel_id(&HashMap::new(), usage)
                .panel_id,
            None
        );
        assert_eq!(
            parser().parse_optional_panel_id(&HashMap::new(), usage).error,
            None
        );

        let uuid = sample_uuid();
        let by_panel =
            parser().parse_optional_panel_id(&opts(&[("panel", &uuid.to_string())]), usage);
        assert_eq!(by_panel.panel_id, Some(uuid));

        let by_surface =
            parser().parse_optional_panel_id(&opts(&[("surface", &uuid.to_string())]), usage);
        assert_eq!(by_surface.panel_id, Some(uuid));

        let empty_panel = parser().parse_optional_panel_id(&opts(&[("panel", "  ")]), usage);
        assert_eq!(empty_panel.panel_id, None);
        assert_eq!(
            empty_panel.error,
            Some("ERROR: Missing panel id — usage: USAGE".to_string())
        );

        let bad_panel = parser().parse_optional_panel_id(&opts(&[("panel", "nope")]), usage);
        assert_eq!(bad_panel.panel_id, None);
        assert_eq!(
            bad_panel.error,
            Some("ERROR: Invalid panel id 'nope'".to_string())
        );
    }

    #[test]
    fn split_block() {
        let none = parser().split_metadata_block_args("--a 1");
        assert_eq!(none.0, "--a 1");
        assert_eq!(none.1, None);

        let split = parser().split_metadata_block_args("--a 1 -- body -- more");
        assert_eq!(split.0, "--a 1");
        assert_eq!(split.1, Some("body -- more".to_string()));
    }
}
