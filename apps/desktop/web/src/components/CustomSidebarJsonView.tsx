import type {
  CustomSidebarJsonAction,
  CustomSidebarJsonBlock,
  CustomSidebarJsonDocument,
  JsonTemplateContext,
  TabPreview,
  TemplateContext,
  WorkspaceListFilter,
  WorkspacePreview,
} from "./customSidebarModel";
import { interpolateCustomSidebarTemplate } from "./customSidebarSwiftParser";

export function JsonSidebarBody({
  document,
  error,
  loading,
  previews,
  selected,
  context,
  onWorkspaceAction,
  onTabAction,
  onCustomAction,
}: {
  document: CustomSidebarJsonDocument | null;
  error: string | null;
  loading: boolean;
  previews: WorkspacePreview[];
  selected: WorkspacePreview | undefined;
  context: JsonTemplateContext;
  onWorkspaceAction: (workspace: WorkspacePreview) => void;
  onTabAction: (tab: TabPreview) => void;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
}): React.JSX.Element {
  if (loading) {
    return <div className="cmux-custom-sidebar-empty">Loading JSON sidebar...</div>;
  }
  if (error !== null) {
    return (
      <div className="cmux-custom-sidebar-error" role="alert">
        {error}
      </div>
    );
  }
  const blocks = document?.blocks ?? [{ type: "workspaceList" as const }];
  return (
    <div className="cmux-custom-sidebar-json-body">
      {blocks.map((block, index) => (
        <JsonSidebarBlock
          key={index}
          block={block}
          previews={previews}
          selected={selected}
          context={context}
          onWorkspaceAction={onWorkspaceAction}
          onTabAction={onTabAction}
          onCustomAction={onCustomAction}
        />
      ))}
    </div>
  );
}

function JsonSidebarBlock({
  block,
  previews,
  selected,
  context,
  onWorkspaceAction,
  onTabAction,
  onCustomAction,
}: {
  block: CustomSidebarJsonBlock;
  previews: WorkspacePreview[];
  selected: WorkspacePreview | undefined;
  context: JsonTemplateContext;
  onWorkspaceAction: (workspace: WorkspacePreview) => void;
  onTabAction: (tab: TabPreview) => void;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
}): React.JSX.Element | null {
  switch (block.type) {
    case "heading":
      return (
        <h3 className="cmux-custom-sidebar-json-heading">
          {interpolateCustomSidebarTemplate(block.text, context)}
        </h3>
      );
    case "text":
      return (
        <p className="cmux-custom-sidebar-json-text">
          {interpolateCustomSidebarTemplate(block.text, context)}
        </p>
      );
    case "divider":
      return <div className="cmux-custom-sidebar-json-divider" aria-hidden="true" />;
    case "stat":
      return (
        <div className="cmux-custom-sidebar-json-stat">
          <span>{interpolateCustomSidebarTemplate(block.label, context)}</span>
          <strong>{interpolateCustomSidebarTemplate(block.value, context)}</strong>
        </div>
      );
    case "button":
      return (
        <button
          type="button"
          className="cmux-custom-sidebar-json-button"
          disabled={block.action === undefined}
          onClick={() => {
            if (block.action !== undefined) {
              onCustomAction(block.action);
            }
          }}
        >
          <span>{interpolateCustomSidebarTemplate(block.label ?? "Run action", context)}</span>
          {block.detail ? (
            <small>{interpolateCustomSidebarTemplate(block.detail, context)}</small>
          ) : null}
        </button>
      );
    case "workspaceList":
      return (
        <WorkspaceListBlock
          block={block}
          previews={previews}
          onWorkspaceAction={onWorkspaceAction}
          onCustomAction={onCustomAction}
        />
      );
    case "selectedTabs":
      return (
        <SelectedTabsBlock
          block={block}
          tabs={selected?.tabs ?? []}
          onTabAction={onTabAction}
          onCustomAction={onCustomAction}
        />
      );
  }
}

function WorkspaceListBlock({
  block,
  previews,
  onWorkspaceAction,
  onCustomAction,
}: {
  block: Extract<CustomSidebarJsonBlock, { type: "workspaceList" }>;
  previews: WorkspacePreview[];
  onWorkspaceAction: (workspace: WorkspacePreview) => void;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
}): React.JSX.Element {
  const filtered = filterWorkspaces(previews, block.filter).slice(0, jsonLimit(block.limit));
  return (
    <section className="cmux-custom-sidebar-json-section">
      <h3>{block.title ?? "Workspaces"}</h3>
      <div className="cmux-custom-sidebar-json-list">
        {filtered.length === 0 ? (
          <div className="cmux-custom-sidebar-empty">No matching workspaces.</div>
        ) : (
          filtered.map((workspace) => (
            <button
              key={workspace.id}
              type="button"
              className={
                workspace.selected
                  ? "cmux-custom-sidebar-json-row cmux-custom-sidebar-json-row-selected"
                  : "cmux-custom-sidebar-json-row"
              }
              onClick={() => {
                if (typeof block.action === "object") {
                  onCustomAction(block.action, { workspace });
                } else if (block.action !== "none") {
                  onWorkspaceAction(workspace);
                }
              }}
            >
              <span>{workspace.title}</span>
              <small>
                {workspace.branch ?? `${workspace.tabCount} tabs`}
                {workspace.unreadCount > 0 ? ` · ${workspace.unreadCount} unread` : ""}
                {workspace.ports.length > 0 ? ` · :${workspace.ports.join(" :")}` : ""}
              </small>
            </button>
          ))
        )}
      </div>
    </section>
  );
}

function SelectedTabsBlock({
  block,
  tabs,
  onTabAction,
  onCustomAction,
}: {
  block: Extract<CustomSidebarJsonBlock, { type: "selectedTabs" }>;
  tabs: TabPreview[];
  onTabAction: (tab: TabPreview) => void;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
}): React.JSX.Element {
  const visible = tabs.slice(0, jsonLimit(block.limit));
  return (
    <section className="cmux-custom-sidebar-json-section">
      <h3>{block.title ?? "Selected tabs"}</h3>
      <div className="cmux-custom-sidebar-json-list">
        {visible.length === 0 ? (
          <div className="cmux-custom-sidebar-empty">No tabs in the selected workspace.</div>
        ) : (
          visible.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={
                tab.focused
                  ? "cmux-custom-sidebar-json-row cmux-custom-sidebar-json-row-selected"
                  : "cmux-custom-sidebar-json-row"
              }
              onClick={() => {
                if (typeof block.action === "object") {
                  onCustomAction(block.action, { tab });
                } else if (block.action !== "none") {
                  onTabAction(tab);
                }
              }}
            >
              <span>{tab.title}</span>
              <small>
                {tab.branch ?? tab.directory ?? tab.id}
                {tab.ports.length > 0 ? ` · :${tab.ports.join(" :")}` : ""}
              </small>
            </button>
          ))
        )}
      </div>
    </section>
  );
}

function jsonLimit(value: number | undefined): number {
  if (value === undefined || !Number.isFinite(value)) {
    return 50;
  }
  return Math.max(0, Math.min(100, Math.trunc(value)));
}

function filterWorkspaces(
  previews: WorkspacePreview[],
  filter: WorkspaceListFilter | undefined,
): WorkspacePreview[] {
  switch (filter) {
    case "selected":
      return previews.filter((workspace) => workspace.selected);
    case "unread":
      return previews.filter((workspace) => workspace.unreadCount > 0);
    case "ports":
      return previews.filter((workspace) => workspace.ports.length > 0);
    case "dirty":
      return previews.filter((workspace) => workspace.dirty);
    case "remote":
      return previews.filter((workspace) => workspace.remoteState !== undefined);
    case "all":
    case undefined:
      return previews;
  }
}
