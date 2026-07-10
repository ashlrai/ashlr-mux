//! Port of the PURE parts of `Sources/TextBoxMentionIndexStore.swift`.
//!
//! The Swift file is an actor mixing filesystem indexing/dispatch with pure
//! filtering/ranking/path-normalization logic. Ported here: the constants,
//! skip rules, priority formulas, candidate builders, deterministic sorting,
//! SKILL.md name parsing, path helpers, and the TTL/LRU file-index cache
//! state machine.
//!
//! # Scope note — NOT ported (host I/O, stays behind the daemon/host layer)
//!
//! - `suggestions(for:rootDirectory:)` / `warmIndexes` / `fileSuggestions`
//!   (actor entry points that orchestrate scans against live queries).
//! - `scanFiles` / `scanRootFileSystemCandidates` / `scanFilesWithRipgrep` /
//!   `scanDirectoryCandidateSeed` / `childDirectoryURLs` (FileManager
//!   enumeration + ripgrep subprocess streaming).
//! - `isGitWorkTree` / `gitIgnoredRelativePaths` (git subprocesses).
//! - `scanSkillFiles` / `skillSearchRoots` / `pluginSkillRoots` /
//!   `isDirectory` (filesystem discovery of SKILL.md files and skill roots
//!   under the project ancestry, `~/.codex`, `~/.agents`, plugin caches).
//! - `normalizedDirectory` (tilde expansion + `fileExists` probe).
//! - The refresh-task coalescing (`fileIndexRefreshTask` /
//!   `refreshFileIndexInBackground`, `TextBoxMentionFileIndexRefreshTask`
//!   task IDs and Swift-concurrency `Task` joins). [`FileIndexCache`] ports
//!   the pure cache half of that machinery: TTL expiry, LRU pruning, and the
//!   stale-refresh guard (`refreshStartedAt` comparison).
//!
//! Paths use "/" separators exactly as the canonical Swift (file URLs). The
//! Windows host normalizes to "/" before calling in.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::cached_index::MentionCachedIndex;
use crate::candidate::MentionCandidate;
use crate::candidate_index::MentionCandidateIndex;
use crate::compare::localized_standard_compare;

pub const FILE_INDEX_TTL_SECONDS: f64 = 30.0;
pub const MAX_CACHED_FILE_INDEXES: usize = 8;
pub const DIRECTORY_SEED_BATCH_SIZE: usize = 128;
pub const MAX_INDEXED_DIRECTORIES: usize = 2000;
pub const MAX_INDEXED_FILES: usize = 6000;
pub const MAX_INDEXED_SKILLS: usize = 800;
pub const ROOT_SUGGESTION_LIMIT: usize = 200;
pub const SUGGESTION_LIMIT: usize = 500;

/// Swift: `skippedDirectoryNames` (a `Set<String>`; membership is
/// case-SENSITIVE — "library" is intentionally not skipped while "Library" is,
/// per `testTextBoxMentionFileSuggestionsKeepCaseVariantProjectDirectories`).
pub const SKIPPED_DIRECTORY_NAMES: [&str; 10] = [
    ".build",
    ".git",
    ".next",
    ".swiftpm",
    ".vercel",
    "DerivedData",
    "Library",
    "node_modules",
    "Pods",
    "vendor",
];

/// Swift: `skippedPackageDirectorySuffixes` (compared case-insensitively).
pub const SKIPPED_PACKAGE_DIRECTORY_SUFFIXES: [&str; 15] = [
    ".app",
    ".appex",
    ".bundle",
    ".dSYM",
    ".framework",
    ".kext",
    ".mdimporter",
    ".plugin",
    ".prefPane",
    ".qlgenerator",
    ".rtfd",
    ".xcframework",
    ".xcodeproj",
    ".xcworkspace",
    ".playground",
];

/// Swift: `shouldSkipIndexedDirectoryName(_:)`.
pub fn should_skip_indexed_directory_name(name: &str) -> bool {
    if SKIPPED_DIRECTORY_NAMES.contains(&name) {
        return true;
    }
    let normalized_name = name.to_lowercase();
    SKIPPED_PACKAGE_DIRECTORY_SUFFIXES
        .iter()
        .any(|suffix| normalized_name.ends_with(&suffix.to_lowercase()))
}

/// Swift: `directoryPriority(relativePath:)`.
pub fn directory_priority(relative_path: &str) -> i64 {
    let depth = path_depth(relative_path);
    ((depth * 2) - 2).min(40)
}

/// Swift: `filePriority(relativePath:)`.
pub fn file_priority(relative_path: &str) -> i64 {
    let depth = path_depth(relative_path);
    ((depth * 2) - 1).min(41)
}

fn path_depth(relative_path: &str) -> i64 {
    // Swift: `max(relativePath.split(separator: "/").count, 1)` —
    // `split(separator:)` omits empty subsequences.
    let component_count = relative_path
        .split('/')
        .filter(|component| !component.is_empty())
        .count() as i64;
    component_count.max(1)
}

/// Swift: `directoryCandidate(relativePath:directoryURL:)`. `directory_path`
/// is the absolute path (Swift `directoryURL.path`); `home` is the current
/// user's home directory path (injected in place of `FileManager`'s).
pub fn directory_candidate(
    relative_path: &str,
    directory_path: &str,
    home: &str,
) -> MentionCandidate {
    let normalized_path = relative_path.trim_matches('/');
    let display_title = format!("@{normalized_path}/");
    let directory_name = last_path_component(directory_path);
    MentionCandidate {
        title: display_title,
        subtitle: display_path(directory_path, home),
        target_path: directory_path.to_string(),
        system_image_name: "folder".to_string(),
        search_key: format!("{normalized_path} {directory_name} folder directory").to_lowercase(),
        priority: directory_priority(normalized_path),
    }
}

/// Swift: `fileCandidate(relativePath:fileURL:fileName:)`.
pub fn file_candidate(
    relative_path: &str,
    file_path: &str,
    file_name: &str,
    home: &str,
) -> MentionCandidate {
    MentionCandidate {
        title: format!("@{relative_path}"),
        subtitle: display_path(file_path, home),
        target_path: file_path.to_string(),
        system_image_name: "doc".to_string(),
        search_key: format!("{relative_path} {file_name}").to_lowercase(),
        priority: file_priority(relative_path),
    }
}

/// The pure candidate construction from the Swift `skillIndex` loop body:
/// title `/<name>`, subtitle/display path, sparkle icon, root-order priority.
pub fn skill_candidate(
    skill_name: &str,
    skill_file_path: &str,
    search_key: String,
    priority: i64,
    home: &str,
) -> MentionCandidate {
    MentionCandidate {
        title: format!("/{skill_name}"),
        subtitle: display_path(skill_file_path, home),
        target_path: skill_file_path.to_string(),
        system_image_name: "sparkle.magnifyingglass".to_string(),
        search_key,
        priority,
    }
}

/// Swift: `sortedFileSystemCandidates(_:)` — priority ascending, then
/// Finder-like title order.
pub fn sorted_file_system_candidates(
    mut candidates: Vec<MentionCandidate>,
) -> Vec<MentionCandidate> {
    candidates.sort_by(|lhs, rhs| {
        lhs.priority
            .cmp(&rhs.priority)
            .then_with(|| localized_standard_compare(&lhs.title, &rhs.title))
    });
    candidates
}

/// Swift: `skillName(from:)`, split into its pure half — the caller supplies
/// the SKILL.md file content and the fallback (Swift uses the skill
/// directory's name when the file is unreadable or has no `name:` line).
pub fn skill_name_from_content(content: &str, fallback: &str) -> String {
    // Swift: `split(separator: "\n", maxSplits: 32, omittingEmptySubsequences:
    // false)` — at most 33 pieces, the last carrying the remainder.
    for line in content.splitn(33, '\n') {
        let trimmed = line.trim_matches(char::is_whitespace);
        let Some(rest) = trimmed.strip_prefix("name:") else {
            continue;
        };
        let name = rest.trim_matches(char::is_whitespace);
        if !name.is_empty() {
            return name
                .trim_matches(|character| character == '"' || character == '\'')
                .to_string();
        }
    }
    fallback.to_string()
}

/// Swift: `skillSearchKey(skillName:skillURL:rootURL:)` — the caller passes
/// the skill's directory path (Swift `skillURL.deletingLastPathComponent()`)
/// and the root path.
pub fn skill_search_key(skill_name: &str, skill_directory_path: &str, root_path: &str) -> String {
    let relative_skill_path = relative_path(skill_directory_path, root_path);
    format!("{skill_name} {relative_skill_path}").to_lowercase()
}

/// Swift: `relativePath(for:rootPath:)`.
pub fn relative_path(path: &str, root_path: &str) -> String {
    // DIVERGENCE: Swift `hasPrefix` + character offset compares with
    // canonical (grapheme) equivalence; this byte-wise strip_prefix matches
    // only exact encodings. Differs when the host mixes NFC/NFD path forms.
    let Some(remainder) = path.strip_prefix(root_path) else {
        return path.to_string();
    };
    let relative = remainder.trim_matches('/');
    if relative.is_empty() {
        last_path_component(path)
    } else {
        relative.to_string()
    }
}

/// Swift: `displayPath(_:)` — `home` injected in place of
/// `FileManager.default.homeDirectoryForCurrentUser.path`.
///
/// DIVERGENCE: byte-wise prefix strip vs Swift's canonical-equivalence
/// `hasPrefix` + `dropFirst(home.count)` (see [`relative_path`]).
pub fn display_path(path: &str, home: &str) -> String {
    let Some(remainder) = path.strip_prefix(home) else {
        return path.to_string();
    };
    format!("~{remainder}")
}

/// `URL(fileURLWithPath:).lastPathComponent` analog for "/"-separated paths.
pub fn last_path_component(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        // `URL(fileURLWithPath: "/").lastPathComponent == "/"`.
        return "/".to_string();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

/// The pure cache half of the Swift actor's `fileIndexesByRoot` handling:
/// `cachedFileIndex` (TTL check + access-time touch), `storeFileIndex`
/// (stale-refresh guard), and `pruneFileIndexCache` (TTL expiry + LRU cap).
/// The host supplies `now`/`stored_at` as seconds timestamps.
#[derive(Debug, Clone, Default)]
pub struct FileIndexCache {
    file_indexes_by_root: HashMap<String, MentionCachedIndex>,
}

impl FileIndexCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.file_indexes_by_root.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file_indexes_by_root.is_empty()
    }

    /// Swift: `cachedFileIndex(rootDirectory:now:)`.
    pub fn cached_file_index(
        &mut self,
        root_directory: &str,
        now: f64,
    ) -> Option<MentionCandidateIndex> {
        let Some(mut cached) = self.file_indexes_by_root.remove(root_directory) else {
            self.prune_file_index_cache(now);
            return None;
        };
        if now - cached.created_at >= FILE_INDEX_TTL_SECONDS {
            // Entry already removed above (Swift assigns nil).
            self.prune_file_index_cache(now);
            return None;
        }
        cached.last_accessed_at = now;
        let index = cached.index.clone();
        self.file_indexes_by_root
            .insert(root_directory.to_string(), cached);
        self.prune_file_index_cache(now);
        Some(index)
    }

    /// Swift: `storeFileIndex(rootDirectory:index:refreshStartedAt:refreshTaskID:)`.
    /// The refresh-task-ID bookkeeping is not ported (scope note above);
    /// `stored_at` stands in for Swift's `Date()` capture.
    pub fn store_file_index(
        &mut self,
        root_directory: &str,
        index: MentionCandidateIndex,
        refresh_started_at: f64,
        stored_at: f64,
    ) {
        if let Some(cached) = self.file_indexes_by_root.get(root_directory) {
            if cached.refresh_started_at > refresh_started_at {
                return;
            }
        }
        self.file_indexes_by_root.insert(
            root_directory.to_string(),
            MentionCachedIndex {
                index,
                created_at: stored_at,
                last_accessed_at: stored_at,
                refresh_started_at,
            },
        );
        self.prune_file_index_cache(stored_at);
    }

    /// Swift: `pruneFileIndexCache(now:)`.
    fn prune_file_index_cache(&mut self, now: f64) {
        let expired_roots: Vec<String> = self
            .file_indexes_by_root
            .iter()
            .filter(|(_, cached)| now - cached.created_at >= FILE_INDEX_TTL_SECONDS)
            .map(|(root_directory, _)| root_directory.clone())
            .collect();
        for root_directory in expired_roots {
            self.file_indexes_by_root.remove(&root_directory);
        }

        if self.file_indexes_by_root.len() <= MAX_CACHED_FILE_INDEXES {
            return;
        }
        let mut roots: Vec<(String, f64)> = self
            .file_indexes_by_root
            .iter()
            .map(|(root_directory, cached)| (root_directory.clone(), cached.last_accessed_at))
            .collect();
        roots.sort_by(|lhs, rhs| {
            lhs.1
                .partial_cmp(&rhs.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| lhs.0.cmp(&rhs.0))
        });
        let excess = self.file_indexes_by_root.len() - MAX_CACHED_FILE_INDEXES;
        for (root_directory, _) in roots.into_iter().take(excess) {
            self.file_indexes_by_root.remove(&root_directory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_swift() {
        assert_eq!(FILE_INDEX_TTL_SECONDS, 30.0);
        assert_eq!(MAX_CACHED_FILE_INDEXES, 8);
        assert_eq!(DIRECTORY_SEED_BATCH_SIZE, 128);
        assert_eq!(MAX_INDEXED_DIRECTORIES, 2000);
        assert_eq!(MAX_INDEXED_FILES, 6000);
        assert_eq!(MAX_INDEXED_SKILLS, 800);
        assert_eq!(ROOT_SUGGESTION_LIMIT, 200);
        assert_eq!(SUGGESTION_LIMIT, 500);
    }

    // Oracle behavior: `testTextBoxMentionFileSuggestionsSkipPackageContents`
    // (GhosttyKit.xcframework skipped) and
    // `testTextBoxMentionFileSuggestionsKeepCaseVariantProjectDirectories`
    // ("library" lowercase must survive).
    #[test]
    fn skip_rules_match_swift() {
        assert!(should_skip_indexed_directory_name("GhosttyKit.xcframework"));
        assert!(should_skip_indexed_directory_name("node_modules"));
        assert!(should_skip_indexed_directory_name("Library"));
        assert!(!should_skip_indexed_directory_name("library"));
        assert!(should_skip_indexed_directory_name("My.APP")); // suffix is case-insensitive
        assert!(should_skip_indexed_directory_name("Demo.playground"));
        assert!(!should_skip_indexed_directory_name("Sources"));
    }

    #[test]
    fn priorities_match_swift_formulas() {
        assert_eq!(directory_priority("Sources"), 0);
        assert_eq!(file_priority("README.md"), 1);
        assert_eq!(directory_priority("Sources/Components"), 2);
        assert_eq!(file_priority("Sources/Components/NestedView.swift"), 5);
        assert_eq!(directory_priority(""), 0); // depth clamps to 1
                                               // Caps at 40 / 41.
        let deep: String = vec!["d"; 64].join("/");
        assert_eq!(directory_priority(&deep), 40);
        assert_eq!(file_priority(&deep), 41);
    }

    #[test]
    fn directory_candidate_matches_swift_shape() {
        let candidate = directory_candidate("Sources/", "/home/u/proj/Sources", "/home/u");
        assert_eq!(candidate.title, "@Sources/");
        assert_eq!(candidate.subtitle, "~/proj/Sources");
        assert_eq!(candidate.target_path, "/home/u/proj/Sources");
        assert_eq!(candidate.system_image_name, "folder");
        assert_eq!(candidate.search_key, "sources sources folder directory");
        assert_eq!(candidate.priority, 0);
    }

    #[test]
    fn file_candidate_matches_swift_shape() {
        let candidate = file_candidate(
            "Sources/TextBoxInput.swift",
            "/home/u/proj/Sources/TextBoxInput.swift",
            "TextBoxInput.swift",
            "/home/u",
        );
        assert_eq!(candidate.title, "@Sources/TextBoxInput.swift");
        assert_eq!(candidate.system_image_name, "doc");
        assert_eq!(
            candidate.search_key,
            "sources/textboxinput.swift textboxinput.swift"
        );
        assert_eq!(candidate.priority, 3);
    }

    // Oracle: SKILL.md contents written by the Swift suite, e.g.
    // "name: sample-dollar-skill\n".
    #[test]
    fn skill_name_parses_name_line() {
        assert_eq!(
            skill_name_from_content("name: sample-dollar-skill\n", "dir-name"),
            "sample-dollar-skill"
        );
        assert_eq!(
            skill_name_from_content("---\nname: \"quoted-skill\"\n---\nbody", "dir-name"),
            "quoted-skill"
        );
        assert_eq!(
            skill_name_from_content("no front matter", "dir-name"),
            "dir-name"
        );
        // An empty `name:` value falls through to the fallback.
        assert_eq!(skill_name_from_content("name:\n", "dir-name"), "dir-name");
        // Swift splits with `maxSplits: 32`; a name line past the 33rd piece
        // is only found if the trimmed remainder happens to START with
        // "name:". Blank padding trims away (found)...
        let padded_name = format!("{}name: still-found\n", "\n".repeat(40));
        assert_eq!(
            skill_name_from_content(&padded_name, "dir-name"),
            "still-found"
        );
        // ...but content before it in the remainder hides it (fallback).
        let buried_name = format!(
            "{}\nname: too-late\n",
            (0..40)
                .map(|i| format!("x{i}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(
            skill_name_from_content(&buried_name, "dir-name"),
            "dir-name"
        );
    }

    #[test]
    fn skill_search_key_lowercases_name_and_relative_path() {
        assert_eq!(
            skill_search_key(
                "Nested-Skill",
                "/root/skills/Team/nested-skill",
                "/root/skills"
            ),
            "nested-skill team/nested-skill"
        );
    }

    #[test]
    fn relative_path_matches_swift() {
        assert_eq!(relative_path("/root/a/b.txt", "/root"), "a/b.txt");
        assert_eq!(relative_path("/elsewhere/a", "/root"), "/elsewhere/a");
        // Prefix-equal path degrades to the last component.
        assert_eq!(relative_path("/root", "/root"), "root");
    }

    #[test]
    fn display_path_substitutes_home() {
        assert_eq!(display_path("/home/u/x", "/home/u"), "~/x");
        assert_eq!(display_path("/opt/x", "/home/u"), "/opt/x");
    }

    #[test]
    fn sorted_candidates_order_by_priority_then_natural_title() {
        let sorted = sorted_file_system_candidates(vec![
            file_candidate("b10.txt", "/r/b10.txt", "b10.txt", "/h"),
            file_candidate("b2.txt", "/r/b2.txt", "b2.txt", "/h"),
            directory_candidate("Sources", "/r/Sources", "/h"),
        ]);
        assert_eq!(
            sorted.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            vec!["@Sources/", "@b2.txt", "@b10.txt"]
        );
    }

    fn empty_index() -> MentionCandidateIndex {
        MentionCandidateIndex::new(Vec::new())
    }

    #[test]
    fn cache_expires_after_ttl() {
        let mut cache = FileIndexCache::new();
        cache.store_file_index("/root", empty_index(), 0.0, 0.0);
        assert!(cache.cached_file_index("/root", 29.0).is_some());
        assert!(cache.cached_file_index("/root", 30.0).is_none());
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_keeps_newer_refresh_over_stale_store() {
        let mut cache = FileIndexCache::new();
        cache.store_file_index("/root", empty_index(), 10.0, 10.0);
        // A stale refresh (started earlier) must not clobber the newer entry.
        cache.store_file_index("/root", empty_index(), 5.0, 11.0);
        assert!(cache.cached_file_index("/root", 41.0).is_none()); // created_at stayed 10.0
        let mut cache = FileIndexCache::new();
        cache.store_file_index("/root", empty_index(), 10.0, 10.0);
        cache.store_file_index("/root", empty_index(), 12.0, 12.0);
        assert!(cache.cached_file_index("/root", 41.0).is_some()); // created_at moved to 12.0
    }

    #[test]
    fn cache_prunes_least_recently_accessed_beyond_cap() {
        let mut cache = FileIndexCache::new();
        for slot in 0..=MAX_CACHED_FILE_INDEXES {
            cache.store_file_index(
                &format!("/root{slot}"),
                empty_index(),
                slot as f64,
                slot as f64,
            );
        }
        assert_eq!(cache.len(), MAX_CACHED_FILE_INDEXES);
        // "/root0" had the oldest lastAccessedAt and was evicted.
        assert!(cache.cached_file_index("/root0", 9.0).is_none());
        assert!(cache.cached_file_index("/root1", 9.0).is_some());
    }
}
