import { useState } from "react";

type FeedFilter = "actionable" | "activity";

export function FeedPanel(): React.JSX.Element {
  const [filter, setFilter] = useState<FeedFilter>("actionable");
  const actionable = filter === "actionable";

  return (
    <section className="cmux-feed-panel" aria-label="Feed">
      <div className="cmux-file-explorer-toolbar" role="tablist" aria-label="Feed filter">
        <button
          type="button"
          role="tab"
          aria-selected={actionable}
          onClick={() => setFilter("actionable")}
        >
          Actionable
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={!actionable}
          onClick={() => setFilter("activity")}
        >
          All Activity
        </button>
      </div>
      <div className="cmux-file-explorer-empty">
        <strong>{actionable ? "No pending decisions" : "No activity yet"}</strong>
        <span>
          {actionable
            ? "Permission, plan, and question requests from AI agents will appear here."
            : "Agent decisions and todo-list updates will appear here."}
        </span>
      </div>
    </section>
  );
}
