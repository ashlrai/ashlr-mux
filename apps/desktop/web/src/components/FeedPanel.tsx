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
}

interface FeedListReply {
  items: FeedItemView[];
  pending_count: number;
  total_count: number;
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
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const apply = (reply: FeedListReply) => {
      if (!disposed) {
        setItems(reply.items);
        setLoading(false);
        setError(null);
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
      .then((reply) => {
        setItems(reply.items);
        setError(null);
      })
      .catch((reason) =>
        setError(reason instanceof Error ? reason.message : String(reason)),
      );
  };

  return (
    <FeedPanelContent
      filter={filter}
      items={items}
      loading={loading}
      error={error}
      onFilterChange={setFilter}
      onResolve={resolve}
    />
  );
}

export interface FeedPanelContentProps {
  filter: FeedFilter;
  items: readonly FeedItemView[];
  loading: boolean;
  error: string | null;
  onFilterChange: (filter: FeedFilter) => void;
  onResolve: (requestId: string, decision: FeedDecision) => void;
}

export function FeedPanelContent({
  filter,
  items,
  loading,
  error,
  onFilterChange,
  onResolve,
}: FeedPanelContentProps): React.JSX.Element {
  const actionable = filter === "actionable";
  const visibleItems = items.filter((item) =>
    actionable
      ? isActionableKind(item.kind)
      : isActionableKind(item.kind) || item.kind === "todos" || item.kind === "stop",
  );

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
      ) : visibleItems.length === 0 ? (
        <FeedEmptyState actionable={actionable} />
      ) : (
        <ul className="cmux-feed-list">
          {visibleItems.map((item) => (
            <li key={item.id} className="cmux-feed-item">
              <FeedItemCard item={item} onResolve={onResolve} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
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
  const detail =
    item.tool_input ?? item.plan ?? item.questions[0]?.prompt ?? item.cwd ?? "";
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
      {pending && item.kind === "question" && (
        <div className="cmux-feed-actions">
          {item.questions.flatMap((question) =>
            question.options.map((option) => (
              <button
                key={`${question.id}:${option.id}`}
                type="button"
                title={option.description ?? undefined}
                onClick={() =>
                  onResolve(item.request_id!, {
                    kind: "question",
                    selections: [option.label],
                  })
                }
              >
                {option.label}
              </button>
            )),
          )}
        </div>
      )}
    </article>
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
