import { useEffect, useState } from "react";

import { host } from "../host/host";

export type FeedFilter = "actionable" | "activity";

export interface FeedQuestionOptionView {
  id: string;
  label: string;
  description?: string | null;
}

export interface FeedQuestionView {
  id: string;
  header?: string | null;
  prompt: string;
  multi_select: boolean;
  options: FeedQuestionOptionView[];
}

export interface FeedContextView {
  lastUserMessage?: string | null;
  assistantPreamble?: string | null;
  planSummary?: string | null;
  allowedPrompts?: readonly { tool: string; prompt: string }[];
  toolSummary?: string | null;
  permissionMode?: string | null;
}

export interface QuestionDraft {
  selectedOptionIds: Readonly<Record<string, readonly string[]>>;
  freeTextByQuestion: Readonly<Record<string, string>>;
}

const CUSTOM_QUESTION_ANSWER_ID = "__cmux_custom_answer__";
export const SKIP_INTERVIEW_AND_PLAN_ANSWER = "Skip interview and plan immediately";
const PLAN_INTERVIEW_CLUES = [
  "plan mode",
  "make a plan",
  "plan-only",
  "plan immediately",
] as const;

export function isPlanInterviewQuestion(
  source: string,
  context: FeedContextView | undefined,
  questions: readonly FeedQuestionView[],
): boolean {
  if (source !== "claude") return false;
  if (context?.permissionMode?.toLowerCase() === "plan") return true;
  const fragments = questions.flatMap((question) => [
    question.header,
    question.prompt,
    ...question.options.flatMap((option) => [option.label, option.description]),
  ]);
  const text = [context?.lastUserMessage, context?.assistantPreamble, ...fragments]
    .filter((value): value is string => value != null)
    .join(" ")
    .toLowerCase();
  return PLAN_INTERVIEW_CLUES.some((clue) => text.includes(clue));
}

export function emptyQuestionDraft(): QuestionDraft {
  return { selectedOptionIds: {}, freeTextByQuestion: {} };
}

export function toggleQuestionOption(
  draft: QuestionDraft,
  question: FeedQuestionView,
  optionId: string,
): QuestionDraft {
  const current = draft.selectedOptionIds[question.id] ?? [];
  const selected = question.multi_select
    ? current.includes(optionId)
      ? current.filter((id) => id !== optionId)
      : [...current, optionId]
    : [optionId];
  return {
    ...draft,
    selectedOptionIds: { ...draft.selectedOptionIds, [question.id]: selected },
  };
}

export function setQuestionFreeText(
  draft: QuestionDraft,
  question: FeedQuestionView,
  value: string,
): QuestionDraft {
  const current = draft.selectedOptionIds[question.id] ?? [];
  const hasText = value.trim() !== "";
  const presetSelections = current.filter((id) => id !== CUSTOM_QUESTION_ANSWER_ID);
  const selected = !hasText
    ? presetSelections
    : question.multi_select
      ? [...presetSelections, CUSTOM_QUESTION_ANSWER_ID]
      : [CUSTOM_QUESTION_ANSWER_ID];
  return {
    selectedOptionIds: { ...draft.selectedOptionIds, [question.id]: selected },
    freeTextByQuestion: { ...draft.freeTextByQuestion, [question.id]: value },
  };
}

export function composeQuestionAnswers(
  questions: readonly FeedQuestionView[],
  draft: QuestionDraft,
): string[] {
  const answers: string[] = [];
  for (const question of questions) {
    const selected = draft.selectedOptionIds[question.id] ?? [];
    const freeText = (draft.freeTextByQuestion[question.id] ?? "").trim();
    if (freeText !== "" && selected.includes(CUSTOM_QUESTION_ANSWER_ID)) {
      answers.push(freeText);
      continue;
    }
    const labels = question.options
      .filter((option) => selected.includes(option.id))
      .map((option) => option.label);
    if (labels.length > 0) {
      answers.push(labels.join(", "));
    }
  }
  return answers;
}

export function canSubmitQuestionAnswers(
  questions: readonly FeedQuestionView[],
  draft: QuestionDraft,
): boolean {
  return (
    composeQuestionAnswers(questions, draft).length > 0 ||
    (questions.length > 0 && questions.every((question) => question.options.length === 0))
  );
}

export interface FeedItemView {
  id: string;
  workstream_id: string;
  source: string;
  kind: string;
  status: "pending" | "resolved" | "expired" | "telemetry";
  title?: string | null;
  cwd?: string | null;
  request_id?: string | null;
  tool_name?: string | null;
  tool_input?: string | null;
  plan?: string | null;
  default_mode?: string | null;
  questions: FeedQuestionView[];
  context?: FeedContextView;
}

interface FeedListReply {
  items: FeedItemView[];
  pending_count: number;
  total_count: number;
  has_more_persisted_items: boolean;
}

export type FeedDecision =
  | { kind: "permission"; mode: "once" | "always" | "all" | "bypass" | "deny" }
  | { kind: "question"; selections: string[] }
  | {
      kind: "exit_plan";
      mode: "ultraplan" | "bypassPermissions" | "autoAccept" | "manual" | "deny";
      feedback?: string;
    };

export function FeedPanel(): React.JSX.Element {
  const [filter, setFilter] = useState<FeedFilter>("actionable");
  const [items, setItems] = useState<FeedItemView[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [hasMorePersistedItems, setHasMorePersistedItems] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const apply = (reply: FeedListReply) => {
      if (!disposed) {
        applyFeedReply(reply, setItems, setHasMorePersistedItems, setError);
        setLoading(false);
      }
    };
    setLoading(true);
    void host.invoke<FeedListReply>("feed_list").then(apply).catch((reason) => {
      if (!disposed) {
        setLoading(false);
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    });
    void host
      .on<FeedListReply>("cmux://feed-changed", apply)
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      })
      .catch((reason) => {
        if (!disposed) {
          setError(reason instanceof Error ? reason.message : String(reason));
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const resolve = (requestId: string, decision: FeedDecision) => {
    void host
      .invoke<FeedListReply>("feed_resolve", { requestId, decision })
      .then((reply) => applyFeedReply(reply, setItems, setHasMorePersistedItems, setError))
      .catch((reason) =>
        setError(reason instanceof Error ? reason.message : String(reason)),
      );
  };

  const loadOlder = () => {
    if (loadingOlder || !hasMorePersistedItems) return;
    setLoadingOlder(true);
    void host
      .invoke<FeedListReply>("feed_load_older")
      .then((reply) => {
        applyFeedReply(reply, setItems, setHasMorePersistedItems, setError);
        setLoadingOlder(false);
      })
      .catch((reason) => {
        setError(reason instanceof Error ? reason.message : String(reason));
        setLoadingOlder(false);
      });
  };

  return (
    <FeedPanelContent
      filter={filter}
      items={items}
      loading={loading}
      error={error}
      hasMorePersistedItems={hasMorePersistedItems}
      isLoadingOlderItems={loadingOlder}
      onFilterChange={setFilter}
      onResolve={resolve}
      onLoadOlderItems={loadOlder}
    />
  );
}

export interface FeedPanelContentProps {
  filter: FeedFilter;
  items: readonly FeedItemView[];
  loading: boolean;
  error: string | null;
  hasMorePersistedItems?: boolean;
  isLoadingOlderItems?: boolean;
  onFilterChange: (filter: FeedFilter) => void;
  onResolve: (requestId: string, decision: FeedDecision) => void;
  onLoadOlderItems?: () => void;
}

export function FeedPanelContent({
  filter,
  items,
  loading,
  error,
  hasMorePersistedItems = false,
  isLoadingOlderItems = false,
  onFilterChange,
  onResolve,
  onLoadOlderItems,
}: FeedPanelContentProps): React.JSX.Element {
  const actionable = filter === "actionable";
  const visibleItems = actionable ? items.filter((item) => isActionableKind(item.kind)) : items;

  return (
    <section className="cmux-feed-panel" aria-label="Feed">
      <div className="cmux-file-explorer-toolbar" role="tablist" aria-label="Feed filter">
        <button
          type="button"
          role="tab"
          aria-selected={actionable}
          onClick={() => onFilterChange("actionable")}
        >
          Actionable
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={!actionable}
          onClick={() => onFilterChange("activity")}
        >
          All Activity
        </button>
      </div>
      {error != null && <p className="cmux-file-explorer-status">{error}</p>}
      {loading ? (
        <div className="cmux-file-explorer-empty">Loading Feed…</div>
      ) : visibleItems.length === 0 && !(filter === "activity" && hasMorePersistedItems) ? (
        <FeedEmptyState actionable={actionable} />
      ) : (
        <ul className="cmux-feed-list">
          {visibleItems.map((item) => (
            <li key={item.id} className="cmux-feed-item">
              <FeedItemCard item={item} onResolve={onResolve} />
            </li>
          ))}
          {filter === "activity" && hasMorePersistedItems && (
            <li className="cmux-feed-history-loader">
              <button
                type="button"
                disabled={isLoadingOlderItems}
                onClick={onLoadOlderItems}
              >
                {isLoadingOlderItems
                  ? "Loading older activity..."
                  : "Load older activity"}
              </button>
            </li>
          )}
        </ul>
      )}
    </section>
  );
}

function applyFeedReply(
  reply: FeedListReply,
  setItems: (items: FeedItemView[]) => void,
  setHasMore: (hasMore: boolean) => void,
  setError: (error: string | null) => void,
): void {
  setItems(reply.items);
  setHasMore(reply.has_more_persisted_items);
  setError(null);
}

function FeedEmptyState({ actionable }: { actionable: boolean }): React.JSX.Element {
  return (
    <div className="cmux-file-explorer-empty">
      <strong>{actionable ? "No pending decisions" : "No activity yet"}</strong>
      <span>
        {actionable
          ? "Permission, plan, and question requests from AI agents will appear here."
          : "Agent decisions and todo-list updates will appear here."}
      </span>
    </div>
  );
}

function FeedItemCard({
  item,
  onResolve,
}: {
  item: FeedItemView;
  onResolve: FeedPanelContentProps["onResolve"];
}): React.JSX.Element {
  const pending = item.status === "pending" && item.request_id != null;
  const detail = item.tool_input ?? item.plan ?? item.cwd ?? "";
  return (
    <article className="cmux-feed-card" data-feed-kind={item.kind} data-feed-status={item.status}>
      <div className="cmux-feed-card-header">
        <span className="cmux-feed-kind">{feedKindLabel(item.kind)}</span>
        <span className="cmux-feed-source">{item.source}</span>
      </div>
      <strong>{item.title ?? item.tool_name ?? feedKindLabel(item.kind)}</strong>
      {detail !== "" && <pre className="cmux-feed-detail">{detail}</pre>}
      {pending && item.kind === "permissionRequest" && (
        <div className="cmux-feed-actions">
          <button
            type="button"
            onClick={() => onResolve(item.request_id!, { kind: "permission", mode: "once" })}
          >
            Allow Once
          </button>
          <button
            type="button"
            onClick={() =>
              onResolve(item.request_id!, { kind: "permission", mode: "always" })
            }
          >
            Always Allow
          </button>
          <button
            type="button"
            onClick={() => onResolve(item.request_id!, { kind: "permission", mode: "deny" })}
          >
            Deny
          </button>
        </div>
      )}
      {pending && item.kind === "exitPlan" && (
        <div className="cmux-feed-actions">
          <button
            type="button"
            onClick={() => onResolve(item.request_id!, { kind: "exit_plan", mode: "manual" })}
          >
            Approve Plan
          </button>
          <button
            type="button"
            onClick={() => onResolve(item.request_id!, { kind: "exit_plan", mode: "autoAccept" })}
          >
            Auto Accept
          </button>
          <button
            type="button"
            onClick={() => onResolve(item.request_id!, { kind: "exit_plan", mode: "deny" })}
          >
            Deny
          </button>
        </div>
      )}
      {item.kind === "question" && (
        <QuestionActionArea
          key={item.request_id ?? item.id}
          questions={item.questions}
          source={item.source}
          context={item.context}
          pending={pending}
          onReply={(selections) =>
            onResolve(item.request_id!, { kind: "question", selections })
          }
        />
      )}
    </article>
  );
}

function QuestionActionArea({
  questions,
  source,
  context,
  pending,
  onReply,
}: {
  questions: readonly FeedQuestionView[];
  source: string;
  context: FeedContextView | undefined;
  pending: boolean;
  onReply: (selections: string[]) => void;
}): React.JSX.Element {
  const [draft, setDraft] = useState<QuestionDraft>(emptyQuestionDraft);
  const answers = composeQuestionAnswers(questions, draft);
  const canSubmit = pending && canSubmitQuestionAnswers(questions, draft);
  const showSkipInterview = pending && isPlanInterviewQuestion(source, context, questions);

  return (
    <div className="cmux-feed-question-area">
      {questions.map((question, index) => {
        const selected = draft.selectedOptionIds[question.id] ?? [];
        return (
          <section key={question.id} className="cmux-feed-question-block">
            <div className="cmux-feed-question-heading">
              <span>{index + 1}.</span>
              <div>
                {question.header != null && question.header !== "" && (
                  <strong>{question.header}</strong>
                )}
                <p>{question.prompt}</p>
              </div>
            </div>
            {question.multi_select && (
              <span className="cmux-feed-question-multi">Multi-select</span>
            )}
            {question.options.length === 0 ? (
              <span className="cmux-feed-question-empty">Agent provided no options.</span>
            ) : (
              <div className="cmux-feed-question-options">
                {question.options.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    aria-pressed={selected.includes(option.id)}
                    disabled={!pending}
                    onClick={() =>
                      setDraft((current) => toggleQuestionOption(current, question, option.id))
                    }
                  >
                    <span>{option.label}</span>
                    {option.description != null && option.description !== "" && (
                      <small>{option.description}</small>
                    )}
                  </button>
                ))}
              </div>
            )}
            {pending && (
              <input
                className="cmux-feed-question-free-text"
                type="text"
                value={draft.freeTextByQuestion[question.id] ?? ""}
                placeholder="Type something..."
                aria-label={`Custom answer for ${question.header || question.prompt}`}
                onChange={(event) =>
                  setDraft((current) => setQuestionFreeText(current, question, event.target.value))
                }
              />
            )}
          </section>
        );
      })}
      <div className="cmux-feed-question-actions">
        {showSkipInterview && (
          <button
            className="cmux-feed-question-skip"
            type="button"
            onClick={() => onReply([SKIP_INTERVIEW_AND_PLAN_ANSWER])}
          >
            Skip + plan immediately
          </button>
        )}
        <button
          className="cmux-feed-question-submit"
          type="button"
          disabled={!canSubmit}
          onClick={() => onReply(answers)}
        >
          {pending ? "Submit All Answers" : "Submitted"}
        </button>
      </div>
    </div>
  );
}

function isActionableKind(kind: string): boolean {
  return kind === "permissionRequest" || kind === "exitPlan" || kind === "question";
}

function feedKindLabel(kind: string): string {
  switch (kind) {
    case "permissionRequest":
      return "PERMISSION";
    case "exitPlan":
      return "PLAN";
    case "question":
      return "QUESTION";
    case "todos":
      return "TASKS";
    case "stop":
      return "STOPPED";
    default:
      return kind.replace(/([a-z])([A-Z])/g, "$1 $2").toUpperCase();
  }
}
