//! Pure git repository-resolution path logic.
//!
//! Headless port of the *pure* string/path cores extracted from
//! `GitMetadataService+RepositoryResolution.swift`
//! (`Packages/macOS/CmuxGit/Sources/CmuxGit/Parsing`). The filesystem shell of
//! `resolveGitRepository(containing:)` — the upward `deleteLastPathComponent`
//! walk, `FileManager.fileExists` dir-vs-file probing, and every
//! `String(contentsOf:)` read — stays OUT (host wiring). What lands here is the
//! four fs-free functions those reads feed:
//!
//! 1. [`should_stop_repository_search`] — the walk's termination predicate
//!    (`shouldStopGitRepositorySearch`, Swift :55-66).
//! 2. [`git_directory_from_dot_git_file`] — the `.git` *file* `gitdir:` pointer
//!    parser (`gitDirectoryFromDotGitFile`, Swift :70-92; the file read is the
//!    caller's job — this takes the already-read contents).
//! 3. [`git_common_directory`] — the `commondir` resolver
//!    (`gitCommonDirectory`, Swift :96-112; likewise takes read contents).
//! 4. [`standardize_posix_path`] — the KEYSTONE: a hand-rolled, lexical,
//!    forward-slash-only port of Foundation `URL(fileURLWithPath:)
//!    .standardizedFileURL.path` for POSIX. Also [`ResolvedGitRepository`].
//!
//! Plus an optional secondary slice (reusing [`crate::git_config_glob_matches`]):
//! the `includeIf "gitdir:"` matcher (`GitMetadataService+Config.swift`
//! :336-434) with the home directory *injected* — [`expanded_pattern`],
//! [`gitdir_pattern_matches`], [`include_if_condition_matches`], and the
//! `[includeIf "…"]` header parser [`git_config_include_if_condition`]. The
//! `onbranch:` arm and the `gitConfigIncludeURL` fs-walk are OUT of scope.
//!
//! WINDOWS / PARITY: git's on-disk pointer files (`.git`, `commondir`) are POSIX
//! forward-slash regardless of host, and the macOS oracle is a POSIX
//! `standardizedFileURL`. Per the canonical-fidelity directive this module keeps
//! pure forward-slash semantics and does NOT reach for `std::path` /
//! `canonicalize` (which would inject Windows `\`/drive handling and touch the
//! filesystem). Any real fs operation lives in the excluded shell.

// ---------------------------------------------------------------------------
// ResolvedGitRepository — Model/ResolvedGitRepository.swift:16-35
// ---------------------------------------------------------------------------

/// The on-disk locations that define a single resolved git repository.
///
/// Port of `ResolvedGitRepository` (`Model/ResolvedGitRepository.swift:16-35`).
/// For a normal clone all three describe the same checkout; for a linked
/// worktree or submodule the `.git` entry is a *file*, so `git_directory` is the
/// per-worktree git dir while `common_directory` (read from `commondir`) is the
/// shared object/ref store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGitRepository {
    /// Absolute path to the working-tree root (the directory containing `.git`).
    pub work_tree_root: String,
    /// Absolute path to this checkout's git directory (the `.git` directory, or
    /// the directory a `.git` *file* points at for worktrees/submodules).
    pub git_directory: String,
    /// Absolute path to the shared common directory (`git_directory` for a
    /// normal clone; the `commondir` target for a linked worktree).
    pub common_directory: String,
}

impl ResolvedGitRepository {
    /// Creates a resolved repository from its three on-disk locations.
    pub fn new(
        work_tree_root: impl Into<String>,
        git_directory: impl Into<String>,
        common_directory: impl Into<String>,
    ) -> Self {
        Self {
            work_tree_root: work_tree_root.into(),
            git_directory: git_directory.into(),
            common_directory: common_directory.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// KEYSTONE: standardize_posix_path + path-join / delete-last-component helpers.
// ---------------------------------------------------------------------------

/// Lexically standardizes a POSIX path, matching Foundation
/// `URL(fileURLWithPath:).standardizedFileURL.path`.
///
/// This is purely lexical (no filesystem, no symlink resolution — that is
/// Foundation's separate `resolvingSymlinksInPath`): it collapses `.`
/// components, resolves `..` against the preceding component, squashes repeated
/// `/`, and strips a trailing `/` except at the root. A leading `/` is
/// preserved; a `..` that would escape the root is dropped (you cannot go above
/// `/`), so `standardize_posix_path("/..") == "/"`.
///
/// The root/`..` behavior is pinned to what the Swift termination oracle
/// requires (`rootVariantsStopRepositorySearch` feeds `/..` and `/../..`): on
/// macOS this is version-sensitive (`deletingLastPathComponent` of `/` was
/// `"/.."` on macOS 14/15 but `"/"` on macOS 26), and the search must terminate
/// regardless. Collapsing `/..` → `/` satisfies both the oracle and git.
pub fn standardize_posix_path(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();

    for component in path.split('/') {
        match component {
            // Empty (from `//` or leading/trailing `/`) and `.` are dropped.
            "" | "." => {}
            ".." => match out.last() {
                // Resolve against a real preceding component.
                Some(&last) if last != ".." => {
                    out.pop();
                }
                // At the root of an absolute path, `..` cannot escape → drop it.
                // For a relative path with nothing (or only `..`) to pop, keep it.
                _ => {
                    if !is_absolute {
                        out.push("..");
                    }
                }
            },
            other => out.push(other),
        }
    }

    let joined = out.join("/");
    if is_absolute {
        // Empty join at absolute → root "/".
        format!("/{joined}")
    } else if joined.is_empty() {
        // Foundation standardizes an all-collapsing relative path to ".".
        ".".to_string()
    } else {
        joined
    }
}

/// Joins `component` onto `base` then standardizes, matching Foundation
/// `base.appendingPathComponent(component).standardizedFileURL.path`.
///
/// `appendingPathComponent` inserts a single `/`; the subsequent standardize
/// collapses any resulting `//`, so a plain `"{base}/{component}"` join is
/// equivalent once normalized.
fn standardize_joining(base: &str, component: &str) -> String {
    standardize_posix_path(&format!("{base}/{component}"))
}

/// Removes the last path component, matching Foundation
/// `URL(fileURLWithPath:).deletingLastPathComponent()` (callers standardize the
/// result, as the Swift source does via `.standardizedFileURL.path`).
fn deleting_last_path_component(path: &str) -> String {
    let is_absolute = path.starts_with('/');
    let mut components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        // Root (or empty): nothing to delete. Standardize handles the rest.
        return path.to_string();
    }
    components.pop();
    let joined = components.join("/");
    if is_absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

// ---------------------------------------------------------------------------
// Fn 1 — shouldStopGitRepositorySearch (Swift :55-66).
// ---------------------------------------------------------------------------

/// Whether the upward repository search should stop at `current`.
///
/// Port of `shouldStopGitRepositorySearch(currentURL:parentURL:)`
/// (Swift :55-66). Stops when the parent no longer differs from the current
/// directory (raw), when the standardized current directory is the filesystem
/// root, or when the standardized parent equals the standardized current (so the
/// walk cannot loop forever). `current`/`parent` are the URLs' `.path` strings.
pub fn should_stop_repository_search(current: &str, parent: &str) -> bool {
    if parent == current {
        return true;
    }
    let standardized_current = standardize_posix_path(current);
    if standardized_current == "/" {
        return true;
    }
    standardize_posix_path(parent) == standardized_current
}

// ---------------------------------------------------------------------------
// Fn 2 — gitDirectoryFromDotGitFile (Swift :70-92).
// ---------------------------------------------------------------------------

/// Resolves the git directory a `.git` *file* points at via its `gitdir:` line.
///
/// Port of `gitDirectoryFromDotGitFile(_:relativeTo:)` (Swift :70-92). The
/// `String(contentsOf:)` read is the caller's job — this takes the already-read
/// `contents`. Trims surrounding whitespace/newlines, requires a
/// case-insensitive `gitdir:` prefix, then extracts the remaining path
/// verbatim: an absolute path is standardized directly, a relative path is
/// resolved against `work_tree_root`. Returns `None` when the prefix is absent
/// or the extracted path is empty.
pub fn git_directory_from_dot_git_file(contents: &str, work_tree_root: &str) -> Option<String> {
    let trimmed = contents.trim();
    let prefix = "gitdir:";
    if !trimmed.to_lowercase().starts_with(prefix) {
        return None;
    }
    // Drop the 7-char ASCII prefix (case preserved on the path) and re-trim.
    // `prefix.len()` == 7 ASCII bytes, a guaranteed char boundary, so slicing
    // yields the same &str the char-skip produced with zero allocation.
    let raw_path = trimmed[prefix.len()..].trim();
    if raw_path.is_empty() {
        return None;
    }
    if raw_path.starts_with('/') {
        Some(standardize_posix_path(raw_path))
    } else {
        Some(standardize_joining(work_tree_root, raw_path))
    }
}

// ---------------------------------------------------------------------------
// Fn 3 — gitCommonDirectory (Swift :96-112).
// ---------------------------------------------------------------------------

/// Resolves the shared common directory for `git_directory`.
///
/// Port of `gitCommonDirectory(gitDirectory:)` (Swift :96-112). The `commondir`
/// file read is the caller's job: pass `None` when the file is absent/unreadable
/// (or `Some(contents)` when read). `None`, or contents that trim to empty, fall
/// back to `git_directory`. Otherwise an absolute path is standardized directly
/// and a relative path is resolved against `git_directory`.
pub fn git_common_directory(contents: Option<&str>, git_directory: &str) -> String {
    let Some(contents) = contents else {
        return git_directory.to_string();
    };
    let raw_path = contents.trim();
    if raw_path.is_empty() {
        return git_directory.to_string();
    }
    if raw_path.starts_with('/') {
        standardize_posix_path(raw_path)
    } else {
        standardize_joining(git_directory, raw_path)
    }
}

// ---------------------------------------------------------------------------
// Optional secondary — includeIf "gitdir:" matcher (Config.swift :295-434),
// reusing crate::git_config_glob_matches. home_dir is INJECTED (Swift reads
// FileManager.homeDirectoryForCurrentUser); the onbranch: arm and the
// gitConfigIncludeURL fs-walk are OUT of scope.
// ---------------------------------------------------------------------------

/// The condition from an `[includeIf "<condition>"]` section header, or `None`.
///
/// Port of `gitConfigIncludeIfCondition(fromSectionHeader:)` (Swift :298-308).
/// The section name is case-insensitive per git; the condition is extracted
/// verbatim (its own keyword prefixes are matched case-insensitively later).
pub fn git_config_include_if_condition(header: &str) -> Option<String> {
    let prefix = "[includeif \"";
    let suffix = "\"]";
    let prefix_len = prefix.chars().count();
    let suffix_len = suffix.chars().count();

    // Swift: `header.count > prefix.count + suffix.count - 1` (≥ one char
    // between the quotes); the negated guard is `< prefix + suffix`.
    if header.chars().count() < prefix_len + suffix_len {
        return None;
    }
    if !header.to_lowercase().starts_with(prefix) {
        return None;
    }
    if !header.ends_with(suffix) {
        return None;
    }
    let chars: Vec<char> = header.chars().collect();
    let condition: String = chars[prefix_len..chars.len() - suffix_len].iter().collect();
    if condition.is_empty() {
        None
    } else {
        Some(condition)
    }
}

/// Expands an `includeIf` gitdir pattern per git's rules.
///
/// Port of `gitConfigExpandedPattern(_:configURL:)` (Swift :408-434) with the
/// home directory injected as `home_dir` (Swift reads
/// `FileManager.homeDirectoryForCurrentUser`). `~`/`~/` expand to `home_dir`,
/// `./` is relative to the config file's directory (`config_path`'s parent,
/// anchored textually so glob metacharacters survive), an absolute pattern is
/// standardized, and anything else is prefixed with `**/` so a relative pattern
/// matches at any depth.
pub fn expanded_pattern(pattern: &str, config_path: &str, home_dir: &str) -> String {
    if pattern == "~" {
        return standardize_posix_path(home_dir);
    }
    if let Some(rest) = pattern.strip_prefix("~/") {
        return standardize_joining(home_dir, rest);
    }
    if let Some(relative_path) = pattern.strip_prefix("./") {
        let base = standardize_posix_path(&deleting_last_path_component(config_path));
        if relative_path.is_empty() {
            return base;
        }
        // Anchor textually to keep glob metacharacters intact (Swift does the
        // same rather than routing the pattern through URL resolution).
        return format!("{base}/{relative_path}");
    }
    if pattern.starts_with('/') {
        return standardize_posix_path(pattern);
    }
    // Relative pattern: match at any depth.
    format!("**/{pattern}")
}

/// Whether a `gitdir`/`gitdir/i` glob pattern matches any of the repository's
/// directories.
///
/// Port of `gitConfigGitdirPatternMatches(_:repository:caseInsensitive:configURL:)`
/// (Swift :375-402). Applies git's expansion ([`expanded_pattern`]) plus the
/// trailing-`/` recursive-directory rule (append `/` then `**`), then tests the
/// git/common/work-tree directories (each standardized, and each also with a
/// trailing `/`) against the expanded glob via [`crate::git_config_glob_matches`].
pub fn gitdir_pattern_matches(
    pattern: &str,
    repository: &ResolvedGitRepository,
    case_insensitive: bool,
    config_path: &str,
    home_dir: &str,
) -> bool {
    let is_recursive_directory_pattern = pattern.ends_with('/');
    let mut expanded = expanded_pattern(pattern, config_path, home_dir);
    if is_recursive_directory_pattern && !expanded.ends_with('/') {
        expanded.push('/');
    }
    if is_recursive_directory_pattern {
        expanded.push_str("**");
    }

    let candidates = [
        standardize_posix_path(&repository.git_directory),
        standardize_posix_path(&repository.common_directory),
        standardize_posix_path(&repository.work_tree_root),
    ];

    for candidate in candidates {
        if crate::git_config_glob_matches(&candidate, &expanded, case_insensitive)
            || crate::git_config_glob_matches(
                &format!("{candidate}/"),
                &expanded,
                case_insensitive,
            )
        {
            return true;
        }
    }
    false
}

/// Whether an `includeIf` condition matches this repository.
///
/// Port of the `gitdir:`/`gitdir/i:` arms of
/// `gitConfigIncludeIfConditionMatches(_:repository:configURL:)`
/// (Swift :339-368) with `home_dir` injected. The `onbranch:` arm is OUT of
/// scope (it needs the live branch name) and any other condition returns
/// `false`.
pub fn include_if_condition_matches(
    condition: &str,
    repository: &ResolvedGitRepository,
    config_path: &str,
    home_dir: &str,
) -> bool {
    let lowercased = condition.to_lowercase();
    if let Some(pattern) = strip_prefix_by_len(condition, &lowercased, "gitdir/i:") {
        return gitdir_pattern_matches(pattern, repository, true, config_path, home_dir);
    }
    if let Some(pattern) = strip_prefix_by_len(condition, &lowercased, "gitdir:") {
        return gitdir_pattern_matches(pattern, repository, false, config_path, home_dir);
    }
    // onbranch: (out of scope) and everything else.
    false
}

/// If `lowercased` (the lowercased `condition`) starts with the ASCII
/// `keyword`, returns the remainder of the *original* `condition` after the
/// keyword (case preserved), mirroring Swift's `condition.dropFirst(keyword.count)`.
fn strip_prefix_by_len<'a>(
    condition: &'a str,
    lowercased: &str,
    keyword: &str,
) -> Option<&'a str> {
    if lowercased.starts_with(keyword) {
        // keyword is ASCII, so its char count equals its byte count.
        Some(&condition[keyword.len()..])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests — ported from GitMetadataServiceTests.swift, GitSlugAndConfigTests.swift,
// GitConfigIncludeTests.swift. Fs-fixture cases are reproduced with the file
// reads elided (the pure cores take the would-be-read contents / concrete
// absolute paths directly).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Fn 1: shouldStopGitRepositorySearch --------------------------------

    /// `rootVariantsStopRepositorySearch` (GitMetadataServiceTests.swift:50-62)
    /// — the three root variants must all terminate the walk.
    #[test]
    fn root_variants_stop_repository_search() {
        for (current, parent) in [("/", "/"), ("/", "/.."), ("/..", "/../..")] {
            assert!(
                should_stop_repository_search(current, parent),
                "current: {current}, parent: {parent}"
            );
        }
    }

    /// `nonRootParentDoesNotStopSearch` (GitMetadataServiceTests.swift:64-69).
    #[test]
    fn non_root_parent_does_not_stop_search() {
        assert!(!should_stop_repository_search(
            "/Users/someone/project",
            "/Users/someone"
        ));
    }

    // --- Fn 4: standardize_posix_path (author-derived oracles) ---------------

    /// Foundation `standardizedFileURL.path` parity table (author-derived from
    /// the lexical `./..` collapse the spec calls out).
    #[test]
    fn standardize_posix_path_oracles() {
        assert_eq!(standardize_posix_path("/a//b"), "/a/b");
        assert_eq!(standardize_posix_path("/a/b/.."), "/a");
        assert_eq!(standardize_posix_path("/a/"), "/a");
        assert_eq!(standardize_posix_path("/"), "/");
        // Foundation-specific root/.. edge — pinned so the walk terminates.
        assert_eq!(standardize_posix_path("/.."), "/");
        assert_eq!(standardize_posix_path("/../.."), "/");
        assert_eq!(standardize_posix_path("/a/./b/./c"), "/a/b/c");
        assert_eq!(standardize_posix_path("/a/b/../../c"), "/c");
        assert_eq!(standardize_posix_path("/a/../.."), "/");
    }

    // --- Fn 2: gitDirectoryFromDotGitFile -----------------------------------

    /// `dotGitFilePointerResolvesGitDirectory`
    /// (GitMetadataServiceTests.swift:29-48). Worktree-style absolute pointer:
    /// `"gitdir: /a/realgit\n"` with work-tree `/a/wt` resolves to `/a/realgit`
    /// (the work-tree root is unused for an absolute pointer).
    #[test]
    fn dot_git_file_pointer_resolves_absolute_git_directory() {
        assert_eq!(
            git_directory_from_dot_git_file("gitdir: /a/realgit\n", "/a/wt").as_deref(),
            Some("/a/realgit")
        );
    }

    /// Relative `gitdir:` pointer is resolved against the work-tree root and
    /// standardized (author-derived: `../realgit` from `/a/wt` → `/a/realgit`).
    #[test]
    fn dot_git_file_pointer_resolves_relative_git_directory() {
        assert_eq!(
            git_directory_from_dot_git_file("gitdir: ../realgit\n", "/a/wt").as_deref(),
            Some("/a/realgit")
        );
        assert_eq!(
            git_directory_from_dot_git_file("gitdir: subdir/git", "/a/wt").as_deref(),
            Some("/a/wt/subdir/git")
        );
    }

    /// The `gitdir:` prefix is case-insensitive; the extracted path is verbatim.
    #[test]
    fn dot_git_file_prefix_is_case_insensitive() {
        assert_eq!(
            git_directory_from_dot_git_file("GITDIR: /a/realgit", "/a/wt").as_deref(),
            Some("/a/realgit")
        );
    }

    /// A missing prefix or an empty extracted path yields `None`.
    #[test]
    fn dot_git_file_rejects_non_pointer_and_empty() {
        assert_eq!(git_directory_from_dot_git_file("not a pointer", "/a/wt"), None);
        assert_eq!(git_directory_from_dot_git_file("gitdir:   \n", "/a/wt"), None);
        assert_eq!(git_directory_from_dot_git_file("", "/a/wt"), None);
    }

    // --- Fn 3: gitCommonDirectory -------------------------------------------

    /// A normal clone (no `commondir` file) falls back to the git directory.
    #[test]
    fn common_directory_falls_back_without_file() {
        assert_eq!(git_common_directory(None, "/a/repo/.git"), "/a/repo/.git");
        // Empty (whitespace-only) contents also fall back.
        assert_eq!(git_common_directory(Some("  \n"), "/a/repo/.git"), "/a/repo/.git");
    }

    /// A `commondir` with an absolute path standardizes it directly; a relative
    /// path resolves against the git directory (author-derived).
    #[test]
    fn common_directory_resolves_absolute_and_relative() {
        assert_eq!(
            git_common_directory(Some("/shared/repo/.git\n"), "/a/wt/.git/worktrees/x"),
            "/shared/repo/.git"
        );
        // Linked-worktree layout: commondir "../.." from
        // .git/worktrees/x → the shared .git.
        assert_eq!(
            git_common_directory(Some("../..\n"), "/a/repo/.git/worktrees/x"),
            "/a/repo/.git"
        );
    }

    // --- includeIf "gitdir:" matcher ----------------------------------------

    /// A concrete fs-free repository fixture (paths that need no standardizing).
    fn fixture_repo() -> ResolvedGitRepository {
        ResolvedGitRepository::new("/tmp/repo", "/tmp/repo/.git", "/tmp/repo/.git")
    }

    /// `git_config_include_if_condition` header parsing (case-insensitive header,
    /// verbatim condition).
    #[test]
    fn include_if_condition_header_parsing() {
        assert_eq!(
            git_config_include_if_condition("[includeIf \"gitdir:~/work/\"]").as_deref(),
            Some("gitdir:~/work/")
        );
        assert_eq!(
            git_config_include_if_condition("[INCLUDEIF \"gitdir:/x/\"]").as_deref(),
            Some("gitdir:/x/")
        );
        assert_eq!(git_config_include_if_condition("[include]"), None);
        assert_eq!(git_config_include_if_condition("[includeIf \"\"]"), None);
    }

    /// `includeIfGitdirRecursiveMatchesNestedRepository`
    /// (GitSlugAndConfigTests.swift:81-90). An absolute gitdir with a trailing
    /// slash matches the git directory recursively.
    #[test]
    fn include_if_gitdir_recursive_matches_nested_repository() {
        let repo = fixture_repo();
        assert!(include_if_condition_matches(
            "gitdir:/tmp/repo/.git/",
            &repo,
            "/tmp/repo/.git/config",
            "/home/user",
        ));
    }

    /// `relativeGitdirPatternMatchesAtAnyDepth`
    /// (GitSlugAndConfigTests.swift:130-148). A relative pattern gets `**/`
    /// prepended, so `<dir>/.git/` matches the absolute git directory; a
    /// different directory name does not.
    #[test]
    fn relative_gitdir_pattern_matches_at_any_depth() {
        let repo = fixture_repo();
        let config = "/tmp/repo/.git/config";
        assert!(include_if_condition_matches(
            "gitdir:repo/.git/",
            &repo,
            config,
            "/home/user",
        ));
        assert!(!include_if_condition_matches(
            "gitdir:not-the-dir/.git/",
            &repo,
            config,
            "/home/user",
        ));
    }

    /// `dotSlashGitdirPatternIsRelativeToConfigDirectory`
    /// (GitSlugAndConfigTests.swift:150-167). `./` is anchored to the config
    /// file's directory: it matches when the config lives in the git directory,
    /// and does not when anchored elsewhere.
    #[test]
    fn dot_slash_gitdir_pattern_is_relative_to_config_directory() {
        let repo = fixture_repo();
        assert!(include_if_condition_matches(
            "gitdir:./",
            &repo,
            "/tmp/repo/.git/config",
            "/home/user",
        ));
        assert!(!include_if_condition_matches(
            "gitdir:./",
            &repo,
            "/somewhere/else/config",
            "/home/user",
        ));
    }

    /// `gitdir/i:` lowercases both sides (case-insensitive match), and `~/`
    /// expands against the injected home directory.
    #[test]
    fn gitdir_case_insensitive_and_home_expansion() {
        let repo = ResolvedGitRepository::new(
            "/home/user/Work/Repo",
            "/home/user/Work/Repo/.git",
            "/home/user/Work/Repo/.git",
        );
        // Case-insensitive: pattern casing differs from the on-disk path.
        assert!(include_if_condition_matches(
            "gitdir/i:/home/user/work/repo/.git/",
            &repo,
            "/home/user/Work/Repo/.git/config",
            "/home/user",
        ));
        // Case-sensitive gitdir: the same mismatched casing must NOT match.
        assert!(!include_if_condition_matches(
            "gitdir:/home/user/work/repo/.git/",
            &repo,
            "/home/user/Work/Repo/.git/config",
            "/home/user",
        ));
        // `~/` expands against the injected home dir.
        assert!(include_if_condition_matches(
            "gitdir:~/Work/",
            &repo,
            "/home/user/Work/Repo/.git/config",
            "/home/user",
        ));
    }

    /// The `onbranch:` arm (and any unrecognized condition) is out of scope and
    /// returns false.
    #[test]
    fn onbranch_and_unknown_conditions_do_not_match() {
        let repo = fixture_repo();
        assert!(!include_if_condition_matches(
            "onbranch:main",
            &repo,
            "/tmp/repo/.git/config",
            "/home/user",
        ));
        assert!(!include_if_condition_matches(
            "hostname:example",
            &repo,
            "/tmp/repo/.git/config",
            "/home/user",
        ));
    }
}
