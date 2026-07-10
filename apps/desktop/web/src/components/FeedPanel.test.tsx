import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { FeedPanel, FeedPanelContent, type FeedItemView } from "./FeedPanel";

describe("FeedPanel", () => {
  test("renders the canonical actionable empty state", () => {
    const markup = renderToStaticMarkup(<FeedPanel />);
    expect(markup).toContain('aria-label="Feed"');
    expect(markup).toContain("Actionable");
    expect(markup).toContain("All Activity");
    expect(markup).toContain("No pending decisions");
    expect(markup).toContain(
      "Permission, plan, and question requests from AI agents will appear here.",
    );
  });

  test("renders actionable permission cards and canonical decisions", () => {
    const item: FeedItemView = {
      id: "item-1",
      workstream_id: "claude-session-1",
      source: "claude",
      kind: "permissionRequest",
      status: "pending",
      title: "Write",
      cwd: "C:/repo",
      request_id: "request-1",
      tool_name: "Write",
      tool_input: '{"file_path":"C:/repo/README.md"}',
      questions: [],
    };
    const markup = renderToStaticMarkup(
      <FeedPanelContent
        filter="actionable"
        items={[item]}
        loading={false}
        error={null}
        onFilterChange={() => {}}
        onResolve={() => {}}
      />,
    );

    expect(markup).toContain("PERMISSION");
    expect(markup).toContain("Write");
    expect(markup).toContain("README.md");
    expect(markup).toContain("Allow Once");
    expect(markup).toContain("Always Allow");
    expect(markup).toContain("Deny");
  });

  test("activity filter includes telemetry rows while actionable excludes them", () => {
    const telemetry: FeedItemView = {
      id: "item-2",
      workstream_id: "codex-session-1",
      source: "codex",
      kind: "stop",
      status: "telemetry",
      title: "Agent stopped",
      cwd: null,
      request_id: null,
      tool_name: null,
      tool_input: "cargo test",
      questions: [],
    };
    expect(
      renderToStaticMarkup(
        <FeedPanelContent
          filter="actionable"
          items={[telemetry]}
          loading={false}
          error={null}
          onFilterChange={() => {}}
          onResolve={() => {}}
        />,
      ),
    ).not.toContain("cargo test");
    expect(
      renderToStaticMarkup(
        <FeedPanelContent
          filter="activity"
          items={[telemetry]}
          loading={false}
          error={null}
          onFilterChange={() => {}}
          onResolve={() => {}}
        />,
      ),
    ).toContain("cargo test");
  });
});
