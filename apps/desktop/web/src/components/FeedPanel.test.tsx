import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import {
  canSubmitQuestionAnswers,
  composeQuestionAnswers,
  emptyQuestionDraft,
  FeedPanel,
  FeedPanelContent,
  isPlanInterviewQuestion,
  SKIP_INTERVIEW_AND_PLAN_ANSWER,
  setQuestionFreeText,
  toggleQuestionOption,
  type FeedItemView,
  type FeedQuestionView,
} from "./FeedPanel";

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
      kind: "toolUse",
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

  test("offers older persisted history only from All Activity", () => {
    const props = {
      items: [] as FeedItemView[],
      loading: false,
      error: null,
      hasMorePersistedItems: true,
      isLoadingOlderItems: false,
      onFilterChange: () => {},
      onResolve: () => {},
      onLoadOlderItems: () => {},
    };
    expect(
      renderToStaticMarkup(<FeedPanelContent {...props} filter="actionable" />),
    ).not.toContain("Load older activity");
    expect(renderToStaticMarkup(<FeedPanelContent {...props} filter="activity" />)).toContain(
      "Load older activity",
    );
    expect(
      renderToStaticMarkup(
        <FeedPanelContent {...props} filter="activity" isLoadingOlderItems={true} />,
      ),
    ).toContain("Loading older activity...");
  });

  test("question draft supports single-select and multi-select before one composed reply", () => {
    const questions: FeedQuestionView[] = [
      {
        id: "language",
        header: "Language",
        prompt: "Which language?",
        multi_select: false,
        options: [
          { id: "rust", label: "Rust" },
          { id: "go", label: "Go" },
        ],
      },
      {
        id: "priorities",
        header: "Priorities",
        prompt: "What matters?",
        multi_select: true,
        options: [
          { id: "fast", label: "Fast" },
          { id: "safe", label: "Safe" },
        ],
      },
    ];
    let draft = toggleQuestionOption(emptyQuestionDraft(), questions[0], "rust");
    draft = toggleQuestionOption(draft, questions[0], "go");
    draft = toggleQuestionOption(draft, questions[1], "fast");
    draft = toggleQuestionOption(draft, questions[1], "safe");

    expect(composeQuestionAnswers(questions, draft)).toEqual(["Go", "Fast, Safe"]);
    expect(canSubmitQuestionAnswers(questions, draft)).toBe(true);
  });

  test("question free-form text wins while active and clearing it restores preset choices", () => {
    const question: FeedQuestionView = {
      id: "details",
      prompt: "Anything else?",
      multi_select: true,
      options: [{ id: "tests", label: "Add tests" }],
    };
    let draft = toggleQuestionOption(emptyQuestionDraft(), question, "tests");
    draft = setQuestionFreeText(draft, question, "  Include benchmarks  ");
    expect(composeQuestionAnswers([question], draft)).toEqual(["Include benchmarks"]);

    draft = setQuestionFreeText(draft, question, "  ");
    expect(composeQuestionAnswers([question], draft)).toEqual(["Add tests"]);
  });

  test("renders every question with descriptions, free-form fields, and one submit action", () => {
    const questionItem: FeedItemView = {
      id: "item-question",
      workstream_id: "claude-session-question",
      source: "claude",
      kind: "question",
      status: "pending",
      request_id: "request-question",
      questions: [
        {
          id: "approach",
          header: "Approach",
          prompt: "Choose an approach",
          multi_select: false,
          options: [
            { id: "simple", label: "Simple", description: "Minimize moving parts" },
          ],
        },
        {
          id: "checks",
          header: "Checks",
          prompt: "Choose checks",
          multi_select: true,
          options: [{ id: "tests", label: "Tests" }],
        },
      ],
    };
    const markup = renderToStaticMarkup(
      <FeedPanelContent
        filter="actionable"
        items={[questionItem]}
        loading={false}
        error={null}
        onFilterChange={() => {}}
        onResolve={() => {}}
      />,
    );

    expect(markup).toContain("Approach");
    expect(markup).toContain("Choose an approach");
    expect(markup).toContain("Minimize moving parts");
    expect(markup).toContain("Multi-select");
    expect(markup.match(/Type something\.\.\./g)?.length).toBe(2);
    expect(markup.match(/Submit All Answers/g)?.length).toBe(1);
    expect(markup).toContain("disabled");
  });

  test("recognizes only Claude plan interviews from mode or canonical text clues", () => {
    const questions: FeedQuestionView[] = [
      {
        id: "approach",
        header: "Plan mode",
        prompt: "Would you like to make a plan?",
        multi_select: false,
        options: [{ id: "interview", label: "Continue interview" }],
      },
    ];
    expect(isPlanInterviewQuestion("claude", { permissionMode: "PLAN" }, questions)).toBe(
      true,
    );
    expect(isPlanInterviewQuestion("claude", undefined, questions)).toBe(true);
    expect(isPlanInterviewQuestion("codex", { permissionMode: "plan" }, questions)).toBe(
      false,
    );
    expect(SKIP_INTERVIEW_AND_PLAN_ANSWER).toBe("Skip interview and plan immediately");
  });

  test("renders the canonical skip-interview action for pending Claude plan questions", () => {
    const questionItem: FeedItemView = {
      id: "item-plan-question",
      workstream_id: "claude-plan-session",
      source: "claude",
      kind: "question",
      status: "pending",
      request_id: "request-plan-question",
      context: { permissionMode: "plan" },
      questions: [
        {
          id: "approach",
          prompt: "Which approach should I plan?",
          multi_select: false,
          options: [{ id: "simple", label: "Simple" }],
        },
      ],
    };
    const markup = renderToStaticMarkup(
      <FeedPanelContent
        filter="actionable"
        items={[questionItem]}
        loading={false}
        error={null}
        onFilterChange={() => {}}
        onResolve={() => {}}
      />,
    );

    expect(markup).toContain("Skip + plan immediately");
    expect(markup.match(/Submit All Answers/g)?.length).toBe(1);
  });
});
