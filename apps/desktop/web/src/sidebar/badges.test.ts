// Oracle-pinned tests for the git/PR badge data contract. Citations:
// SidebarBranchOrdering.swift (orderedUniqueBranches :196-227,
// orderedUniquePullRequests :231-295), Workspace.swift:5271-5287,
// ContentView.swift (:14594-14604 summary, :14680-14691 display rows,
// :13683-13712 render, :14730-14736 status words), and
// SidebarValueVocabularyTests.swift (branch-name normalization parity).

import { describe, expect, test } from "bun:test";

import {
  DEFAULT_BADGE_VISIBILITY,
  badgesForWorkspace,
  branchLineLabel,
  gitBranchSummaryText,
  normalizedReviewUrlKey,
  normalizedSidebarBranchName,
  orderedUniqueBranches,
  orderedUniquePullRequests,
  pullRequestStatusLabel,
  validPanelPullRequests,
  type SidebarPullRequestState,
  type WorkspaceBadgeInput,
} from "./badges";

const P1 = "panel-1";
const P2 = "panel-2";
const P3 = "panel-3";

function pr(
  overrides: Partial<SidebarPullRequestState> = {},
): SidebarPullRequestState {
  return {
    number: 12,
    label: "owner/repo",
    url: "https://github.com/owner/repo/pull/12",
    status: "open",
    isStale: false,
    ...overrides,
  };
}

describe("normalizedSidebarBranchName", () => {
  // Parity with SidebarValueVocabularyTests.swift:31-38.
  test("trims whitespace and newlines", () => {
    expect(normalizedSidebarBranchName("  main \n")).toBe("main");
    expect(normalizedSidebarBranchName("main")).toBe("main");
  });

  test("empty and whitespace-only become undefined", () => {
    expect(normalizedSidebarBranchName("   ")).toBeUndefined();
    expect(normalizedSidebarBranchName("")).toBeUndefined();
    expect(normalizedSidebarBranchName(undefined)).toBeUndefined();
  });
});

describe("orderedUniqueBranches", () => {
  test("first-seen panel order", () => {
    const result = orderedUniqueBranches(
      [P1, P2, P3],
      {
        [P1]: { branch: "feature", isDirty: false },
        [P2]: { branch: "main", isDirty: false },
        [P3]: { branch: "feature", isDirty: false },
      },
      undefined,
    );
    expect(result).toEqual([
      { name: "feature", isDirty: false },
      { name: "main", isDirty: false },
    ]);
  });

  test("dirty if any contributing panel is dirty (later dirty upgrades)", () => {
    const result = orderedUniqueBranches(
      [P1, P2],
      {
        [P1]: { branch: "main", isDirty: false },
        [P2]: { branch: "main", isDirty: true },
      },
      undefined,
    );
    expect(result).toEqual([{ name: "main", isDirty: true }]);
  });

  test("later clean panel does not downgrade dirty", () => {
    const result = orderedUniqueBranches(
      [P1, P2],
      {
        [P1]: { branch: "main", isDirty: true },
        [P2]: { branch: "main", isDirty: false },
      },
      undefined,
    );
    expect(result).toEqual([{ name: "main", isDirty: true }]);
  });

  test("names are trimmed; whitespace-only skipped", () => {
    const result = orderedUniqueBranches(
      [P1, P2],
      {
        [P1]: { branch: "  main  ", isDirty: false },
        [P2]: { branch: "   ", isDirty: true },
      },
      undefined,
    );
    expect(result).toEqual([{ name: "main", isDirty: false }]);
  });

  test("panels without a branch are skipped", () => {
    const result = orderedUniqueBranches(
      [P1, P2],
      { [P2]: { branch: "dev", isDirty: false } },
      undefined,
    );
    expect(result).toEqual([{ name: "dev", isDirty: false }]);
  });

  test("fallback used only when no panel reports a branch", () => {
    expect(
      orderedUniqueBranches([P1], {}, { branch: " main ", isDirty: true }),
    ).toEqual([{ name: "main", isDirty: true }]);
    expect(
      orderedUniqueBranches(
        [P1],
        { [P1]: { branch: "dev", isDirty: false } },
        { branch: "main", isDirty: true },
      ),
    ).toEqual([{ name: "dev", isDirty: false }]);
  });

  test("empty fallback yields no rows", () => {
    expect(
      orderedUniqueBranches([], {}, { branch: "   ", isDirty: true }),
    ).toEqual([]);
    expect(orderedUniqueBranches([], {}, undefined)).toEqual([]);
  });
});

describe("normalizedReviewUrlKey", () => {
  test("strips query and fragment", () => {
    expect(
      normalizedReviewUrlKey("https://github.com/o/r/pull/1?tab=files#top"),
    ).toBe("https://github.com/o/r/pull/1");
  });

  test("lowercases scheme and host, keeps path case", () => {
    expect(normalizedReviewUrlKey("HTTPS://GitHub.com/O/R/pull/1")).toBe(
      "https://github.com/O/R/pull/1",
    );
  });

  test("drops one trailing path slash, keeps bare root slash", () => {
    expect(normalizedReviewUrlKey("https://github.com/o/r/pull/1/")).toBe(
      "https://github.com/o/r/pull/1",
    );
    expect(normalizedReviewUrlKey("https://github.com/")).toBe(
      "https://github.com/",
    );
  });

  test("preserves an explicit non-default port", () => {
    expect(normalizedReviewUrlKey("https://git.corp:8443/o/r/pull/1")).toBe(
      "https://git.corp:8443/o/r/pull/1",
    );
  });

  test("unparseable URL keys by its raw string", () => {
    expect(normalizedReviewUrlKey("not a url")).toBe("not a url");
  });
});

describe("orderedUniquePullRequests", () => {
  test("first-seen panel order across distinct reviews", () => {
    const a = pr({ number: 1, url: "https://github.com/o/r/pull/1" });
    const b = pr({ number: 2, url: "https://github.com/o/r/pull/2" });
    const result = orderedUniquePullRequests(
      [P2, P1],
      { [P2]: b, [P1]: a },
      undefined,
    );
    expect(result).toEqual([b, a]);
  });

  test("dedupes across query/fragment/trailing-slash and label-case variants", () => {
    const first = pr({ url: "https://github.com/owner/repo/pull/12" });
    const variant = pr({
      label: "OWNER/REPO",
      url: "https://github.com/owner/repo/pull/12/?tab=checks#bottom",
    });
    const result = orderedUniquePullRequests(
      [P1, P2],
      { [P1]: first, [P2]: variant },
      undefined,
    );
    expect(result).toEqual([first]);
  });

  test("fresh replaces stale at the first-seen position", () => {
    const stale = pr({ isStale: true, status: "open" });
    const fresh = pr({ isStale: false, status: "open" });
    const other = pr({ number: 9, url: "https://github.com/o/r/pull/9" });
    const result = orderedUniquePullRequests(
      [P1, P2, P3],
      { [P1]: stale, [P2]: other, [P3]: fresh },
      undefined,
    );
    expect(result).toEqual([fresh, other]);
  });

  test("stale does not replace fresh", () => {
    const fresh = pr({ isStale: false, status: "open" });
    const stale = pr({ isStale: true, status: "merged" });
    const result = orderedUniquePullRequests(
      [P1, P2],
      { [P1]: fresh, [P2]: stale },
      undefined,
    );
    expect(result).toEqual([fresh]);
  });

  test("equal freshness: higher status wins (merged > open > closed)", () => {
    const open = pr({ status: "open" });
    const merged = pr({ status: "merged" });
    const closed = pr({ status: "closed" });
    expect(
      orderedUniquePullRequests([P1, P2], { [P1]: open, [P2]: merged }, undefined),
    ).toEqual([merged]);
    expect(
      orderedUniquePullRequests([P1, P2], { [P1]: open, [P2]: closed }, undefined),
    ).toEqual([open]);
  });

  test("fallback used only when no panel reports a PR", () => {
    const fallback = pr();
    expect(orderedUniquePullRequests([P1], {}, fallback)).toEqual([fallback]);
    const panelPr = pr({ number: 3, url: "https://github.com/o/r/pull/3" });
    expect(
      orderedUniquePullRequests([P1], { [P1]: panelPr }, fallback),
    ).toEqual([panelPr]);
  });

  test("unparseable URLs dedupe by raw string", () => {
    const a = pr({ url: "not a url" });
    const b = pr({ url: "not a url", status: "merged" });
    const result = orderedUniquePullRequests(
      [P1, P2],
      { [P1]: a, [P2]: b },
      undefined,
    );
    expect(result).toEqual([b]);
  });
});

describe("validPanelPullRequests", () => {
  // Workspace.swift:5271-5277.
  test("keeps a PR whose branch matches the panel branch after normalization", () => {
    const state = pr({ branch: " main " });
    const result = validPanelPullRequests(
      { [P1]: state },
      { [P1]: { branch: "main", isDirty: false } },
    );
    expect(result).toEqual({ [P1]: state });
  });

  test("drops a PR whose branch differs from the panel branch", () => {
    const result = validPanelPullRequests(
      { [P1]: pr({ branch: "feature" }) },
      { [P1]: { branch: "main", isDirty: false } },
    );
    expect(result).toEqual({});
  });

  test("drops a branch-carrying PR when the panel has no branch", () => {
    const result = validPanelPullRequests({ [P1]: pr({ branch: "main" }) }, {});
    expect(result).toEqual({});
  });

  test("keeps a PR without a branch regardless of panel branch", () => {
    const state = pr();
    expect(
      validPanelPullRequests(
        { [P1]: state },
        { [P1]: { branch: "anything", isDirty: true } },
      ),
    ).toEqual({ [P1]: state });
    expect(validPanelPullRequests({ [P1]: state }, {})).toEqual({
      [P1]: state,
    });
  });
});

describe("branch summary text", () => {
  test("dirty marker and separator (ContentView.swift:14597,14602)", () => {
    expect(branchLineLabel({ name: "main", isDirty: true })).toBe("main*");
    expect(branchLineLabel({ name: "main", isDirty: false })).toBe("main");
    expect(
      gitBranchSummaryText([
        { name: "main", isDirty: true },
        { name: "dev", isDirty: false },
      ]),
    ).toBe("main* | dev");
  });

  test("null when there are no branches", () => {
    expect(gitBranchSummaryText([])).toBeNull();
  });
});

describe("pullRequestStatusLabel", () => {
  test("canonical English status words", () => {
    expect(pullRequestStatusLabel("open")).toBe("open");
    expect(pullRequestStatusLabel("merged")).toBe("merged");
    expect(pullRequestStatusLabel("closed")).toBe("closed");
  });
});

describe("badgesForWorkspace", () => {
  function input(overrides: Partial<WorkspaceBadgeInput> = {}): WorkspaceBadgeInput {
    return {
      orderedPanelIds: [P1, P2],
      panelGitBranches: {
        [P1]: { branch: "main", isDirty: true },
        [P2]: { branch: "dev", isDirty: false },
      },
      panelPullRequests: {
        [P1]: pr({ branch: "main" }),
      },
      ...overrides,
    };
  }

  test("branch badges precede PR badges, both in display order", () => {
    const result = badgesForWorkspace(input());
    expect(result.badges.map((b) => b.kind)).toEqual([
      "branch",
      "branch",
      "pullRequest",
    ]);
    expect(result.badges.map((b) => b.label)).toEqual([
      "main*",
      "dev",
      "owner/repo #12",
    ]);
    expect(result.branchSummaryText).toBe("main* | dev");
  });

  test("PR badge carries the canonical display id, status word, and tone", () => {
    const result = badgesForWorkspace(
      input({
        panelPullRequests: {
          [P1]: pr({ label: "Owner/Repo", status: "merged", branch: "main" }),
        },
      }),
    );
    const badge = result.badges.at(-1);
    expect(badge).toEqual({
      kind: "pullRequest",
      // ContentView.swift:14683 — lowercased label, raw URL.
      id: "owner/repo#12|https://github.com/owner/repo/pull/12",
      label: "Owner/Repo #12",
      tone: "secondary",
      statusLabel: "merged",
      status: "merged",
      url: "https://github.com/owner/repo/pull/12",
      number: 12,
      repoLabel: "Owner/Repo",
      isStale: false,
    });
  });

  test("stale PR gets the secondaryStale tone (0.5-opacity render rule)", () => {
    const result = badgesForWorkspace(
      input({
        panelPullRequests: { [P1]: pr({ isStale: true, branch: "main" }) },
      }),
    );
    const badge = result.badges.at(-1);
    expect(badge?.kind).toBe("pullRequest");
    expect(badge?.tone).toBe("secondaryStale");
  });

  test("branch badge shape", () => {
    const result = badgesForWorkspace(input());
    expect(result.badges[0]).toEqual({
      kind: "branch",
      id: "branch:main",
      label: "main*",
      tone: "secondary",
      name: "main",
      isDirty: true,
    });
  });

  test("branch-validity filter applies before dedupe", () => {
    const result = badgesForWorkspace(
      input({
        panelPullRequests: {
          [P1]: pr({ branch: "stale-branch" }),
          [P2]: pr({ number: 7, url: "https://github.com/o/r/pull/7", branch: "dev" }),
        },
      }),
    );
    const prBadges = result.badges.filter((b) => b.kind === "pullRequest");
    expect(prBadges.map((b) => b.number)).toEqual([7]);
  });

  test("workspace fallback branch used only when panels report none", () => {
    const result = badgesForWorkspace(
      input({
        panelGitBranches: {},
        fallbackBranch: { branch: "main", isDirty: false },
        panelPullRequests: {},
      }),
    );
    expect(result.branchSummaryText).toBe("main");
    expect(result.badges).toEqual([
      {
        kind: "branch",
        id: "branch:main",
        label: "main",
        tone: "secondary",
        name: "main",
        isDirty: false,
      },
    ]);
  });

  test("hideAllDetails suppresses everything", () => {
    const result = badgesForWorkspace(
      input({ settings: { hideAllDetails: true } }),
    );
    expect(result.badges).toEqual([]);
    expect(result.branchSummaryText).toBeNull();
  });

  test("showBranchDirectory=false hides branch badges but keeps PRs", () => {
    const result = badgesForWorkspace(
      input({ settings: { showBranchDirectory: false } }),
    );
    expect(result.branchSummaryText).toBeNull();
    expect(result.badges.map((b) => b.kind)).toEqual(["pullRequest"]);
  });

  test("showGitBranch=false hides branch badges but keeps PRs", () => {
    const result = badgesForWorkspace(
      input({ settings: { showGitBranch: false } }),
    );
    expect(result.branchSummaryText).toBeNull();
    expect(result.badges.map((b) => b.kind)).toEqual(["pullRequest"]);
  });

  test("showPullRequests=false hides PR badges but keeps branches", () => {
    const result = badgesForWorkspace(
      input({ settings: { showPullRequests: false } }),
    );
    expect(result.branchSummaryText).toBe("main* | dev");
    expect(result.badges.map((b) => b.kind)).toEqual(["branch", "branch"]);
  });

  test("defaults show everything and missing maps are empty", () => {
    expect(DEFAULT_BADGE_VISIBILITY).toEqual({
      hideAllDetails: false,
      showBranchDirectory: true,
      showGitBranch: true,
      showPullRequests: true,
      showPorts: true,
      showSsh: true,
    });
    const result = badgesForWorkspace({ orderedPanelIds: [P1] });
    expect(result.badges).toEqual([]);
    expect(result.branchSummaryText).toBeNull();
  });

  test("no fallback PR: canonical passes nil (Workspace.swift:5281)", () => {
    const result = badgesForWorkspace(
      input({ panelPullRequests: {}, panelGitBranches: {} }),
    );
    expect(result.badges).toEqual([]);
  });

  test("listening ports render as sorted localhost badge descriptors", () => {
    const result = badgesForWorkspace(
      input({
        listeningPorts: [5173, 3000, 5173, 0, 65536],
      }),
    );
    expect(result.badges.slice(-2)).toEqual([
      {
        kind: "port",
        id: "port:3000",
        label: ":3000",
        tone: "secondary",
        port: 3000,
        url: "http://localhost:3000",
      },
      {
        kind: "port",
        id: "port:5173",
        label: ":5173",
        tone: "secondary",
        port: 5173,
        url: "http://localhost:5173",
      },
    ]);
  });

  test("command-running shell activity renders a runtime badge", () => {
    const result = badgesForWorkspace(
      input({
        panelShellActivity: {
          [P1]: "promptIdle",
          [P2]: "commandRunning",
        },
      }),
    );

    expect(result.badges.at(-1)).toEqual({
      kind: "shellActivity",
      id: "shell-activity:running",
      label: "shell",
      tone: "secondary",
      status: "running",
      statusLabel: "running",
      runningPanelCount: 1,
      title: `Running command in ${P2}`,
    });
  });

  test("hideAllDetails suppresses shell activity badges", () => {
    const result = badgesForWorkspace(
      input({
        panelShellActivity: {
          [P1]: "commandRunning",
        },
        settings: { hideAllDetails: true },
      }),
    );

    expect(result.badges).toEqual([]);
  });

  test("showPorts=false hides listening-port badges only", () => {
    const result = badgesForWorkspace(
      input({
        listeningPorts: [3000],
        settings: { showPorts: false },
      }),
    );
    expect(result.badges.map((badge) => badge.kind)).toEqual([
      "branch",
      "branch",
      "pullRequest",
    ]);
  });

  test("remote workspace state renders as an SSH badge before port chips", () => {
    const result = badgesForWorkspace(
      input({
        listeningPorts: [5173],
        remote: {
          enabled: true,
          connected: true,
          state: "connected",
          transport: "ssh",
          destination: "dev.example.com",
          localProxyPort: 31337,
          hasSshOptions: true,
          proxyUrl: "socks5://127.0.0.1:31337",
          forwardedPorts: [5173],
          activeTerminalSessions: 2,
        },
        panelShellActivity: {
          [P2]: "commandRunning",
        },
      }),
    );

    expect(result.badges.map((badge) => badge.kind)).toEqual([
      "branch",
      "branch",
      "pullRequest",
      "remote",
      "shellActivity",
      "port",
    ]);
    expect(result.badges[3]).toEqual({
      kind: "remote",
      id: "remote:ssh:dev.example.com",
      label: "SSH dev.example.com",
      tone: "secondary",
      status: "connected",
      statusLabel: "connected",
      transport: "SSH",
      destination: "dev.example.com",
      title:
        "SSH dev.example.com | state: connected | proxy: socks5://127.0.0.1:31337 | local proxy: 31337 | forwarded: 5173 | terminals: 2 | custom SSH options",
    });
    expect(result.badges[4]).toMatchObject({
      kind: "shellActivity",
      statusLabel: "running",
    });
  });

  test("showSsh=false hides only the remote badge", () => {
    const result = badgesForWorkspace(
      input({
        listeningPorts: [3000],
        remote: {
          enabled: true,
          connected: false,
          state: "bootstrapping",
          transport: "ssh",
          destination: "dev.example.com",
        },
        settings: { showSsh: false },
      }),
    );

    expect(result.badges.map((badge) => badge.kind)).toEqual([
      "branch",
      "branch",
      "pullRequest",
      "port",
    ]);
  });

  test("remote errors render with stale secondary tone and detail", () => {
    const result = badgesForWorkspace(
      input({
        panelGitBranches: {},
        panelPullRequests: {},
        remote: {
          enabled: true,
          connected: false,
          state: "error",
          transport: "ssh",
          destination: "dev.example.com",
          conflictedPorts: [31337],
          detail: "proxy failed",
        },
      }),
    );

    expect(result.badges).toEqual([
      {
        kind: "remote",
        id: "remote:ssh:dev.example.com",
        label: "SSH dev.example.com",
        tone: "secondaryStale",
        status: "error",
        statusLabel: "error",
        transport: "SSH",
        destination: "dev.example.com",
        title: "SSH dev.example.com | state: error | conflicts: 31337 | proxy failed",
      },
    ]);
  });
});
