// Pure data contract for the sidebar's per-workspace git/PR badges — the web
// twin of the canonical projection chain:
//
//   Packages/macOS/CmuxSidebar/.../Git/SidebarBranchOrdering.swift
//     (orderedUniqueBranches :196-227, orderedUniquePullRequests :231-295)
//   Sources/Workspace.swift:5237-5287
//     (sidebarGitBranchesInDisplayOrder / sidebarPullRequestsInDisplayOrder,
//      including the PR-vs-current-branch validity filter :5271-5277)
//   Sources/ContentView.swift
//     (gitBranchSummaryText :14594-14604, PullRequestDisplay :12901-12907 +
//      :14680-14691, row render :13585-13712, status words :14730-14736,
//      visibility gates :9583-9607 + :14466-14473)
//
// This module is the contract consumed by the badge mount lane and the future
// git poller: no components, no polling, no IPC. Directory rendering (the
// `SidebarDirectoryText` half of the branch+directory row) is a separate
// concern and is NOT part of this contract.
//
// Display rules pinned here rather than at the mount site:
// - Labels are never character-truncated. Canonical truncates via
//   `lineLimit(1)` + `.truncationMode(.tail)` — a one-line ellipsized layout
//   rule for the renderer, not a string transform.
// - A dirty branch renders as `name*` (ContentView.swift:14602).
// - The compact (non-vertical) layout joins branch labels with " | "
//   (ContentView.swift:14597).
// - A PR row is `{label} #{number}` plus a status word; stale rows render at
//   0.5 opacity (ContentView.swift:13687,13695,13700) — carried as the
//   "secondaryStale" tone.

/// The git branch reported for one panel (Swift `SidebarGitBranchState`).
export type SidebarGitBranchState = {
  branch: string;
  isDirty: boolean;
};

/// Lifecycle status of a PR row. Raw values are a control-socket wire
/// format; frozen (Swift `SidebarPullRequestStatus`).
export type SidebarPullRequestStatus = "open" | "merged" | "closed";

/// The pull request reported for one panel (Swift `SidebarPullRequestState`).
/// `branch` must be pre-normalized by the producer the way the Swift
/// initializer does (`normalizedSidebarBranchName`); consumers re-normalize
/// defensively, so an untrimmed value still compares correctly.
export type SidebarPullRequestState = {
  number: number;
  /// The repository label, e.g. `owner/repo`.
  label: string;
  url: string;
  status: SidebarPullRequestStatus;
  /// The PR's head branch; PRs without one bypass the branch-validity filter.
  branch?: string;
  /// Whether the row was reported by an inactive panel.
  isStale: boolean;
};

/// The branch name trimmed of whitespace/newlines, or `undefined` when empty
/// (Swift `String.normalizedSidebarBranchName`).
export function normalizedSidebarBranchName(
  text: string | undefined,
): string | undefined {
  const trimmed = text?.trim();
  return trimmed ? trimmed : undefined;
}

/// One unique branch row: the branch name and whether any contributing panel
/// is dirty (Swift `SidebarBranchOrdering.BranchEntry`).
export type BranchEntry = {
  name: string;
  isDirty: boolean;
};

/// Unique branches in first-seen panel order, dirty if any contributing panel
/// is dirty; falls back to the workspace-level branch when no panel reports
/// one (SidebarBranchOrdering.swift:196-227).
export function orderedUniqueBranches(
  orderedPanelIds: readonly string[],
  panelBranches: Readonly<Record<string, SidebarGitBranchState>>,
  fallbackBranch: SidebarGitBranchState | undefined,
): BranchEntry[] {
  const orderedNames: string[] = [];
  const branchDirty = new Map<string, boolean>();

  for (const panelId of orderedPanelIds) {
    const state = panelBranches[panelId];
    if (state === undefined) {
      continue;
    }
    const name = state.branch.trim();
    if (name.length === 0) {
      continue;
    }
    const dirty = branchDirty.get(name);
    if (dirty === undefined) {
      orderedNames.push(name);
      branchDirty.set(name, state.isDirty);
    } else if (state.isDirty) {
      branchDirty.set(name, true);
    }
  }

  if (orderedNames.length === 0 && fallbackBranch !== undefined) {
    const name = fallbackBranch.branch.trim();
    if (name.length !== 0) {
      return [{ name, isDirty: fallbackBranch.isDirty }];
    }
  }

  return orderedNames.map((name) => ({
    name,
    isDirty: branchDirty.get(name) ?? false,
  }));
}

// Higher wins when two panels report the same review item
// (SidebarBranchOrdering.swift:236-246).
const STATUS_PRIORITY: Record<SidebarPullRequestStatus, number> = {
  merged: 3,
  open: 2,
  closed: 1,
};

function freshnessPriority(isStale: boolean): number {
  return isStale ? 0 : 1;
}

/// Canonical key for a review URL: query/fragment stripped, scheme/host
/// lowercased, one trailing path slash dropped — URL variants that differ
/// only by those parts are the same review item
/// (SidebarBranchOrdering.swift:248-264). Unparseable URLs key by their raw
/// string, like the Swift `URLComponents` guard.
export function normalizedReviewUrlKey(url: string): string {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return url;
  }
  const scheme = parsed.protocol.replace(/:$/, "").toLowerCase();
  const host = parsed.hostname.toLowerCase();
  // Note: WHATWG URL omits a scheme-default explicit port (":443" on https),
  // where Swift URLComponents preserves it. Both variants of the same review
  // URL still collapse to one key, so dedupe behavior is unaffected.
  const port = parsed.port ? `:${parsed.port}` : "";
  let path = parsed.pathname;
  if (path.endsWith("/") && path.length > 1) {
    path = path.slice(0, -1);
  }
  return `${scheme}://${host}${port}${path}`;
}

function reviewKey(state: SidebarPullRequestState): string {
  return `${state.label.toLowerCase()}#${state.number}|${normalizedReviewUrlKey(state.url)}`;
}

/// Unique pull requests in first-seen panel order, deduplicated by normalized
/// review URL; fresher then higher-status states win, keeping the first-seen
/// position (SidebarBranchOrdering.swift:231-295).
export function orderedUniquePullRequests(
  orderedPanelIds: readonly string[],
  panelPullRequests: Readonly<Record<string, SidebarPullRequestState>>,
  fallbackPullRequest: SidebarPullRequestState | undefined,
): SidebarPullRequestState[] {
  const orderedKeys: string[] = [];
  const pullRequestsByKey = new Map<string, SidebarPullRequestState>();

  for (const panelId of orderedPanelIds) {
    const state = panelPullRequests[panelId];
    if (state === undefined) {
      continue;
    }
    const key = reviewKey(state);
    const existing = pullRequestsByKey.get(key);
    if (existing === undefined) {
      orderedKeys.push(key);
      pullRequestsByKey.set(key, state);
      continue;
    }
    if (freshnessPriority(state.isStale) > freshnessPriority(existing.isStale)) {
      pullRequestsByKey.set(key, state);
    } else if (
      freshnessPriority(state.isStale) === freshnessPriority(existing.isStale) &&
      STATUS_PRIORITY[state.status] > STATUS_PRIORITY[existing.status]
    ) {
      pullRequestsByKey.set(key, state);
    }
  }

  if (orderedKeys.length === 0 && fallbackPullRequest !== undefined) {
    return [fallbackPullRequest];
  }

  return orderedKeys.flatMap((key) => {
    const state = pullRequestsByKey.get(key);
    return state === undefined ? [] : [state];
  });
}

/// Drops panel PRs whose head branch no longer matches the panel's current
/// branch (both normalized); PRs without a branch are always kept
/// (Workspace.swift:5271-5277 `sidebarPullRequestsInDisplayOrder`).
export function validPanelPullRequests(
  panelPullRequests: Readonly<Record<string, SidebarPullRequestState>>,
  panelGitBranches: Readonly<Record<string, SidebarGitBranchState>>,
): Record<string, SidebarPullRequestState> {
  const valid: Record<string, SidebarPullRequestState> = {};
  for (const [panelId, state] of Object.entries(panelPullRequests)) {
    const pullRequestBranch = normalizedSidebarBranchName(state.branch);
    if (
      pullRequestBranch === undefined ||
      normalizedSidebarBranchName(panelGitBranches[panelId]?.branch) ===
        pullRequestBranch
    ) {
      valid[panelId] = state;
    }
  }
  return valid;
}

/// One branch line as rendered: `name` plus the `*` dirty marker
/// (ContentView.swift:14600-14604).
export function branchLineLabel(entry: BranchEntry): string {
  return `${entry.name}${entry.isDirty ? "*" : ""}`;
}

/// The compact single-line branch summary: per-branch labels joined with
/// " | ", or `null` when there is nothing to show
/// (ContentView.swift:14594-14598).
export function gitBranchSummaryText(entries: readonly BranchEntry[]): string | null {
  if (entries.length === 0) {
    return null;
  }
  return entries.map(branchLineLabel).join(" | ");
}

/// The status word rendered after the PR title (ContentView.swift:14730-14736
/// English default values; localization rides the message catalog at mount).
export function pullRequestStatusLabel(status: SidebarPullRequestStatus): string {
  return status;
}

/// Visibility settings that gate badge emission, resolved booleans with the
/// canonical defaults (Sources/CmuxSettingsJSONPathSupport.swift:6-20 and
/// ContentView.swift:9569 `sidebarShowGitBranch` default true).
export type BadgeVisibilitySettings = {
  /// `sidebar.hideAllDetails` — hides every detail row when true.
  hideAllDetails: boolean;
  /// `sidebarShowBranchDirectory` — gates the whole branch+directory row.
  showBranchDirectory: boolean;
  /// `sidebarShowGitBranch` — gates the branch half of that row.
  showGitBranch: boolean;
  /// `sidebarShowPullRequest` — gates the PR rows.
  showPullRequests: boolean;
};

export const DEFAULT_BADGE_VISIBILITY: BadgeVisibilitySettings = {
  hideAllDetails: false,
  showBranchDirectory: true,
  showGitBranch: true,
  showPullRequests: true,
};

/// Everything the projection needs about one workspace. Per-panel maps are
/// keyed by panel (surface) id; `orderedPanelIds` is the sidebar's spatial
/// panel display order (Workspace.swift `sidebarOrderedPanelIds()`), which
/// the future git poller/session layer supplies alongside the git state.
export type WorkspaceBadgeInput = {
  orderedPanelIds: readonly string[];
  panelGitBranches?: Readonly<Record<string, SidebarGitBranchState>>;
  panelPullRequests?: Readonly<Record<string, SidebarPullRequestState>>;
  /// The workspace-level branch mirror (Workspace.swift `gitBranch`), used
  /// only when no panel reports a branch.
  fallbackBranch?: SidebarGitBranchState;
  settings?: Partial<BadgeVisibilitySettings>;
};

/// Visual class of a badge. Both badge kinds render in the sidebar's
/// secondary foreground (branch: activeSecondaryColor(0.75) monospace,
/// ContentView.swift:13600-13601; PR: pullRequestForegroundColor,
/// ContentView.swift:14693-14695); a stale PR additionally renders at 0.5
/// opacity (ContentView.swift:13700).
export type WorkspaceBadgeTone = "secondary" | "secondaryStale";

/// A branch badge. `id` is port-synthesized from the (unique) branch name —
/// canonical keys these lines by ForEach offset, so there is no oracle id to
/// mirror.
export type BranchBadgeDescriptor = {
  kind: "branch";
  id: string;
  /// `name` + `*` dirty marker, exactly as rendered.
  label: string;
  tone: "secondary";
  name: string;
  isDirty: boolean;
};

/// A pull-request badge. `id` is the canonical `PullRequestDisplay.id`:
/// `{label.lowercased()}#{number}|{url}` (ContentView.swift:14683 — the raw
/// URL, not the dedupe-normalized one).
export type PullRequestBadgeDescriptor = {
  kind: "pullRequest";
  id: string;
  /// `{label} #{number}`, exactly as rendered (ContentView.swift:13687).
  label: string;
  tone: WorkspaceBadgeTone;
  /// The status word rendered after the label.
  statusLabel: string;
  status: SidebarPullRequestStatus;
  url: string;
  number: number;
  repoLabel: string;
  isStale: boolean;
};

export type WorkspaceBadgeDescriptor =
  | BranchBadgeDescriptor
  | PullRequestBadgeDescriptor;

/// The renderable badge state for one workspace row.
export type WorkspaceBadges = {
  /// Compact (non-vertical) single-line branch summary, `null` when hidden
  /// or empty. Vertical-layout consumers use the per-branch badges instead.
  branchSummaryText: string | null;
  /// Ordered descriptors: branch badges in display order, then PR badges in
  /// display order (matching canonical's row stacking).
  badges: readonly WorkspaceBadgeDescriptor[];
};

/// Projects one workspace's git/PR state into ordered badge descriptors,
/// applying the canonical visibility gates:
/// - `hideAllDetails` suppresses everything
///   (SidebarWorkspaceAuxiliaryDetailVisibility.resolved, ContentView.swift:9599-9607).
/// - branch badges require `showBranchDirectory && showGitBranch`
///   (ContentView.swift:14466-14473).
/// - PR badges require `showPullRequests` (ContentView.swift:13683).
export function badgesForWorkspace(input: WorkspaceBadgeInput): WorkspaceBadges {
  const settings = { ...DEFAULT_BADGE_VISIBILITY, ...input.settings };
  const panelGitBranches = input.panelGitBranches ?? {};
  const panelPullRequests = input.panelPullRequests ?? {};

  const showBranches =
    !settings.hideAllDetails &&
    settings.showBranchDirectory &&
    settings.showGitBranch;
  const showPullRequests = !settings.hideAllDetails && settings.showPullRequests;

  const branchEntries = showBranches
    ? orderedUniqueBranches(
        input.orderedPanelIds,
        panelGitBranches,
        input.fallbackBranch,
      )
    : [];

  const pullRequests = showPullRequests
    ? orderedUniquePullRequests(
        input.orderedPanelIds,
        validPanelPullRequests(panelPullRequests, panelGitBranches),
        // Canonical passes no fallback here (Workspace.swift:5281).
        undefined,
      )
    : [];

  const badges: WorkspaceBadgeDescriptor[] = branchEntries.map((entry) => ({
    kind: "branch",
    id: `branch:${entry.name}`,
    label: branchLineLabel(entry),
    tone: "secondary",
    name: entry.name,
    isDirty: entry.isDirty,
  }));

  for (const pullRequest of pullRequests) {
    badges.push({
      kind: "pullRequest",
      id: `${pullRequest.label.toLowerCase()}#${pullRequest.number}|${pullRequest.url}`,
      label: `${pullRequest.label} #${pullRequest.number}`,
      tone: pullRequest.isStale ? "secondaryStale" : "secondary",
      statusLabel: pullRequestStatusLabel(pullRequest.status),
      status: pullRequest.status,
      url: pullRequest.url,
      number: pullRequest.number,
      repoLabel: pullRequest.label,
      isStale: pullRequest.isStale,
    });
  }

  return {
    branchSummaryText: gitBranchSummaryText(branchEntries),
    badges,
  };
}
