//! Port of `Search/CommandPaletteSwitcherSearchIndexer.swift` and
//! `Search/CommandPaletteSwitcherSearchMetadata.swift`.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

/// Metadata delimiter set: `CharacterSet(charactersIn: "/\\.:_- ")`.
const METADATA_DELIMITERS: [char; 7] = ['/', '\\', '.', ':', '_', '-', ' '];

fn is_metadata_delimiter(character: char) -> bool {
    METADATA_DELIMITERS.contains(&character)
}

/// How much metadata detail to tokenize: workspaces index whole values, surfaces
/// additionally index path/branch components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataDetail {
    /// Workspace-level detail (whole values only).
    Workspace,
    /// Surface-level detail (whole values plus components).
    Surface,
}

/// Searchable workspace/surface metadata (directories, git branches, ports, and
/// the user description) feeding the switcher search corpus.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandPaletteSwitcherSearchMetadata {
    /// Working directories associated with the workspace or surface.
    pub directories: Vec<String>,
    /// Git branches associated with the workspace or surface.
    pub branches: Vec<String>,
    /// Listening ports associated with the workspace or surface.
    pub ports: Vec<i64>,
    /// Optional user-provided description.
    pub description: Option<String>,
}

impl CommandPaletteSwitcherSearchMetadata {
    /// Creates metadata; all fields default to empty.
    pub fn new(
        directories: Vec<String>,
        branches: Vec<String>,
        ports: Vec<i64>,
        description: Option<String>,
    ) -> Self {
        Self {
            directories,
            branches,
            ports,
            description,
        }
    }

    /// Feeds the metadata into `hasher` for switcher change detection (order-
    /// and count-sensitive). Mirrors Swift `combine(into:)`, including its
    /// `description ?? ""` collapse of a missing description.
    pub fn combine<H: Hasher>(&self, hasher: &mut H) {
        self.directories.len().hash(hasher);
        for directory in &self.directories {
            directory.hash(hasher);
        }
        self.branches.len().hash(hasher);
        for branch in &self.branches {
            branch.hash(hasher);
        }
        self.ports.len().hash(hasher);
        for port in &self.ports {
            port.hash(hasher);
        }
        self.description.as_deref().unwrap_or("").hash(hasher);
    }
}

/// Derives normalized, de-duplicated search keywords for one switcher entry from
/// base keywords plus workspace/surface metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteSwitcherSearchIndexer {
    /// Base keywords supplied for the entry.
    pub base_keywords: Vec<String>,
    /// Workspace/surface metadata to tokenize.
    pub metadata: CommandPaletteSwitcherSearchMetadata,
    /// How much metadata detail to tokenize.
    pub detail: MetadataDetail,
}

impl CommandPaletteSwitcherSearchIndexer {
    /// Captures the inputs for one switcher entry's keyword derivation. Swift's
    /// `detail` defaults to `.surface`.
    pub fn new(
        base_keywords: Vec<String>,
        metadata: CommandPaletteSwitcherSearchMetadata,
        detail: MetadataDetail,
    ) -> Self {
        Self {
            base_keywords,
            metadata,
            detail,
        }
    }

    /// The unique, order-preserving keyword list for the entry.
    pub fn keywords(&self) -> Vec<String> {
        let metadata_keywords = Self::metadata_keywords_for_search(&self.metadata, self.detail);
        let mut combined = self.base_keywords.clone();
        combined.extend(metadata_keywords);
        unique_normalized_preserving_order(combined)
    }

    fn metadata_keywords_for_search(
        metadata: &CommandPaletteSwitcherSearchMetadata,
        detail: MetadataDetail,
    ) -> Vec<String> {
        let directory_tokens: Vec<String> = metadata
            .directories
            .iter()
            .flat_map(|directory| directory_tokens_for_search(directory, detail))
            .collect();
        let branch_tokens: Vec<String> = metadata
            .branches
            .iter()
            .flat_map(|branch| branch_tokens_for_search(branch, detail))
            .collect();
        let port_tokens: Vec<String> = metadata
            .ports
            .iter()
            .flat_map(|&port| port_tokens_for_search(port))
            .collect();
        let description_tokens = description_tokens_for_search(metadata.description.as_deref());

        let mut context_keywords: Vec<String> = Vec::new();
        if !directory_tokens.is_empty() {
            context_keywords.extend(["directory", "dir", "cwd", "path"].map(String::from));
        }
        if !branch_tokens.is_empty() {
            context_keywords.extend(["branch", "git"].map(String::from));
        }
        if !port_tokens.is_empty() {
            context_keywords.extend(["port", "ports"].map(String::from));
        }
        if !description_tokens.is_empty() {
            context_keywords
                .extend(["description", "descriptions", "notes", "note"].map(String::from));
        }

        let mut result = context_keywords;
        result.extend(directory_tokens);
        result.extend(branch_tokens);
        result.extend(port_tokens);
        result.extend(description_tokens);
        result
    }
}

fn directory_tokens_for_search(raw_directory: &str, detail: MetadataDetail) -> Vec<String> {
    let trimmed = trim_whitespace_and_newlines(raw_directory);
    if trimmed.is_empty() {
        return Vec::new();
    }

    // DIVERGENCE: Swift uses `(trimmed as NSString).standardizingPath` and
    // `abbreviatingWithTildeInPath`, both of which consult the current user's
    // `$HOME` and resolve symlinks/relative components against the filesystem.
    // Those are neither deterministic nor portable, so this port performs a
    // purely lexical standardization (resolve `.`/`..`, collapse separators,
    // drop a trailing separator) and treats tilde-abbreviation as the identity.
    let standardized = standardizing_path_lexical(trimmed);
    let canonical: &str = if standardized.is_empty() {
        trimmed
    } else {
        &standardized
    };
    let abbreviated = abbreviating_with_tilde_lexical(canonical);

    match detail {
        MetadataDetail::Workspace => unique_normalized_preserving_order(vec![
            trimmed.to_string(),
            canonical.to_string(),
            abbreviated.clone(),
        ]),
        MetadataDetail::Surface => {
            let basename = last_path_component(canonical);
            let components = split_on_delimiters(canonical);
            let mut values = vec![
                trimmed.to_string(),
                canonical.to_string(),
                abbreviated,
                basename,
            ];
            values.extend(components);
            unique_normalized_preserving_order(values)
        }
    }
}

fn branch_tokens_for_search(raw_branch: &str, detail: MetadataDetail) -> Vec<String> {
    let trimmed = trim_whitespace_and_newlines(raw_branch);
    if trimmed.is_empty() {
        return Vec::new();
    }
    match detail {
        MetadataDetail::Workspace => vec![trimmed.to_string()],
        MetadataDetail::Surface => {
            let mut values = vec![trimmed.to_string()];
            values.extend(split_on_delimiters(trimmed));
            unique_normalized_preserving_order(values)
        }
    }
}

fn port_tokens_for_search(port: i64) -> Vec<String> {
    if !(1..=65_535).contains(&port) {
        return Vec::new();
    }
    let port_text = port.to_string();
    vec![port_text.clone(), format!(":{port_text}")]
}

fn description_tokens_for_search(raw_description: Option<&str>) -> Vec<String> {
    let trimmed = raw_description
        .map(trim_whitespace_and_newlines)
        .unwrap_or("");
    if trimmed.is_empty() {
        return Vec::new();
    }
    // Swift: `replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)`.
    // DIVERGENCE: `split_whitespace` collapses Unicode whitespace runs, matching
    // the intent of `\s+` -> " " on the already-trimmed text without a regex
    // dependency.
    let normalized_whitespace = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let components = split_on_delimiters(&normalized_whitespace);
    let mut values = vec![trimmed.to_string(), normalized_whitespace];
    values.extend(components);
    unique_normalized_preserving_order(values)
}

/// Swift: `uniqueNormalizedPreservingOrder(_:)`.
///
/// DIVERGENCE: the dedup key is `trimmed.folding([.diacriticInsensitive,
/// .caseInsensitive], locale: .current).lowercased()`; this port folds with
/// plain Unicode lowercasing only (no diacritic stripping, no locale rules),
/// consistent with `cmux_mentions`'s `normalizeForSearch`.
fn unique_normalized_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(values.len());
    let mut seen: HashSet<String> = HashSet::new();

    for value in values {
        let trimmed = trim_whitespace_and_newlines(&value);
        if trimmed.is_empty() {
            continue;
        }
        let normalized_key: String = trimmed.chars().flat_map(char::to_lowercase).collect();
        if seen.insert(normalized_key) {
            result.push(trimmed.to_string());
        }
    }
    result
}

fn split_on_delimiters(text: &str) -> Vec<String> {
    text.split(is_metadata_delimiter)
        .filter(|component| !component.is_empty())
        .map(String::from)
        .collect()
}

/// Swift trims `.whitespacesAndNewlines`.
///
/// DIVERGENCE: `char::is_whitespace` covers Unicode spaces plus newlines/tabs,
/// approximating `.whitespacesAndNewlines`.
fn trim_whitespace_and_newlines(text: &str) -> &str {
    text.trim_matches(char::is_whitespace)
}

/// Lexical stand-in for `NSString.standardizingPath` (see the DIVERGENCE note in
/// [`directory_tokens_for_search`]). Resolves `.` and `..` components and
/// collapses redundant separators without touching the filesystem or `$HOME`.
fn standardizing_path_lexical(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut components: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if matches!(components.last(), Some(&last) if last != "..") {
                    components.pop();
                } else if !is_absolute {
                    components.push("..");
                }
            }
            other => components.push(other),
        }
    }
    let joined = components.join("/");
    if is_absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        // `standardizingPath` maps an all-`.`/empty relative path to itself;
        // fall back to the original so the caller's empty-check picks `trimmed`.
        path.to_string()
    } else {
        joined
    }
}

/// Lexical stand-in for `NSString.abbreviatingWithTildeInPath` — identity, since
/// there is no portable `$HOME` to abbreviate against (see the DIVERGENCE note
/// in [`directory_tokens_for_search`]).
fn abbreviating_with_tilde_lexical(path: &str) -> String {
    path.to_string()
}

/// Swift: `URL(fileURLWithPath: canonical, isDirectory: true).lastPathComponent`.
///
/// DIVERGENCE: computed lexically as the final non-empty `/`-separated
/// component; a path that is only separators yields the canonical string.
fn last_path_component(path: &str) -> String {
    path.split('/')
        .rfind(|component| !component.is_empty())
        .map(String::from)
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_detail_indexes_whole_values_and_context_keywords() {
        let indexer = CommandPaletteSwitcherSearchIndexer::new(
            vec!["workspace".to_string(), "switch".to_string()],
            CommandPaletteSwitcherSearchMetadata::new(
                vec!["/Users/example/dev/proj".to_string()],
                vec!["feature/rename".to_string()],
                vec![3000],
                None,
            ),
            MetadataDetail::Workspace,
        );
        let keywords = indexer.keywords();

        assert_eq!(
            keywords,
            vec![
                "workspace".to_string(),
                "switch".to_string(),
                // directory context keywords
                "directory".to_string(),
                "dir".to_string(),
                "cwd".to_string(),
                "path".to_string(),
                // branch context keywords
                "branch".to_string(),
                "git".to_string(),
                // port context keywords
                "port".to_string(),
                "ports".to_string(),
                // whole directory (canonical == trimmed, abbreviated == canonical)
                "/Users/example/dev/proj".to_string(),
                // whole branch
                "feature/rename".to_string(),
                // ports
                "3000".to_string(),
                ":3000".to_string(),
            ]
        );
    }

    #[test]
    fn surface_detail_adds_path_and_branch_components() {
        let indexer = CommandPaletteSwitcherSearchIndexer::new(
            Vec::new(),
            CommandPaletteSwitcherSearchMetadata::new(
                vec!["/Users/example/dev/proj".to_string()],
                vec!["feature/rename-tab".to_string()],
                Vec::new(),
                None,
            ),
            MetadataDetail::Surface,
        );
        let keywords = indexer.keywords();

        // Directory basename + components appear; branch splits on delimiters.
        assert!(keywords.contains(&"proj".to_string()));
        // Original casing is preserved; only the dedup key is folded.
        assert!(keywords.contains(&"Users".to_string()));
        assert!(keywords.contains(&"example".to_string()));
        assert!(keywords.contains(&"dev".to_string()));
        assert!(keywords.contains(&"feature".to_string()));
        assert!(keywords.contains(&"rename".to_string()));
        assert!(keywords.contains(&"tab".to_string()));
        // Whole values retained too.
        assert!(keywords.contains(&"/Users/example/dev/proj".to_string()));
        assert!(keywords.contains(&"feature/rename-tab".to_string()));
    }

    #[test]
    fn keywords_are_unique_normalized_preserving_first_occurrence() {
        let indexer = CommandPaletteSwitcherSearchIndexer::new(
            vec!["Workspace".to_string(), "workspace".to_string()],
            CommandPaletteSwitcherSearchMetadata::default(),
            MetadataDetail::Workspace,
        );
        // "Workspace" and "workspace" fold to the same key; first wins.
        assert_eq!(indexer.keywords(), vec!["Workspace".to_string()]);
    }

    #[test]
    fn description_tokens_collapse_whitespace_and_split() {
        let indexer = CommandPaletteSwitcherSearchIndexer::new(
            Vec::new(),
            CommandPaletteSwitcherSearchMetadata::new(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some("  hello   world-notes  ".to_string()),
            ),
            MetadataDetail::Surface,
        );
        let keywords = indexer.keywords();
        assert!(keywords.contains(&"description".to_string()));
        assert!(keywords.contains(&"hello world-notes".to_string()));
        assert!(keywords.contains(&"hello".to_string()));
        assert!(keywords.contains(&"world".to_string()));
        assert!(keywords.contains(&"notes".to_string()));
    }

    #[test]
    fn out_of_range_ports_are_dropped() {
        assert!(port_tokens_for_search(0).is_empty());
        assert!(port_tokens_for_search(70_000).is_empty());
        assert_eq!(
            port_tokens_for_search(8080),
            vec!["8080".to_string(), ":8080".to_string()]
        );
    }

    #[test]
    fn standardizing_path_resolves_dot_components() {
        assert_eq!(standardizing_path_lexical("/a/b/../c"), "/a/c");
        assert_eq!(standardizing_path_lexical("/a/./b/"), "/a/b");
        assert_eq!(standardizing_path_lexical("/a/b/proj"), "/a/b/proj");
    }
}
