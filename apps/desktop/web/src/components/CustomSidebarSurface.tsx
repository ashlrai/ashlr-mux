import {
  useContext,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type DragEventHandler,
  type FocusEventHandler,
  type KeyboardEventHandler,
  type MouseEventHandler,
  type ReactNode,
} from "react";
import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import { readFileExplorerFile } from "../host/fileExplorer";
import { host } from "../host/host";
import { useSession } from "../hooks/useSession";
import { orderedPanelIdsFromLayout } from "../sidebar/sessionBadges";
import { NativeBridgeError } from "../tauri-bridge";
import { JsonSidebarBody } from "./CustomSidebarJsonView";

import {
  type CustomSidebarAssetMap,
  customSidebarAssetMapFromSnapshot,
  type CustomSidebarEventFrame,
  type CustomSidebarEventsContext,
  customSidebarEventsContextFromSnapshot,
  type CustomSidebarJsonAction,
  type CustomSidebarJsonDocument,
  type CustomSidebarReloadEvent,
  customSidebarReloadMatches,
  customSidebarSourceKind,
  customSidebarSourceName,
  type CustomSidebarSwiftDocument,
  customSidebarSwiftEventHandlerMatches,
  type CustomSidebarSwiftLocalHandler,
  type CustomSidebarSwiftLocalHandlerModifierName,
  type CustomSidebarSwiftModifier,
  type CustomSidebarSwiftNode,
  type CustomSidebarSwiftStateAssignment,
  type CustomSidebarSwiftTextRun,
  emptyCustomSidebarEventsContext,
  type JsonTemplateContext,
  nextCustomSidebarEventsContext,
  parseCustomSidebarJson,
  setSwiftSidebarStateValue,
  type SwiftGridItemValue,
  SwiftNavigationContext,
  type SwiftNavigationContextValue,
  type SwiftNavigationEntry,
  SwiftPresentationContext,
  SwiftSidebarStateContext,
  type SwiftSidebarStateContextValue,
  type SwiftSidebarStateValue,
  type TabPreview,
  type TemplateContext,
  type WorkspacePreview,
} from "./customSidebarModel";
export * from "./customSidebarModel";
import {
  interpolateCustomSidebarTemplate,
  parseCustomSidebarSwift,
  resolveCustomSidebarActionParams,
} from "./customSidebarSwiftParser";
export * from "./customSidebarSwiftParser";

import {
  applySwiftForegroundStyle,
  swiftAlignmentGuideIsVertical,
  swiftBackgroundColor,
  swiftBackgroundStyle,
  swiftBlendMode,
  swiftColor,
  swiftContainerRelativeFrameSize,
  swiftDynamicTypeFontSize,
  swiftFontStyle,
  swiftFontWeight,
  swiftFontWidth,
  swiftGridCellAnchorPlaceSelf,
  swiftGridCellAnchorToken,
  swiftGridSelfAlignment,
  swiftHorizontalAlignmentToken,
  swiftKeyboardShortcutAria,
  swiftLazyStackClass,
  swiftMaterialBackdropFilter,
  swiftMaterialToken,
  swiftPinnedViewsClass,
  swiftRedactionReasons,
  swiftShadowColor,
  swiftStackAlignmentClass,
  swiftStackAlignmentStyle,
  swiftSystemImageGlyph,
  swiftTabViewStyleToken,
  swiftTextAlign,
  swiftTextDecorationStyle,
  swiftToken,
  swiftTransformOrigin,
} from "./customSidebarSwiftStyles";

const CUSTOM_SIDEBAR_RELOAD_EVENT = "cmux://custom-sidebar-reload";
const CONTROL_EVENTS_CHANGED_EVENT = "cmux://events-changed";

export interface CustomSidebarSurfaceProps {
  sourcePath?: string;
  sourceOverride?: string;
  assetOverride?: CustomSidebarAssetMap;
}

function invokeCustomSidebarAction(
  action: CustomSidebarJsonAction,
  context: TemplateContext,
  sourcePath?: string,
  onError?: (message: string | null) => void,
): void {
  const method = action.method.trim();
  if (method === "") {
    return;
  }
  onError?.(null);
  const params = resolveCustomSidebarActionParams(action.params ?? {}, context);
  if (typeof params !== "object" || params === null || Array.isArray(params)) {
    const message = "Custom sidebar action params must resolve to an object.";
    onError?.(message);
    console.error(message);
    return;
  }
  void host
    .invoke("custom_sidebar_action_invoke", {
      method,
      params,
      sourcePath,
    })
    .catch((error) => {
      onError?.(customSidebarActionErrorMessage(error));
      console.error("custom_sidebar_action_invoke failed", error);
    });
}

export function customSidebarActionErrorMessage(error: unknown): string {
  if (error instanceof NativeBridgeError) {
    if (error.code === "custom_sidebar_capability_denied") {
      const manifest = (error.data as { manifest?: unknown } | null)?.manifest;
      const denied = Array.isArray(
        (manifest as { denied_requested_methods?: unknown } | null)
          ?.denied_requested_methods,
      )
        ? ((manifest as { denied_requested_methods: unknown[] })
            .denied_requested_methods.filter(
              (method): method is string => typeof method === "string",
            ))
        : [];
      return denied.length > 0
        ? `${error.message}. Manifest requested: ${denied.join(", ")}. These are not granted by the safe default policy.`
        : error.message;
    }
    if (error.code === "custom_sidebar_action_schema_invalid") {
      const data = error.data as {
        accepted_keys?: unknown;
        expected?: unknown;
        field?: unknown;
      } | null;
      const field = typeof data?.field === "string" ? data.field : "params";
      const expected =
        typeof data?.expected === "string" ? data.expected : "valid action params";
      const acceptedKeys = Array.isArray(data?.accepted_keys)
        ? data.accepted_keys.filter(
            (key): key is string => typeof key === "string",
          )
        : [];
      return acceptedKeys.length > 0
        ? `Custom sidebar action params need ${field}: expected ${expected}. Accepted keys: ${acceptedKeys.join(", ")}.`
        : `Custom sidebar action params need ${field}: expected ${expected}.`;
    }
    return `Custom sidebar action failed: ${error.message}`;
  }
  return `Custom sidebar action failed: ${error instanceof Error ? error.message : String(error)}`;
}

function workspaceTitle(workspace: SessionWorkspaceSnapshot, index: number): string {
  return (
    workspace.custom_title?.trim() ||
    workspace.process_title?.trim() ||
    workspace.workspace_id?.trim() ||
    `Workspace ${index + 1}`
  );
}

function validPort(port: number | undefined): number | undefined {
  if (port === undefined || !Number.isInteger(port) || port < 1 || port > 65535) {
    return undefined;
  }
  return port;
}

function uniqueSortedPorts(ports: Array<number | undefined>): number[] {
  const seen = new Set<number>();
  for (const rawPort of ports) {
    const port = validPort(rawPort);
    if (port !== undefined) {
      seen.add(port);
    }
  }
  return [...seen].sort((a, b) => a - b);
}

function workspacePorts(workspace: SessionWorkspaceSnapshot): number[] {
  return uniqueSortedPorts([
    ...(workspace.listening_ports ?? []),
    ...(workspace.agent_listening_ports ?? []),
    ...(workspace.panel_listening_ports ?? []).flatMap((entry) => entry.ports),
  ]);
}

function panelPorts(workspace: SessionWorkspaceSnapshot, panelId: string): number[] {
  return uniqueSortedPorts(
    workspace.panel_listening_ports?.find((entry) => entry.panel_id === panelId)?.ports ?? [],
  );
}

function workspaceUnreadCount(workspace: SessionWorkspaceSnapshot): number {
  return (workspace.panel_unreads ?? []).filter((entry) => entry.is_unread).length;
}

function workspaceBranch(workspace: SessionWorkspaceSnapshot): {
  branch?: string;
  dirty: boolean;
} {
  const panelBranch = workspace.panel_git_branches?.find((entry) =>
    entry.branch?.trim(),
  );
  if (panelBranch !== undefined) {
    return {
      branch: panelBranch.branch,
      dirty: panelBranch.is_dirty === true,
    };
  }
  return {
    branch: workspace.git_branch?.branch,
    dirty: workspace.git_branch?.is_dirty === true,
  };
}

function panelBranch(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): { branch?: string; dirty: boolean } {
  const branch = workspace.panel_git_branches?.find((entry) => entry.panel_id === panelId);
  if (branch !== undefined) {
    return {
      branch: branch.branch,
      dirty: branch.is_dirty === true,
    };
  }
  return workspaceBranch(workspace);
}

function workspaceTabTitle(
  workspace: SessionWorkspaceSnapshot,
  panelId: string,
): string {
  const custom = workspace.panel_titles?.find((entry) => entry.panel_id === panelId)
    ?.custom_title;
  return custom?.trim() || panelId;
}

function progressPercent(workspace: SessionWorkspaceSnapshot): number | undefined {
  const value = workspace.sidebar_progress?.value;
  if (typeof value !== "number" || Number.isNaN(value)) {
    return undefined;
  }
  return Math.max(0, Math.min(100, Math.round(value * 100)));
}

function selectedPanelId(workspace: SessionWorkspaceSnapshot): string | undefined {
  const layout = workspace.layout;
  if (layout?.type === "pane") {
    return layout.pane.selected_panel_id ?? layout.pane.panel_ids[0];
  }
  return undefined;
}

function workspaceTabs(workspace: SessionWorkspaceSnapshot): TabPreview[] {
  const selected = selectedPanelId(workspace);
  return orderedPanelIdsFromLayout(workspace.layout).map((panelId) => {
    const branch = panelBranch(workspace, panelId);
    return {
      id: panelId,
      title: workspaceTabTitle(workspace, panelId),
      directory: workspace.current_directory,
      branch: branch.branch,
      dirty: branch.dirty,
      ports: panelPorts(workspace, panelId),
      focused: panelId === selected,
    };
  });
}

function workspacePreview(
  workspace: SessionWorkspaceSnapshot,
  index: number,
  selectedWorkspaceIndex: number,
): WorkspacePreview {
  const branch = workspaceBranch(workspace);
  const tabs = workspaceTabs(workspace);
  return {
    id: workspace.workspace_id ?? `workspace-${index}`,
    index,
    title: workspaceTitle(workspace, index),
    directory: workspace.current_directory,
    tabs,
    tabCount: tabs.length,
    unreadCount: workspaceUnreadCount(workspace),
    ports: workspacePorts(workspace),
    branch: branch.branch,
    dirty: branch.dirty,
    progress: progressPercent(workspace),
    statusCount: workspace.sidebar_status_entries?.length ?? 0,
    metadataCount:
      (workspace.sidebar_metadata_entries?.length ?? 0) +
      (workspace.sidebar_metadata_blocks?.length ?? 0),
    logCount: workspace.sidebar_log_entries?.length ?? 0,
    remoteState: workspace.remote?.state,
    selected: index === selectedWorkspaceIndex,
  };
}

function sourceKindLabel(sourceName: string): string {
  const lower = sourceName.toLowerCase();
  if (lower.endsWith(".swift")) return "Swift";
  if (lower.endsWith(".json")) return "JSON";
  return "Custom";
}

function detailRow(label: string, value: ReactNode): React.JSX.Element {
  return (
    <div className="cmux-custom-sidebar-detail-row">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function selectedWorkspace(
  previews: WorkspacePreview[],
  selectedWorkspaceIndex: number,
): WorkspacePreview | undefined {
  return previews[selectedWorkspaceIndex] ?? previews[0];
}

export function CustomSidebarSurface({
  sourcePath,
  sourceOverride,
  assetOverride,
}: CustomSidebarSurfaceProps): React.JSX.Element {
  const { workspaces, selectedWorkspaceIndex, selectWorkspace, selectWorkspaceSurface } =
    useSession();
  const [source, setSource] = useState<string | null>(sourceOverride ?? null);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [swiftStateValues, setSwiftStateValues] = useState<
    Record<string, SwiftSidebarStateValue>
  >({});
  const handledSwiftEventSeq = useRef<number | undefined>(undefined);
  const [eventsContext, setEventsContext] = useState<CustomSidebarEventsContext>(() =>
    emptyCustomSidebarEventsContext(),
  );
  const [assetMap, setAssetMap] = useState<CustomSidebarAssetMap>(() => assetOverride ?? {});
  const sourceName = customSidebarSourceName(sourcePath);
  const sourceKind = customSidebarSourceKind(sourcePath);
  const previews = workspaces.map((workspace, index) =>
    workspacePreview(workspace, index, selectedWorkspaceIndex),
  );
  const selected = selectedWorkspace(previews, selectedWorkspaceIndex);
  const unreadTotal = previews.reduce((total, workspace) => total + workspace.unreadCount, 0);
  const portTotal = previews.reduce((total, workspace) => total + workspace.ports.length, 0);
  const context: JsonTemplateContext = {
    sourceName,
    workspaceCount: workspaces.length,
    selectedTitle: selected?.title ?? "",
    selectedId: selected?.id ?? "",
    unreadTotal,
    portTotal,
    events: eventsContext,
    assets: assetMap,
    latestEventName:
      typeof eventsContext.latest?.name === "string" ? eventsContext.latest.name : "",
    latestEventCategory:
      typeof eventsContext.latest?.category === "string"
        ? eventsContext.latest.category
        : "",
    latestEventSeq: eventsContext.latest_seq,
  };

  useEffect(() => {
    if (assetOverride !== undefined) {
      setAssetMap(assetOverride);
    }
  }, [assetOverride]);

  useEffect(() => {
    let cancelled = false;
    let unlistenEvents: (() => void) | undefined;

    void host
      .invoke<{ events?: unknown; assets?: unknown }>("extension.sidebar.snapshot", {
        source_path: sourcePath ?? "",
      })
      .then((snapshot) => {
        if (!cancelled) {
          setEventsContext(customSidebarEventsContextFromSnapshot(snapshot.events));
          if (assetOverride === undefined) {
            setAssetMap(customSidebarAssetMapFromSnapshot(snapshot.assets));
          }
        }
      })
      .catch((error) => {
        console.error("custom sidebar events snapshot failed", error);
      });

    void host
      .on<CustomSidebarEventFrame>(CONTROL_EVENTS_CHANGED_EVENT, (event) => {
        if (!cancelled) {
          setEventsContext((current) => nextCustomSidebarEventsContext(current, event));
        }
      })
      .then((unlisten) => {
        if (!cancelled) {
          unlistenEvents = unlisten;
        } else {
          unlisten();
        }
      })
      .catch((error) => {
        console.error("custom sidebar events listener failed", error);
      });

    return () => {
      cancelled = true;
      unlistenEvents?.();
    };
  }, [assetOverride, sourcePath]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: number | undefined;
    let unlistenReload: (() => void) | undefined;
    let lastSource = sourceOverride ?? null;
    setSource(sourceOverride ?? null);
    setSourceError(null);
    if (sourceOverride !== undefined || !sourcePath || sourceKind === "custom") {
      return () => {
        cancelled = true;
      };
    }

    const reloadSource = (): void => {
      void readFileExplorerFile({ path: sourcePath })
        .then((reply) => {
          if (!cancelled) {
            setSourceError(null);
            if (reply.content !== lastSource) {
              lastSource = reply.content;
              setSource(reply.content);
            }
          }
        })
        .catch((error) => {
          if (!cancelled) {
            setSourceError(error instanceof Error ? error.message : String(error));
          }
        });
    };

    reloadSource();

    void host
      .on<CustomSidebarReloadEvent>(CUSTOM_SIDEBAR_RELOAD_EVENT, (event) => {
        if (customSidebarReloadMatches(event, sourcePath)) {
          reloadSource();
        }
      })
      .then((unlisten) => {
        if (!cancelled) {
          unlistenReload = unlisten;
        } else {
          unlisten();
        }
      })
      .catch((error) => {
        console.error("custom sidebar reload listener failed", error);
      });

    if (typeof window !== "undefined") {
      intervalId = window.setInterval(reloadSource, 1500);
    }

    return () => {
      cancelled = true;
      if (intervalId !== undefined) {
        window.clearInterval(intervalId);
      }
      unlistenReload?.();
    };
  }, [sourceKind, sourceOverride, sourcePath]);

  let jsonDocument: CustomSidebarJsonDocument | null = null;
  let swiftDocument: CustomSidebarSwiftDocument | null = null;
  let jsonError = sourceError;
  if (sourceKind === "json" && source !== null) {
    try {
      jsonDocument = parseCustomSidebarJson(source);
    } catch (error) {
      jsonError = error instanceof Error ? error.message : String(error);
    }
  }
  let swiftError = sourceError;
  if (sourceKind === "swift" && source !== null) {
    try {
      swiftDocument = parseCustomSidebarSwift(source, context, previews, swiftStateValues);
    } catch (error) {
      swiftError = error instanceof Error ? error.message : String(error);
    }
  }

  useEffect(() => {
    const latest = eventsContext.latest;
    const latestSeq = eventsContext.latest_seq;
    if (sourceKind !== "swift" || latest === null || latestSeq <= 0) return;
    if (handledSwiftEventSeq.current === undefined) {
      handledSwiftEventSeq.current = latestSeq;
      return;
    }
    if (latestSeq <= handledSwiftEventSeq.current) return;
    handledSwiftEventSeq.current = latestSeq;
    if (swiftDocument === null) return;

    const matchingHandlers = swiftDocument.eventHandlers.filter((handler) =>
      customSidebarSwiftEventHandlerMatches(handler, latest),
    );
    if (matchingHandlers.length === 0) return;

    const nextAssignments: CustomSidebarSwiftStateAssignment[] = [];
    for (const handler of matchingHandlers) {
      for (const assignment of handler.assignments) {
        nextAssignments.push(assignment);
      }
      if (handler.action !== undefined) {
        invokeCustomSidebarAction(handler.action, context, sourcePath, setActionError);
      }
    }
    if (nextAssignments.length > 0) {
      setSwiftStateValues((current) =>
        nextAssignments.reduce(
          (next, assignment) =>
            setSwiftSidebarStateValue(next, assignment.key, assignment.value),
          current,
        ),
      );
    }
  }, [context, eventsContext.latest, eventsContext.latest_seq, sourceKind, sourcePath, swiftDocument]);

  return (
    <section className="cmux-custom-sidebar-surface" aria-label="Custom sidebar preview">
      <header className="cmux-custom-sidebar-header">
        <div>
          <div className="cmux-custom-sidebar-kicker">
            {jsonDocument !== null
              ? "JSON custom sidebar"
              : swiftDocument !== null
                ? "Swift custom sidebar"
                : "Custom sidebar preview"}
          </div>
          <h2>
            {interpolateCustomSidebarTemplate(jsonDocument?.title, context) || sourceName}
          </h2>
          <p>{sourcePath?.trim() || "No sidebar source path is attached to this pane."}</p>
        </div>
        <div className="cmux-custom-sidebar-kind">{sourceKindLabel(sourceName)}</div>
      </header>

      {jsonDocument?.subtitle ? (
        <div className="cmux-custom-sidebar-json-subtitle">
          {interpolateCustomSidebarTemplate(jsonDocument.subtitle, context)}
        </div>
      ) : null}

      <div className="cmux-custom-sidebar-stats" aria-label="Custom sidebar session stats">
        <span>{workspaces.length} workspaces</span>
        <span>{unreadTotal} unread</span>
        <span>{portTotal} ports</span>
        <span>Live session data</span>
      </div>

      {actionError !== null ? (
        <div className="cmux-custom-sidebar-action-error" role="alert">
          {actionError}
        </div>
      ) : null}

      {sourceKind === "json" ? (
        <JsonSidebarBody
          document={jsonDocument}
          error={jsonError}
          loading={sourcePath !== undefined && source === null && sourceError === null}
          previews={previews}
          selected={selected}
          context={context}
          onWorkspaceAction={(workspace) => selectWorkspace(workspace.index)}
          onTabAction={(tab) => {
            if (selected !== undefined) {
              selectWorkspaceSurface(selected.id, tab.id);
            }
          }}
          onCustomAction={(action, extraContext) =>
            invokeCustomSidebarAction(
              action,
              { ...context, ...extraContext },
              sourcePath,
              setActionError,
            )
          }
        />
      ) : sourceKind === "swift" && (source !== null || swiftError !== null) ? (
        <SwiftSidebarBody
          document={swiftDocument}
          error={swiftError}
          loading={sourcePath !== undefined && source === null && sourceError === null}
          context={context}
          stateValues={swiftStateValues}
          onStateChange={(key, value) => {
            setSwiftStateValues((current) => setSwiftSidebarStateValue(current, key, value));
          }}
          onCustomAction={(action, extraContext) =>
            invokeCustomSidebarAction(
              action,
              { ...context, ...extraContext },
              sourcePath,
              setActionError,
            )
          }
        />
      ) : (
        <PreviewSidebarBody previews={previews} selected={selected} />
      )}

      <footer className="cmux-custom-sidebar-footer">
        {jsonDocument?.footer
          ? interpolateCustomSidebarTemplate(jsonDocument.footer, context)
          : sourceKind === "json"
            ? "Windows/Tauri JSON renderer with live source reload and safe-scoped authored actions."
            : sourceKind === "swift"
              ? "Windows/Tauri Swift subset renderer with safe-scoped authored actions. Complex SwiftUI remains a parity follow-up."
            : "Windows/Tauri preview renderer. Full authored SwiftUI/action hosting remains a parity follow-up."}
      </footer>
    </section>
  );
}

function PreviewSidebarBody({
  previews,
  selected,
}: {
  previews: WorkspacePreview[];
  selected: WorkspacePreview | undefined;
}): React.JSX.Element {
  return (
    <div className="cmux-custom-sidebar-body">
      <nav className="cmux-custom-sidebar-list" aria-label="Workspace preview list">
        {previews.length === 0 ? (
          <div className="cmux-custom-sidebar-empty">No workspaces in this session yet.</div>
        ) : (
          previews.map((workspace) => (
            <article
              key={workspace.id}
              className={
                workspace.selected
                  ? "cmux-custom-sidebar-card cmux-custom-sidebar-card-selected"
                  : "cmux-custom-sidebar-card"
              }
            >
              <div className="cmux-custom-sidebar-card-title">
                <span>{workspace.title}</span>
                {workspace.unreadCount > 0 ? <strong>{workspace.unreadCount}</strong> : null}
              </div>
              <div className="cmux-custom-sidebar-card-meta">
                <span>{workspace.tabCount} tabs</span>
                {workspace.branch ? (
                  <span>
                    {workspace.branch}
                    {workspace.dirty ? "*" : ""}
                  </span>
                ) : null}
                {workspace.ports.length > 0 ? (
                  <span>:{workspace.ports.join(" :")}</span>
                ) : null}
              </div>
            </article>
          ))
        )}
      </nav>

      <section className="cmux-custom-sidebar-detail" aria-label="Selected workspace details">
        {selected === undefined ? (
          <div className="cmux-custom-sidebar-empty">Select a workspace to see details.</div>
        ) : (
          <>
            <div className="cmux-custom-sidebar-detail-heading">
              <div>
                <span>Selected workspace</span>
                <h3>{selected.title}</h3>
              </div>
              {selected.progress !== undefined ? (
                <div className="cmux-custom-sidebar-progress">
                  <span>{selected.progress}%</span>
                  <div>
                    <i style={{ width: `${selected.progress}%` }} />
                  </div>
                </div>
              ) : null}
            </div>
            {detailRow("Directory", selected.directory ?? "Not reported")}
            {detailRow("Tabs", selected.tabCount)}
            {detailRow("Unread", selected.unreadCount)}
            {detailRow(
              "Ports",
              selected.ports.length > 0 ? selected.ports.join(", ") : "None",
            )}
            {detailRow(
              "Branch",
              selected.branch
                ? `${selected.branch}${selected.dirty ? " (dirty)" : ""}`
                : "Not reported",
            )}
            {detailRow("Remote", selected.remoteState ?? "Local")}
            {detailRow("Status entries", selected.statusCount)}
            {detailRow("Metadata blocks", selected.metadataCount)}
            {detailRow("Log entries", selected.logCount)}
          </>
        )}
      </section>
    </div>
  );
}

function SwiftSidebarBody({
  document,
  error,
  loading,
  context,
  stateValues,
  onStateChange,
  onCustomAction,
}: {
  document: CustomSidebarSwiftDocument | null;
  error: string | null;
  loading: boolean;
  context: JsonTemplateContext;
  stateValues: Record<string, SwiftSidebarStateValue>;
  onStateChange: (key: string, value: SwiftSidebarStateValue) => void;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
}): React.JSX.Element {
  if (loading) {
    return <div className="cmux-custom-sidebar-empty">Loading Swift sidebar...</div>;
  }
  if (error !== null) {
    return (
      <div className="cmux-custom-sidebar-error" role="alert">
        {error}
      </div>
    );
  }
  if (document === null) {
    return <div className="cmux-custom-sidebar-empty">No supported SwiftUI view found.</div>;
  }
  return (
    <SwiftSidebarStateContext.Provider
      value={{
        values: stateValues,
        setValue: onStateChange,
        runLocalHandler: (handler) => {
          for (const assignment of handler.assignments) {
            onStateChange(assignment.key, assignment.value);
          }
          if (handler.action !== undefined) {
            onCustomAction(handler.action);
          }
        },
      }}
    >
      <div className="cmux-custom-sidebar-swift-body">
        <SwiftSidebarNodeView
          node={document.root}
          context={context}
          onCustomAction={onCustomAction}
        />
        {document.warnings.length > 0 ? (
          <div className="cmux-custom-sidebar-swift-warnings">
            {document.warnings.slice(0, 3).join(" · ")}
          </div>
        ) : null}
      </div>
    </SwiftSidebarStateContext.Provider>
  );
}

function swiftPickerOptionLabels(nodes: CustomSidebarSwiftNode[]): string[] {
  const labels: string[] = [];
  for (const node of nodes) {
    if (node.kind === "text" && node.text.trim() !== "") {
      labels.push(node.text);
    } else if (node.kind === "label" && node.text.trim() !== "") {
      labels.push(node.text);
    } else if (node.kind === "modified") {
      labels.push(...swiftPickerOptionLabels([node.base]));
    } else if (
      (node.kind === "group" ||
        node.kind === "vstack" ||
        node.kind === "hstack" ||
        node.kind === "gridRow") &&
      node.children.length > 0
    ) {
      labels.push(...swiftPickerOptionLabels(node.children));
    }
  }
  return [...new Set(labels)];
}

function swiftTabItemLabel(node: CustomSidebarSwiftNode): string | undefined {
  if (node.kind !== "modified") return undefined;
  const tabItem = node.childModifiers.find((modifier) => modifier.name === "tabItem");
  const labels = swiftPickerOptionLabels(tabItem?.children ?? []);
  if (labels.length > 0) return labels.join(" ");
  return swiftTabItemLabel(node.base);
}

function swiftDatePickerInputType(displayedComponents: string | undefined): "date" | "time" | "datetime-local" {
  const normalized = displayedComponents?.replace(/^\./, "");
  if (normalized === "date") return "date";
  if (normalized === "hourAndMinute") return "time";
  return "datetime-local";
}

function swiftDatePickerInputValue(value: string, inputType: "date" | "time" | "datetime-local"): string {
  if (inputType === "date") return value.match(/^\d{4}-\d{2}-\d{2}/)?.[0] ?? "";
  if (inputType === "time") return value.match(/\d{2}:\d{2}/)?.[0] ?? "";
  return value.match(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/)?.[0] ?? "";
}

function swiftColorPickerInputValue(value: string): string {
  const trimmed = value.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) return trimmed;
  if (/^#[0-9a-fA-F]{3}$/.test(trimmed)) {
    return `#${trimmed
      .slice(1)
      .split("")
      .map((char) => `${char}${char}`)
      .join("")}`;
  }
  return swiftColor(trimmed);
}

function swiftGridItemSummary(items: SwiftGridItemValue[] | undefined): string | undefined {
  if (items === undefined || items.length === 0) return undefined;
  return items
    .map((item) => {
      const bounds = [
        item.minimum !== undefined ? `min:${item.minimum}` : undefined,
        item.maximum !== undefined ? `max:${item.maximum}` : undefined,
        item.spacing !== undefined ? `spacing:${item.spacing}` : undefined,
        item.alignment !== undefined ? `alignment:${item.alignment}` : undefined,
      ].filter(Boolean);
      return bounds.length === 0 ? item.size : `${item.size}(${bounds.join(",")})`;
    })
    .join("|");
}

function swiftGridStyle(
  node: Extract<CustomSidebarSwiftNode, { kind: "grid" }>,
  presentationStyle: CSSProperties,
): CSSProperties {
  const style: CSSProperties = {
    ...presentationStyle,
    ...(node.spacing !== undefined ? { gap: `${node.spacing}px` } : {}),
  };
  if (node.gridKind === "grid") return style;
  const template = swiftGridTemplate(node.gridItems);
  return {
    ...style,
    display: "grid",
    ...(node.gridKind === "lazyVGrid"
      ? { gridTemplateColumns: template }
      : { gridTemplateRows: template, gridAutoFlow: "column" }),
  };
}

function swiftGridTemplate(items: SwiftGridItemValue[] | undefined): string {
  if (items === undefined || items.length === 0) {
    return "repeat(auto-fit, minmax(54px, 1fr))";
  }
  return items.map(swiftGridItemTemplate).join(" ");
}

function swiftGridItemTemplate(item: SwiftGridItemValue): string {
  if (item.size === "fixed") {
    return `${Math.max(0, item.minimum ?? 0)}px`;
  }
  const minimum = Math.max(0, item.minimum ?? 0);
  const maximum = item.maximum !== undefined ? `${Math.max(minimum, item.maximum)}px` : "1fr";
  if (item.size === "adaptive") {
    return `repeat(auto-fit, minmax(${minimum}px, ${maximum}))`;
  }
  return `minmax(${minimum}px, ${maximum})`;
}

function SwiftNavigationStackView({
  node,
  context,
  onCustomAction,
  className,
  style,
  title,
  accessibilityProps,
}: {
  node: Extract<CustomSidebarSwiftNode, { kind: "navigationStack" }>;
  context: JsonTemplateContext;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
  className: string;
  style: CSSProperties;
  title?: string;
  accessibilityProps: ReturnType<typeof swiftAccessibilityProps>;
}): React.JSX.Element {
  const [path, setPath] = useState<SwiftNavigationEntry[]>([]);
  const active = path.at(-1);
  const visibleNodes = active?.destination ?? node.children;
  const navigationContext: SwiftNavigationContextValue = {
    push: (entry) => setPath((current) => [...current, entry]),
  };
  return (
    <SwiftNavigationContext.Provider value={navigationContext}>
      <section className={className} style={style} title={title} {...accessibilityProps}>
        {active !== undefined ? (
          <header className="cmux-custom-sidebar-swift-navigation-stack-header">
            <button
              type="button"
              className="cmux-custom-sidebar-swift-navigation-back"
              onClick={() => setPath((current) => current.slice(0, -1))}
            >
              Back
            </button>
            <div className="cmux-custom-sidebar-swift-navigation-stack-title">
              {active.title}
            </div>
          </header>
        ) : null}
        <div className="cmux-custom-sidebar-swift-navigation-stack-content">
          {visibleNodes.map((child, index) => (
            <SwiftSidebarNodeView
              key={`${path.length}-${index}`}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      </section>
    </SwiftNavigationContext.Provider>
  );
}

function swiftLocalModifierHandlers(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
  name: CustomSidebarSwiftLocalHandlerModifierName,
): CustomSidebarSwiftLocalHandler[] {
  return (modifiers ?? [])
    .filter((modifier) => modifier.name === name && modifier.localHandler !== undefined)
    .map((modifier) => modifier.localHandler)
    .filter((handler): handler is CustomSidebarSwiftLocalHandler => handler !== undefined);
}

function swiftHasLocalModifierHandler(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
  name: CustomSidebarSwiftLocalHandlerModifierName,
): boolean {
  return swiftLocalModifierHandlers(modifiers, name).length > 0;
}

function runSwiftLocalModifierHandlers(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
  name: CustomSidebarSwiftLocalHandlerModifierName,
  swiftState: SwiftSidebarStateContextValue,
): void {
  for (const handler of swiftLocalModifierHandlers(modifiers, name)) {
    swiftState.runLocalHandler(handler);
  }
}

function runSwiftHoverModifierHandlers(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
  isHovering: boolean,
  swiftState: SwiftSidebarStateContextValue,
): void {
  for (const modifier of modifiers ?? []) {
    if (modifier.name !== "onHover") continue;
    const handler = isHovering ? modifier.localHandler : modifier.falseLocalHandler;
    if (handler !== undefined) swiftState.runLocalHandler(handler);
  }
}

function swiftLifecycleModifierSignature(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
): string {
  const handlers = (modifiers ?? [])
    .filter(
      (modifier) =>
        (modifier.name === "onAppear" || modifier.name === "onDisappear") &&
        modifier.localHandler !== undefined,
    )
    .map((modifier) => ({
      name: modifier.name,
      handler: modifier.localHandler,
    }));
  return handlers.length === 0 ? "" : JSON.stringify(handlers);
}

function swiftTaskModifierSignature(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
): string {
  const handlers = (modifiers ?? [])
    .filter((modifier) => modifier.name === "task" && modifier.localHandler !== undefined)
    .map((modifier) => ({
      name: modifier.name,
      value: modifier.value,
      idExpression: modifier.secondaryValue,
      handler: modifier.localHandler,
    }));
  return handlers.length === 0 ? "" : JSON.stringify(handlers);
}

function isSwiftAlertPresentation(modifier: CustomSidebarSwiftModifier): boolean {
  return modifier.name === "alert" || modifier.name === "confirmationDialog";
}

function swiftPresentationContentNodes(
  modifier: CustomSidebarSwiftModifier,
): CustomSidebarSwiftNode[] {
  const children = modifier.children ?? [];
  return isSwiftAlertPresentation(modifier)
    ? children.filter((child) => child.kind !== "button")
    : children;
}

function swiftPresentationActionNodes(
  modifier: CustomSidebarSwiftModifier,
): CustomSidebarSwiftNode[] {
  return isSwiftAlertPresentation(modifier)
    ? (modifier.children ?? []).filter((child) => child.kind === "button")
    : [];
}

function swiftPresentationChrome(modifier: CustomSidebarSwiftModifier): {
  className: string;
  detents?: string;
  dragIndicator?: string;
  background?: string;
  cornerRadius?: string;
  style: CSSProperties;
} {
  const nodes = swiftPresentationContentNodes(modifier);
  const detents = swiftFindPresentationChrome(nodes, "presentationDetents");
  const dragIndicator = swiftFindPresentationChrome(nodes, "presentationDragIndicator");
  const background = swiftFindPresentationChrome(nodes, "presentationBackground");
  const cornerRadius = swiftFindPresentationChrome(nodes, "presentationCornerRadius");
  const classes: string[] = [];
  const style: CSSProperties = {};
  if (detents !== undefined) {
    classes.push(
      `cmux-custom-sidebar-swift-presentation-detents-${swiftToken(detents, "custom")}`,
    );
  }
  if (dragIndicator !== undefined) {
    classes.push(
      `cmux-custom-sidebar-swift-presentation-drag-indicator-${swiftToken(
        dragIndicator,
        "automatic",
      )}`,
    );
  }
  if (background !== undefined) {
    classes.push(
      `cmux-custom-sidebar-swift-presentation-background-${swiftToken(
        background,
        "custom",
      )}`,
    );
    style.background = swiftBackgroundStyle(background);
  }
  if (cornerRadius !== undefined) {
    const radius = Number(cornerRadius);
    if (Number.isFinite(radius)) {
      classes.push("cmux-custom-sidebar-swift-presentation-corner-radius");
      style.borderRadius = `${Math.max(0, radius)}px`;
    }
  }
  return { className: classes.join(" "), detents, dragIndicator, background, cornerRadius, style };
}

function applySwiftPaddingLikeStyle(
  style: CSSProperties,
  modifier: CustomSidebarSwiftModifier,
): {
  edges?: string;
  length?: string;
  insets?: string;
} {
  const insetEntries = [
    ["top", modifier.paddingTop],
    ["leading", modifier.paddingLeading],
    ["bottom", modifier.paddingBottom],
    ["trailing", modifier.paddingTrailing],
  ] as const;
  const insets = insetEntries
    .filter(([, value]) => value !== undefined)
    .map(([edge, value]) => `${edge}:${Math.max(0, value ?? 0)}`);
  if (insets.length > 0) {
    if (modifier.paddingTop !== undefined) {
      style.paddingTop = `${Math.max(0, modifier.paddingTop)}px`;
    }
    if (modifier.paddingLeading !== undefined) {
      style.paddingLeft = `${Math.max(0, modifier.paddingLeading)}px`;
    }
    if (modifier.paddingBottom !== undefined) {
      style.paddingBottom = `${Math.max(0, modifier.paddingBottom)}px`;
    }
    if (modifier.paddingTrailing !== undefined) {
      style.paddingRight = `${Math.max(0, modifier.paddingTrailing)}px`;
    }
    return { insets: insets.join(",") };
  }

  const amount = modifier.value === undefined ? 8 : Number(modifier.value);
  const cssAmount = `${Number.isFinite(amount) ? amount : 8}px`;
  const edges = modifier.edge?.split(",").filter(Boolean) ?? [];
  if (edges.length === 0 || edges.length >= 4) {
    style.padding = cssAmount;
    return {
      edges: "all",
      ...(modifier.value !== undefined ? { length: modifier.value } : {}),
    };
  }
  if (edges.includes("top")) style.paddingTop = cssAmount;
  if (edges.includes("bottom")) style.paddingBottom = cssAmount;
  if (edges.includes("leading")) style.paddingLeft = cssAmount;
  if (edges.includes("trailing")) style.paddingRight = cssAmount;
  return {
    edges: edges.join(","),
    ...(modifier.value !== undefined ? { length: modifier.value } : {}),
  };
}

function swiftFindPresentationChrome(
  nodes: CustomSidebarSwiftNode[],
  name:
    | "presentationDetents"
    | "presentationDragIndicator"
    | "presentationBackground"
    | "presentationCornerRadius",
): string | undefined {
  for (const node of nodes) {
    const direct = node.modifiers?.find((modifier) => modifier.name === name)?.value;
    if (direct !== undefined) return direct;
    if (node.kind === "modified") {
      const child = node.childModifiers.find((modifier) => modifier.name === name)?.value;
      if (child !== undefined) return child;
      const base = swiftFindPresentationChrome([node.base], name);
      if (base !== undefined) return base;
    }
    const nested = swiftFindPresentationChrome(swiftNodeChildren(node), name);
    if (nested !== undefined) return nested;
  }
  return undefined;
}

function swiftNodeChildren(node: CustomSidebarSwiftNode): CustomSidebarSwiftNode[] {
  switch (node.kind) {
    case "vstack":
    case "hstack":
    case "zstack":
    case "group":
    case "splitView":
    case "navigationStack":
    case "tabView":
    case "list":
    case "section":
    case "labeledContent":
    case "grid":
    case "gridRow":
    case "menu":
    case "textField":
    case "stepper":
    case "picker":
    case "datePicker":
    case "colorPicker":
    case "toggle":
    case "scrollView":
    case "button":
      return node.children;
    case "navigationLink":
      return [...node.children, ...node.destination];
    case "externalLink":
      return node.labelChildren;
    case "contentUnavailable":
      return [...node.labelChildren, ...node.descriptionChildren, ...node.actionsChildren];
    case "groupBox":
    case "disclosureGroup":
      return [...node.labelChildren, ...node.children];
    case "modified":
      return [node.base, ...node.childModifiers.flatMap((modifier) => modifier.children ?? [])];
    default:
      return [];
  }
}

function SwiftSidebarNodeView({
  node,
  context,
  onCustomAction,
}: {
  node: CustomSidebarSwiftNode;
  context: JsonTemplateContext;
  onCustomAction: (
    action: CustomSidebarJsonAction,
    extraContext?: Pick<TemplateContext, "workspace" | "tab">,
  ) => void;
}): React.JSX.Element | null {
  const swiftState = useContext(SwiftSidebarStateContext);
  const navigation = useContext(SwiftNavigationContext);
  const presentationContext = useContext(SwiftPresentationContext);
  const presentation = swiftModifierPresentation(node.modifiers);
  const accessibilityProps = swiftAccessibilityProps(presentation);
  const className = (base: string): string =>
    presentation.className ? `${base} ${presentation.className}` : base;
  const hasOnChange = swiftHasLocalModifierHandler(node.modifiers, "onChange");
  const hasOnSubmit = swiftHasLocalModifierHandler(node.modifiers, "onSubmit");
  const lifecycleSignature = swiftLifecycleModifierSignature(node.modifiers);
  const taskSignature = swiftTaskModifierSignature(node.modifiers);
  const hasOnHover = swiftHasLocalModifierHandler(node.modifiers, "onHover");
  const dropDestinationModifier = node.modifiers?.find(
    (modifier) => modifier.name === "dropDestination" && modifier.action !== undefined,
  );
  const accessibilityActionModifier = node.modifiers?.find(
    (modifier) => modifier.name === "accessibilityAction" && modifier.action !== undefined,
  );
  const onMouseEnter: MouseEventHandler | undefined = hasOnHover
    ? () => runSwiftHoverModifierHandlers(node.modifiers, true, swiftState)
    : undefined;
  const onMouseLeave: MouseEventHandler | undefined = hasOnHover
    ? () => runSwiftHoverModifierHandlers(node.modifiers, false, swiftState)
    : undefined;
  const onFocus: FocusEventHandler | undefined =
    presentation.focusedStateBindingKey === undefined
      ? undefined
      : () => swiftState.setValue(presentation.focusedStateBindingKey!, true);
  const onBlur: FocusEventHandler | undefined =
    presentation.focusedStateBindingKey === undefined
      ? undefined
      : () => swiftState.setValue(presentation.focusedStateBindingKey!, false);
  const onDragOver: DragEventHandler | undefined =
    dropDestinationModifier === undefined
      ? undefined
      : (event) => {
          event.preventDefault();
        };
  const onDrop: DragEventHandler | undefined =
    dropDestinationModifier?.action === undefined
      ? undefined
      : (event) => {
          event.preventDefault();
          if (!presentation.disabled) {
            onCustomAction(dropDestinationModifier.action!);
          }
        };
  const onKeyDown: KeyboardEventHandler | undefined =
    accessibilityActionModifier?.action === undefined
      ? undefined
      : (event) => {
          if (event.key !== "Enter" && event.key !== " ") return;
          event.preventDefault();
          if (!presentation.disabled) {
            onCustomAction(accessibilityActionModifier.action!);
          }
        };
  const nodeProps = {
    ...accessibilityProps,
    ...((presentation.isFocusable || accessibilityActionModifier !== undefined) &&
    !presentation.disabled
      ? { tabIndex: 0 }
      : {}),
    ...(onMouseEnter !== undefined ? { onMouseEnter } : {}),
    ...(onMouseLeave !== undefined ? { onMouseLeave } : {}),
    ...(onFocus !== undefined ? { onFocus } : {}),
    ...(onBlur !== undefined ? { onBlur } : {}),
    ...(onDragOver !== undefined ? { onDragOver } : {}),
    ...(onDrop !== undefined ? { onDrop } : {}),
    ...(onKeyDown !== undefined ? { onKeyDown } : {}),
  };
  const setLocalStateValue = (key: string, value: SwiftSidebarStateValue): void => {
    swiftState.setValue(key, value);
    runSwiftLocalModifierHandlers(node.modifiers, "onChange", swiftState);
  };
  const submitLocalStateValue = (): void => {
    runSwiftLocalModifierHandlers(node.modifiers, "onSubmit", swiftState);
  };
  useEffect(() => {
    if (lifecycleSignature === "") return;
    runSwiftLocalModifierHandlers(node.modifiers, "onAppear", swiftState);
    return () => {
      runSwiftLocalModifierHandlers(node.modifiers, "onDisappear", swiftState);
    };
  }, [lifecycleSignature]);
  useEffect(() => {
    if (taskSignature === "") return;
    runSwiftLocalModifierHandlers(node.modifiers, "task", swiftState);
  }, [taskSignature]);
  const stackStyle = (
    spacing: number | undefined,
    alignment: string | undefined,
    axis: "vertical" | "horizontal" | "zstack",
  ): CSSProperties => ({
    ...swiftStackAlignmentStyle(axis, alignment),
    ...presentation.style,
    ...(spacing !== undefined ? { gap: `${spacing}px` } : {}),
  });
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clearLongPressTimer = (): void => {
    if (longPressTimer.current !== null) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  };
  switch (node.kind) {
    case "navigationStack":
      return (
        <SwiftNavigationStackView
          node={node}
          context={context}
          onCustomAction={onCustomAction}
          className={className("cmux-custom-sidebar-swift-navigation-stack")}
          style={presentation.style}
          title={presentation.title}
          accessibilityProps={nodeProps}
        />
      );
    case "tabView": {
      const tabViewStyle = presentation.tabViewStyle ?? "automatic";
      const isPageStyle = tabViewStyle === "page";
      const tabItemLabels = node.children.map((child) => swiftTabItemLabel(child));
      const hasTabItems = tabItemLabels.some((label) => label !== undefined);
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-tab-view cmux-custom-sidebar-swift-tab-view-${swiftToken(
              tabViewStyle,
              "automatic",
            )}`,
          )}
          style={{
            ...presentation.style,
            display: "flex",
            flexDirection: "column",
            gap: "8px",
          }}
          title={presentation.title}
          data-swift-tab-view="true"
          {...nodeProps}
        >
          {hasTabItems ? (
            <div
              className="cmux-custom-sidebar-swift-tab-view-tabs"
              data-swift-tab-items="true"
              style={{
                display: "flex",
                gap: "6px",
                overflowX: "auto",
              }}
            >
              {tabItemLabels.map((label, index) =>
                label === undefined ? null : (
                  <span
                    key={index}
                    className="cmux-custom-sidebar-swift-tab-view-tab"
                    data-swift-tab-item-index={String(index)}
                    data-swift-tab-item={label}
                    style={{
                      border: "1px solid rgba(148, 163, 184, 0.26)",
                      borderRadius: "999px",
                      color: "#cfe7e1",
                      flex: "0 0 auto",
                      fontSize: "11px",
                      padding: "3px 8px",
                    }}
                  >
                    {label}
                  </span>
                ),
              )}
            </div>
          ) : null}
          <div
            className="cmux-custom-sidebar-swift-tab-view-pages"
            style={{
              display: "flex",
              flexDirection: isPageStyle ? "row" : "column",
              gap: isPageStyle ? "10px" : "8px",
              overflowX: isPageStyle ? "auto" : undefined,
              scrollSnapType: isPageStyle ? "x mandatory" : undefined,
            }}
          >
            {node.children.map((child, index) => {
              const tabItemLabel = tabItemLabels[index];
              return (
                <div
                  key={index}
                  className="cmux-custom-sidebar-swift-tab-view-page"
                  data-swift-tab-view-page={String(index)}
                  {...(tabItemLabel !== undefined ? { "data-swift-tab-item": tabItemLabel } : {})}
                  style={{
                    flex: isPageStyle ? "0 0 100%" : undefined,
                    minWidth: isPageStyle ? "100%" : undefined,
                    scrollSnapAlign: isPageStyle ? "start" : undefined,
                  }}
                >
                  <SwiftSidebarNodeView
                    node={child}
                    context={context}
                    onCustomAction={onCustomAction}
                  />
                </div>
              );
            })}
          </div>
        </div>
      );
    }
    case "modified": {
      const backgroundModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "background",
      );
      const overlayModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "overlay" || modifier.name === "mask",
      );
      const topInsets = node.childModifiers.filter(
        (modifier) =>
          modifier.name === "safeAreaInset" && (modifier.edge ?? "top") !== "bottom",
      );
      const bottomInsets = node.childModifiers.filter(
        (modifier) => modifier.name === "safeAreaInset" && modifier.edge === "bottom",
      );
      const contextMenus = node.childModifiers.filter(
        (modifier) => modifier.name === "contextMenu",
      );
      const refreshableModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "refreshable",
      );
      const swipeActionModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "swipeActions",
      );
      const presentationModifiers = node.childModifiers.filter(
        (modifier) =>
          (modifier.name === "sheet" ||
            modifier.name === "popover" ||
            modifier.name === "fullScreenCover" ||
            modifier.name === "alert" ||
            modifier.name === "confirmationDialog") &&
          modifier.boolValue === true,
      );
      const toolbarModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "toolbar",
      );
      const searchableModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "searchable",
      );
      const accessibilityRepresentationModifiers = node.childModifiers.filter(
        (modifier) => modifier.name === "accessibilityRepresentation",
      );
      const accessibilityRepresentationLabels = swiftPickerOptionLabels(
        accessibilityRepresentationModifiers.flatMap((modifier) => modifier.children ?? []),
      );
      const dismissPresentationModifier = (modifier: CustomSidebarSwiftModifier): void => {
        if (modifier.stateBindingKey === undefined) return;
        swiftState.setValue(
          modifier.stateBindingKey,
          modifier.presentationBindingKind === "item" ? null : false,
        );
      };
      const basePresentation = swiftModifierPresentation(node.base.modifiers);
      const toolbarChromeClassName = swiftToolbarChromeClassName(basePresentation);
      const toolbarChromeStyle = swiftToolbarChromeStyle(basePresentation);
      const toolbarChromeProps = swiftToolbarChromeProps(basePresentation);
      const renderModifierChildren = (
        modifier: CustomSidebarSwiftModifier,
        extraClassName: string,
      ): ReactNode => (
        <div className={extraClassName}>
          {(modifier.children ?? []).map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-modified${
              accessibilityRepresentationModifiers.length > 0
                ? " cmux-custom-sidebar-swift-accessibility-representation"
                : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          data-swift-accessibility-representation={
            accessibilityRepresentationModifiers.length > 0 ? "true" : undefined
          }
          data-swift-accessibility-representation-count={
            accessibilityRepresentationModifiers.length > 0
              ? String(accessibilityRepresentationModifiers.length)
              : undefined
          }
          data-swift-accessibility-representation-label={
            accessibilityRepresentationLabels.length > 0
              ? accessibilityRepresentationLabels.join(" ")
              : undefined
          }
          {...nodeProps}
        >
          {basePresentation.navigationTitle !== undefined ||
          basePresentation.navigationSubtitle !== undefined ? (
            <header
              className={`cmux-custom-sidebar-swift-navigation cmux-custom-sidebar-swift-navigation-${swiftToken(
                basePresentation.navigationDisplayMode,
                "automatic",
              )}${toolbarChromeClassName}`}
              style={toolbarChromeStyle}
              {...toolbarChromeProps}
            >
              {basePresentation.navigationTitle !== undefined ? (
                <div className="cmux-custom-sidebar-swift-navigation-title">
                  {basePresentation.navigationTitle}
                </div>
              ) : null}
              {basePresentation.navigationSubtitle !== undefined ? (
                <div className="cmux-custom-sidebar-swift-navigation-subtitle">
                  {basePresentation.navigationSubtitle}
                </div>
              ) : null}
            </header>
          ) : null}
          {toolbarModifiers.map((modifier, index) => (
            <div
              key={`toolbar-${index}`}
              className={`cmux-custom-sidebar-swift-toolbar${toolbarChromeClassName}`}
              style={toolbarChromeStyle}
              {...toolbarChromeProps}
            >
              {renderModifierChildren(modifier, "cmux-custom-sidebar-swift-toolbar-content")}
            </div>
          ))}
          {searchableModifiers.map((modifier, index) => {
            const stateBindingKey = modifier.stateBindingKey;
            const editable = stateBindingKey !== undefined && !presentation.disabled;
            return (
              <label
                key={`searchable-${index}`}
                className="cmux-custom-sidebar-swift-searchable"
                data-swift-searchable="true"
                data-swift-search-placement={modifier.placement}
                data-swift-state-binding={stateBindingKey}
              >
                <span className="cmux-custom-sidebar-swift-searchable-icon" aria-hidden="true">
                  search
                </span>
                <input
                  type="search"
                  value={modifier.value ?? ""}
                  placeholder={modifier.secondaryValue ?? "Search"}
                  readOnly={!editable}
                  disabled={presentation.disabled}
                  aria-readonly={editable ? undefined : "true"}
                  data-swift-search-prompt={modifier.secondaryValue}
                  onChange={
                    stateBindingKey !== undefined && editable
                      ? (event) => setLocalStateValue(stateBindingKey, event.currentTarget.value)
                      : undefined
                  }
                />
              </label>
            );
          })}
          {topInsets.map((modifier, index) => (
            <div key={`top-${index}`} className="cmux-custom-sidebar-swift-safe-area-inset">
              {renderModifierChildren(modifier, "cmux-custom-sidebar-swift-safe-area-content")}
            </div>
          ))}
          <div className="cmux-custom-sidebar-swift-layered">
            {backgroundModifiers.map((modifier, index) => (
              <div
                key={`background-${index}`}
                className="cmux-custom-sidebar-swift-modifier-background"
                aria-hidden="true"
              >
                {renderModifierChildren(modifier, "cmux-custom-sidebar-swift-modifier-content")}
              </div>
            ))}
            <div className="cmux-custom-sidebar-swift-modifier-base">
              <SwiftSidebarNodeView
                node={node.base}
                context={context}
                onCustomAction={onCustomAction}
              />
            </div>
            {overlayModifiers.map((modifier, index) => (
              <div
                key={`overlay-${index}`}
                className={`cmux-custom-sidebar-swift-modifier-overlay cmux-custom-sidebar-swift-modifier-overlay-${swiftToken(
                  modifier.value,
                  "center",
                )}${modifier.name === "mask" ? " cmux-custom-sidebar-swift-modifier-mask" : ""}`}
                aria-hidden={modifier.name === "mask" ? "true" : undefined}
              >
                {renderModifierChildren(modifier, "cmux-custom-sidebar-swift-modifier-content")}
              </div>
            ))}
          </div>
          {bottomInsets.map((modifier, index) => (
            <div key={`bottom-${index}`} className="cmux-custom-sidebar-swift-safe-area-inset">
              {renderModifierChildren(modifier, "cmux-custom-sidebar-swift-safe-area-content")}
            </div>
          ))}
          {contextMenus.map((modifier, index) => (
            <details key={`context-${index}`} className="cmux-custom-sidebar-swift-context-menu">
              <summary>Context</summary>
              <div className="cmux-custom-sidebar-swift-context-menu-body">
                {(modifier.children ?? []).map((child, childIndex) => (
                  <SwiftSidebarNodeView
                    key={childIndex}
                    node={child}
                    context={context}
                    onCustomAction={onCustomAction}
                  />
                ))}
              </div>
            </details>
          ))}
          {refreshableModifiers.map((modifier, index) => (
            <div
              key={`refresh-${index}`}
              className="cmux-custom-sidebar-swift-refreshable"
              data-swift-refreshable="true"
            >
              <button
                type="button"
                className="cmux-custom-sidebar-swift-refresh-button"
                disabled={modifier.action === undefined || presentation.disabled}
                onClick={() => {
                  if (modifier.action !== undefined && !presentation.disabled) {
                    onCustomAction(modifier.action);
                  }
                }}
              >
                Refresh
              </button>
            </div>
          ))}
          {swipeActionModifiers.map((modifier, index) => (
            <div
              key={`swipe-${index}`}
              className={`cmux-custom-sidebar-swift-swipe-actions cmux-custom-sidebar-swift-swipe-actions-${swiftToken(
                modifier.value,
                "trailing",
              )}`}
              data-swift-swipe-actions={modifier.value ?? "trailing"}
              data-swift-swipe-allows-full-swipe={modifier.boolValue}
            >
              {(modifier.children ?? []).map((child, childIndex) => (
                <SwiftSidebarNodeView
                  key={childIndex}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </div>
          ))}
          {presentationModifiers.map((modifier, index) => {
            const chrome = swiftPresentationChrome(modifier);
            return (
              <section
                key={`presentation-${index}`}
                className={`cmux-custom-sidebar-swift-presentation cmux-custom-sidebar-swift-presentation-${swiftToken(
                  modifier.name,
                  "sheet",
                )}${chrome.className ? ` ${chrome.className}` : ""}`}
                role={
                  modifier.name === "alert" || modifier.name === "confirmationDialog"
                    ? "alertdialog"
                    : "dialog"
                }
                aria-label={modifier.value ?? modifier.name}
                data-swift-presentation-binding={modifier.presentationBindingKind}
                data-swift-presentation-item={
                  modifier.itemValue === null ? undefined : String(modifier.itemValue)
                }
                data-swift-presentation-detents={chrome.detents}
                data-swift-presentation-drag-indicator={chrome.dragIndicator}
                data-swift-presentation-background={chrome.background}
                data-swift-presentation-corner-radius={chrome.cornerRadius}
                style={chrome.style}
              >
                <header className="cmux-custom-sidebar-swift-presentation-header">
                  <div className="cmux-custom-sidebar-swift-presentation-title">
                    {modifier.value ?? swiftPresentationTitle(modifier.name)}
                  </div>
                  {modifier.stateBindingKey !== undefined ? (
                    <button
                      type="button"
                      className="cmux-custom-sidebar-swift-presentation-close"
                      onClick={() => dismissPresentationModifier(modifier)}
                    >
                      Close
                    </button>
                  ) : null}
                </header>
                <div className="cmux-custom-sidebar-swift-presentation-body">
                  <SwiftPresentationContext.Provider
                    value={{ dismiss: () => dismissPresentationModifier(modifier) }}
                  >
                    {swiftPresentationContentNodes(modifier).map((child, childIndex) => (
                      <SwiftSidebarNodeView
                        key={childIndex}
                        node={child}
                        context={context}
                        onCustomAction={onCustomAction}
                      />
                    ))}
                    {swiftPresentationActionNodes(modifier).length > 0 ? (
                      <div className="cmux-custom-sidebar-swift-presentation-actions">
                        {swiftPresentationActionNodes(modifier).map((child, childIndex) => (
                          <SwiftSidebarNodeView
                            key={`action-${childIndex}`}
                            node={child}
                            context={context}
                            onCustomAction={onCustomAction}
                          />
                        ))}
                      </div>
                    ) : null}
                  </SwiftPresentationContext.Provider>
                </div>
              </section>
            );
          })}
          {accessibilityRepresentationModifiers.map((modifier, index) => (
            <div
              key={`accessibility-representation-${index}`}
              className="cmux-custom-sidebar-swift-accessibility-representation-content"
              data-swift-accessibility-representation-content="true"
              hidden
              aria-hidden="true"
            >
              {renderModifierChildren(
                modifier,
                "cmux-custom-sidebar-swift-accessibility-representation-body",
              )}
            </div>
          ))}
        </div>
      );
    }
    case "vstack":
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-stack cmux-custom-sidebar-swift-stack-vertical${swiftLazyStackClass(node.lazyStack)}${swiftPinnedViewsClass(node.pinnedViews)}${swiftStackAlignmentClass(node.alignment)}`,
          )}
          style={stackStyle(node.spacing, node.alignment, "vertical")}
          title={presentation.title}
          {...(node.pinnedViews !== undefined ? { "data-swift-pinned-views": node.pinnedViews } : {})}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    case "group": {
      const isControlGroup = node.groupRole === "controlGroup";
      const isViewThatFits = node.groupRole === "viewThatFits";
      const isSemanticGroup = node.groupRole === "group";
      const isToolbarItem = node.groupRole === "toolbarItem";
      const groupAxis = isControlGroup
        ? "horizontal"
        : isToolbarItem
          ? "horizontal"
        : isViewThatFits && node.fitAxis === "horizontal"
          ? "horizontal"
          : "vertical";
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-stack ${
              isControlGroup
                ? "cmux-custom-sidebar-swift-control-group"
                : isToolbarItem
                  ? `cmux-custom-sidebar-swift-toolbar-item cmux-custom-sidebar-swift-toolbar-item-${swiftToken(
                      node.toolbarPlacement,
                      "automatic",
                    )}`
                : isViewThatFits
                  ? `cmux-custom-sidebar-swift-view-that-fits cmux-custom-sidebar-swift-view-that-fits-${swiftToken(
                      node.fitAxis,
                      "vertical",
                    )}`
                  : isSemanticGroup
                    ? "cmux-custom-sidebar-swift-group"
                    : "cmux-custom-sidebar-swift-stack-vertical"
            }${swiftStackAlignmentClass(node.alignment)}`,
          )}
          style={
            isControlGroup
              ? stackStyle(node.spacing ?? 6, node.alignment, "horizontal")
              : isToolbarItem
                ? stackStyle(node.spacing ?? 6, node.alignment, "horizontal")
              : isSemanticGroup
                ? { ...presentation.style, display: "contents" }
              : stackStyle(node.spacing, node.alignment, groupAxis)
          }
          title={presentation.title}
          {...(isSemanticGroup ? { "data-swift-group": "true" } : {})}
          {...(isControlGroup ? { "data-swift-control-group": "true" } : {})}
          {...(isToolbarItem
            ? { "data-swift-toolbar-placement": node.toolbarPlacement ?? "automatic" }
            : {})}
          {...(isViewThatFits ? { "data-swift-view-that-fits": node.fitAxis ?? "vertical" } : {})}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    }
    case "groupBox": {
      const labelText =
        node.title ??
        node.labelChildren
          .map((child) =>
            child.kind === "text" || child.kind === "label" ? child.text : undefined,
          )
          .find((text) => text !== undefined && text.trim() !== "");
      const hasLabel = node.title !== undefined || node.labelChildren.length > 0;
      return (
        <section
          className={className("cmux-custom-sidebar-swift-group-box")}
          style={presentation.style}
          title={presentation.title}
          data-swift-group-box={labelText ?? "true"}
          {...(labelText !== undefined ? { "data-swift-group-box-label": labelText } : {})}
          {...nodeProps}
        >
          {hasLabel ? (
            <div className="cmux-custom-sidebar-swift-group-box-label">
              {node.labelChildren.length > 0 ? (
                node.labelChildren.map((child, index) => (
                  <SwiftSidebarNodeView
                    key={index}
                    node={child}
                    context={context}
                    onCustomAction={onCustomAction}
                  />
                ))
              ) : (
                <span>{node.title}</span>
              )}
            </div>
          ) : null}
          <div className="cmux-custom-sidebar-swift-group-box-content">
            {node.children.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))}
          </div>
        </section>
      );
    }
    case "disclosureGroup": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const labelText =
        node.title ??
        node.labelChildren
          .map((child) =>
            child.kind === "text" || child.kind === "label" ? child.text : undefined,
          )
          .find((text) => text !== undefined && text.trim() !== "");
      const hasLabel = node.title !== undefined || node.labelChildren.length > 0;
      return (
        <details
          className={className(
            `cmux-custom-sidebar-swift-disclosure-group${
              node.isExpanded ? " cmux-custom-sidebar-swift-disclosure-group-expanded" : ""
            }${editable ? " cmux-custom-sidebar-swift-control-editable" : ""}`,
          )}
          style={presentation.style}
          title={presentation.title}
          open={node.isExpanded}
          data-swift-disclosure-group={labelText ?? "true"}
          data-swift-disclosure-expanded={String(node.isExpanded)}
          data-swift-state-binding={node.stateBindingKey}
          {...(labelText !== undefined
            ? { "data-swift-disclosure-group-label": labelText }
            : {})}
          {...nodeProps}
          onToggle={
            editable
              ? (event) => {
                  setLocalStateValue(node.stateBindingKey!, event.currentTarget.open);
                }
              : undefined
          }
        >
          <summary className="cmux-custom-sidebar-swift-disclosure-summary">
            {hasLabel ? (
              node.labelChildren.length > 0 ? (
                node.labelChildren.map((child, index) => (
                  <SwiftSidebarNodeView
                    key={index}
                    node={child}
                    context={context}
                    onCustomAction={onCustomAction}
                  />
                ))
              ) : (
                <span>{node.title}</span>
              )
            ) : (
              <span>Details</span>
            )}
          </summary>
          <div className="cmux-custom-sidebar-swift-disclosure-content">
            {node.children.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))}
          </div>
        </details>
      );
    }
    case "zstack":
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-zstack${swiftStackAlignmentClass(node.alignment)}`,
          )}
          style={stackStyle(node.spacing, node.alignment, "zstack")}
          title={presentation.title}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    case "hstack":
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-stack cmux-custom-sidebar-swift-stack-horizontal${swiftLazyStackClass(node.lazyStack)}${swiftPinnedViewsClass(node.pinnedViews)}${swiftStackAlignmentClass(node.alignment)}`,
          )}
          style={stackStyle(node.spacing, node.alignment, "horizontal")}
          title={presentation.title}
          {...(node.pinnedViews !== undefined ? { "data-swift-pinned-views": node.pinnedViews } : {})}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    case "splitView":
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-split cmux-custom-sidebar-swift-split-${node.axis}`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <div key={index} className="cmux-custom-sidebar-swift-split-pane">
              <SwiftSidebarNodeView
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            </div>
          ))}
        </div>
      );
    case "scrollView":
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-scroll cmux-custom-sidebar-swift-scroll-${node.axis}${
              node.showsIndicators ? "" : " cmux-custom-sidebar-swift-scroll-no-indicators"
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          data-swift-scroll-axis={node.axis}
          data-swift-scroll-shows-indicators={node.showsIndicators}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    case "text":
      return (
        <span
          className={className(
            `cmux-custom-sidebar-swift-text${
              node.textStyle !== undefined
                ? ` cmux-custom-sidebar-swift-text-style-${swiftToken(node.textStyle, "style")}`
                : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          data-swift-text-style={node.textStyle}
          data-swift-timer-interval-start-ms={node.timerIntervalStartMs}
          data-swift-timer-interval-end-ms={node.timerIntervalEndMs}
          data-swift-timer-counts-down={node.timerCountsDown}
          {...nodeProps}
        >
          {node.markdownRuns === undefined
            ? node.text
            : renderSwiftTextRuns(node.markdownRuns)}
        </span>
      );
    case "externalLink": {
      const labelText =
        node.title ??
        node.labelChildren
          .map((child) =>
            child.kind === "text" || child.kind === "label" ? child.text : undefined,
          )
          .find((text) => text !== undefined && text.trim() !== "");
      const labelContent =
        node.labelChildren.length > 0
          ? node.labelChildren.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))
          : <span>{node.title ?? node.href ?? "Link"}</span>;
      const safeHref = presentation.disabled ? undefined : node.href;
      const linkProps = {
        className: className(
          `cmux-custom-sidebar-swift-link${
            safeHref === undefined ? " cmux-custom-sidebar-swift-link-disabled" : ""
          }`,
        ),
        style: presentation.style,
        title: presentation.title,
        "data-swift-link": labelText ?? node.href ?? "true",
        "data-swift-link-destination": node.href,
        ...(safeHref === undefined ? { "data-swift-link-blocked": "true" } : {}),
        ...nodeProps,
      };
      return safeHref === undefined ? (
        <span role="link" aria-disabled="true" {...linkProps}>
          {labelContent}
        </span>
      ) : (
        <a href={safeHref} rel="noreferrer" target="_blank" {...linkProps}>
          {labelContent}
        </a>
      );
    }
    case "contentUnavailable": {
      const labelText =
        node.title ??
        node.labelChildren
          .map((child) =>
            child.kind === "text" || child.kind === "label" ? child.text : undefined,
          )
          .find((text) => text !== undefined && text.trim() !== "");
      return (
        <section
          className={className("cmux-custom-sidebar-swift-content-unavailable")}
          style={presentation.style}
          title={presentation.title}
          data-swift-content-unavailable={labelText ?? "true"}
          {...(node.systemImage !== undefined
            ? { "data-swift-system-image": node.systemImage }
            : {})}
          {...nodeProps}
        >
          {node.systemImage !== undefined ? (
            <span
              className="cmux-custom-sidebar-swift-content-unavailable-icon"
              aria-hidden="true"
              data-swift-system-image-glyph={swiftSystemImageGlyph(node.systemImage)}
            >
              {swiftSystemImageGlyph(node.systemImage)}
            </span>
          ) : null}
          <div className="cmux-custom-sidebar-swift-content-unavailable-label">
            {node.labelChildren.length > 0 ? (
              node.labelChildren.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))
            ) : (
              <span>{node.title ?? "No content"}</span>
            )}
          </div>
          {node.descriptionChildren.length > 0 ? (
            <div className="cmux-custom-sidebar-swift-content-unavailable-description">
              {node.descriptionChildren.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </div>
          ) : null}
          {node.actionsChildren.length > 0 ? (
            <div className="cmux-custom-sidebar-swift-content-unavailable-actions">
              {node.actionsChildren.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </div>
          ) : null}
        </section>
      );
    }
    case "navigationLink": {
      const canNavigate = navigation !== null && node.destination.length > 0;
      return (
        <button
          type="button"
          className={className(
            `cmux-custom-sidebar-swift-navigation-link${
              canNavigate ? "" : " cmux-custom-sidebar-swift-navigation-link-disabled"
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          disabled={!canNavigate}
          data-navigation-value={node.value}
          onClick={() => {
            if (canNavigate) {
              navigation.push({ title: node.title, destination: node.destination });
            }
          }}
          {...nodeProps}
        >
          <span className="cmux-custom-sidebar-swift-navigation-link-label">
            {node.children.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))}
          </span>
          <span className="cmux-custom-sidebar-swift-navigation-link-chevron" aria-hidden="true">
            {"\u203a"}
          </span>
        </button>
      );
    }
    case "image":
      return (
        <span
          className={className("cmux-custom-sidebar-swift-image")}
          style={presentation.style}
          title={presentation.title}
          aria-label={node.systemName}
          data-swift-system-image={node.systemName}
          data-swift-system-image-glyph={swiftSystemImageGlyph(node.systemName)}
          {...nodeProps}
        >
          {swiftSystemImageGlyph(node.systemName)}
        </span>
      );
    case "assetImage": {
      const { "aria-label": _ariaLabel, "aria-hidden": _ariaHidden, ...decorativeNodeProps } =
        nodeProps;
      const assetNodeProps = node.decorative ? decorativeNodeProps : nodeProps;
      return node.url === undefined ? (
        <span
          className={className(
            "cmux-custom-sidebar-swift-image cmux-custom-sidebar-swift-asset-image cmux-custom-sidebar-swift-asset-image-empty",
          )}
          style={presentation.style}
          title={presentation.title ?? `Missing Image asset: ${node.name}`}
          {...assetNodeProps}
          aria-label={node.decorative ? undefined : `Missing Image asset: ${node.name}`}
          aria-hidden={node.decorative ? true : accessibilityProps["aria-hidden"]}
        >
          asset
        </span>
      ) : (
        <img
          className={className(
            "cmux-custom-sidebar-swift-image cmux-custom-sidebar-swift-asset-image",
          )}
          style={presentation.style}
          title={presentation.title}
          src={node.url}
          alt={node.decorative ? "" : (presentation.ariaLabel ?? node.name)}
          loading="lazy"
          decoding="async"
          referrerPolicy="no-referrer"
          {...assetNodeProps}
          aria-hidden={node.decorative ? true : accessibilityProps["aria-hidden"]}
        />
      );
    }
    case "asyncImage":
      if (node.url !== undefined && node.successChildren !== undefined) {
        return (
          <span
            className={className(
              "cmux-custom-sidebar-swift-image cmux-custom-sidebar-swift-async-image cmux-custom-sidebar-swift-async-image-content",
            )}
            style={presentation.style}
            title={presentation.title}
            data-swift-async-image-phase="success"
            data-swift-async-image-url={node.url}
            data-swift-async-image-content-count={node.successChildren.length}
            data-swift-async-image-placeholder-count={node.placeholderChildren?.length}
            {...nodeProps}
          >
            {node.successChildren.map((child, index) => (
              <SwiftSidebarNodeView
                key={`async-success-${index}`}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))}
          </span>
        );
      }
      return node.url === undefined ? (
        <span
          className={className(
            "cmux-custom-sidebar-swift-image cmux-custom-sidebar-swift-async-image cmux-custom-sidebar-swift-async-image-empty",
          )}
          style={presentation.style}
          title={presentation.title ?? "Unsupported AsyncImage URL"}
          aria-label="Unsupported AsyncImage URL"
          data-swift-async-image-phase="failure"
          data-swift-async-image-placeholder-count={node.placeholderChildren?.length}
          {...nodeProps}
        >
          {node.placeholderChildren === undefined
            ? "image"
            : node.placeholderChildren.map((child, index) => (
                <SwiftSidebarNodeView
                  key={`async-placeholder-${index}`}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
        </span>
      ) : (
        <img
          className={className(
            "cmux-custom-sidebar-swift-image cmux-custom-sidebar-swift-async-image",
          )}
          style={presentation.style}
          title={presentation.title}
          src={node.url}
          alt={presentation.ariaLabel ?? ""}
          loading="lazy"
          decoding="async"
          referrerPolicy="no-referrer"
          data-swift-async-image-phase="success"
          data-swift-async-image-url={node.url}
          {...nodeProps}
        />
      );
    case "label":
      return (
        <span
          className={className("cmux-custom-sidebar-swift-label")}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          {node.systemImage ? (
            <span
              className="cmux-custom-sidebar-swift-image"
              aria-label={node.systemImage}
              data-swift-system-image={node.systemImage}
              data-swift-system-image-glyph={swiftSystemImageGlyph(node.systemImage)}
            >
              {swiftSystemImageGlyph(node.systemImage)}
            </span>
          ) : null}
          <span>{node.text}</span>
        </span>
      );
    case "labeledContent":
      return (
        <div
          className={className("cmux-custom-sidebar-swift-labeled-content")}
          style={presentation.style}
          title={presentation.title}
          data-swift-labeled-content={node.title ?? "true"}
          {...nodeProps}
        >
          <span className="cmux-custom-sidebar-swift-labeled-content-label">
            {node.title ?? "Label"}
          </span>
          <div className="cmux-custom-sidebar-swift-labeled-content-value">
            {node.children.length > 0 ? (
              node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))
            ) : (
              <span>{node.value ?? ""}</span>
            )}
          </div>
        </div>
      );
    case "progress": {
      const ratio =
        node.value === undefined
          ? undefined
          : Math.max(0, Math.min(1, node.value / (node.total ?? 1)));
      return (
        <div
          className={
            ratio === undefined
              ? className(
                  "cmux-custom-sidebar-swift-progress cmux-custom-sidebar-swift-progress-indeterminate",
                )
              : className("cmux-custom-sidebar-swift-progress")
          }
          style={presentation.style}
          title={presentation.title}
          role="progressbar"
          {...nodeProps}
          aria-valuemin={0}
          aria-valuemax={node.total ?? 1}
          aria-valuenow={node.value}
        >
          <span style={{ width: `${(ratio ?? 0.38) * 100}%` }} />
        </div>
      );
    }
    case "textField": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const inputLabel =
        node.placeholder ?? (node.multiline ? "Text editor" : node.secure ? "Secure field" : "Text field");
      return (
        <label
          className={className(
            `cmux-custom-sidebar-swift-text-field${
              node.multiline ? " cmux-custom-sidebar-swift-text-editor" : ""
            }${node.secure ? " cmux-custom-sidebar-swift-secure-field" : ""}${
              editable ? " cmux-custom-sidebar-swift-control-editable" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          {node.children.length > 0 ? (
            <span className="cmux-custom-sidebar-swift-text-field-label">
              {node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </span>
          ) : null}
          {node.multiline ? (
            <textarea
              className="cmux-custom-sidebar-swift-text-field-input cmux-custom-sidebar-swift-text-editor-input"
              value={node.text}
              placeholder={node.placeholder}
              aria-label={inputLabel}
              readOnly={!editable}
              disabled={presentation.disabled}
              rows={4}
              data-swift-state-binding={node.stateBindingKey}
              data-swift-on-change={hasOnChange ? "true" : undefined}
              data-swift-on-submit={hasOnSubmit ? "true" : undefined}
              onChange={
                editable
                  ? (event) => {
                      setLocalStateValue(node.stateBindingKey!, event.currentTarget.value);
                    }
                  : undefined
              }
              onKeyDown={
                editable && hasOnSubmit
                  ? (event) => {
                      if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                        event.preventDefault();
                        submitLocalStateValue();
                      }
                    }
                  : undefined
              }
            />
          ) : (
            <input
              className="cmux-custom-sidebar-swift-text-field-input"
              type={node.secure ? "password" : "text"}
              value={node.text}
              placeholder={node.placeholder}
              aria-label={inputLabel}
              readOnly={!editable}
              disabled={presentation.disabled}
              data-swift-state-binding={node.stateBindingKey}
              data-swift-on-change={hasOnChange ? "true" : undefined}
              data-swift-on-submit={hasOnSubmit ? "true" : undefined}
              onChange={
                editable
                  ? (event) => {
                      setLocalStateValue(node.stateBindingKey!, event.currentTarget.value);
                    }
                  : undefined
              }
              onKeyDown={
                editable && hasOnSubmit
                  ? (event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        submitLocalStateValue();
                      }
                    }
                  : undefined
              }
            />
          )}
        </label>
      );
    }
    case "stepper": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const currentValue = node.value ?? 0;
      const finiteLower = Number.isFinite(node.lowerBound) ? node.lowerBound : undefined;
      const finiteUpper = Number.isFinite(node.upperBound) ? node.upperBound : undefined;
      const clamped = (next: number): number =>
        Math.min(finiteUpper ?? next, Math.max(finiteLower ?? next, next));
      const decrementDisabled =
        !editable || (finiteLower !== undefined && currentValue <= finiteLower);
      const incrementDisabled =
        !editable || (finiteUpper !== undefined && currentValue >= finiteUpper);
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-stepper${
              editable ? " cmux-custom-sidebar-swift-control-editable" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          <div className="cmux-custom-sidebar-swift-stepper-label">
            {node.children.length > 0 ? (
              node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))
            ) : (
              <span>{node.title ?? "Stepper"}</span>
            )}
          </div>
          <div
            className="cmux-custom-sidebar-swift-stepper-control"
            role="spinbutton"
            aria-valuenow={currentValue}
            aria-valuemin={finiteLower}
            aria-valuemax={finiteUpper}
            aria-readonly={editable ? undefined : "true"}
            data-swift-state-binding={node.stateBindingKey}
            data-swift-on-change={hasOnChange ? "true" : undefined}
          >
            <button
              type="button"
              disabled={decrementDisabled}
              aria-label="Decrement"
              onClick={() =>
                setLocalStateValue(node.stateBindingKey!, clamped(currentValue - node.step))
              }
            >
              -
            </button>
            <span>{currentValue}</span>
            <button
              type="button"
              disabled={incrementDisabled}
              aria-label="Increment"
              onClick={() =>
                setLocalStateValue(node.stateBindingKey!, clamped(currentValue + node.step))
              }
            >
              +
            </button>
          </div>
        </div>
      );
    }
    case "slider": {
      const span = node.upperBound - node.lowerBound;
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const ratio =
        node.value === undefined || span <= 0
          ? 0
          : Math.max(0, Math.min(1, (node.value - node.lowerBound) / span));
      const sliderValue = node.value ?? node.lowerBound;
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-slider${
              editable ? " cmux-custom-sidebar-swift-control-editable" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          {node.children.length > 0 ? (
            <span className="cmux-custom-sidebar-swift-slider-label">
              {node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </span>
          ) : null}
          <div
            className="cmux-custom-sidebar-swift-slider-track"
            role={editable ? undefined : "slider"}
            aria-valuemin={editable ? undefined : node.lowerBound}
            aria-valuemax={editable ? undefined : node.upperBound}
            aria-valuenow={editable ? undefined : node.value}
            aria-readonly={editable ? undefined : "true"}
            aria-hidden={editable ? "true" : undefined}
          >
            <span
              className="cmux-custom-sidebar-swift-slider-fill"
              style={{ width: `${ratio * 100}%` }}
            />
            <span
              className="cmux-custom-sidebar-swift-slider-thumb"
              style={{ left: `${ratio * 100}%` }}
            />
          </div>
          {editable ? (
            <input
              className="cmux-custom-sidebar-swift-slider-input"
              type="range"
              min={node.lowerBound}
              max={node.upperBound}
              step="any"
              value={sliderValue}
              role="slider"
              aria-valuemin={node.lowerBound}
              aria-valuemax={node.upperBound}
              aria-valuenow={sliderValue}
              aria-label="Slider"
              data-swift-state-binding={node.stateBindingKey}
              data-swift-on-change={hasOnChange ? "true" : undefined}
              onChange={(event) => {
                setLocalStateValue(node.stateBindingKey!, Number(event.currentTarget.value));
              }}
            />
          ) : null}
        </div>
      );
    }
    case "picker": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const optionItems =
        node.options !== undefined && node.options.length > 0
          ? node.options
          : swiftPickerOptionLabels(node.children).map((label) => ({
              label,
              value: label,
              encodedValue: JSON.stringify(label),
            }));
      const selectedEncoded = JSON.stringify(node.selectedValue ?? node.selection);
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-picker${
              editable ? " cmux-custom-sidebar-swift-control-editable" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          <div className="cmux-custom-sidebar-swift-picker-header">
            {node.title ? (
              <span className="cmux-custom-sidebar-swift-picker-title">
                {interpolateCustomSidebarTemplate(node.title, context)}
              </span>
            ) : null}
            <span className="cmux-custom-sidebar-swift-picker-value">
              {node.selection || "None"}
            </span>
            {editable && optionItems.length > 0 ? (
              <select
                className="cmux-custom-sidebar-swift-picker-select"
                value={selectedEncoded}
                aria-label={node.title ?? "Picker"}
                data-swift-state-binding={node.stateBindingKey}
                data-swift-on-change={hasOnChange ? "true" : undefined}
                onChange={(event) => {
                  const option = optionItems.find(
                    (candidate) => candidate.encodedValue === event.currentTarget.value,
                  );
                  setLocalStateValue(
                    node.stateBindingKey!,
                    option?.value ?? event.currentTarget.value,
                  );
                }}
              >
                {optionItems.map((option) => (
                  <option
                    key={option.encodedValue}
                    value={option.encodedValue}
                    data-swift-picker-tag={option.encodedValue}
                  >
                    {option.label}
                  </option>
                ))}
              </select>
            ) : null}
          </div>
          {node.children.length > 0 ? (
            <div className="cmux-custom-sidebar-swift-picker-options" aria-hidden="true">
              {node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </div>
          ) : null}
        </div>
      );
    }
    case "datePicker": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const inputType = swiftDatePickerInputType(node.displayedComponents);
      const inputValue = swiftDatePickerInputValue(node.value, inputType);
      return (
        <label
          className={className(
            `cmux-custom-sidebar-swift-date-picker${
              editable ? " cmux-custom-sidebar-swift-control-editable" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          <span className="cmux-custom-sidebar-swift-date-picker-title">
            {node.title ?? "Date"}
          </span>
          <span className="cmux-custom-sidebar-swift-date-picker-value">
            {inputValue || node.value || "No date"}
          </span>
          <input
            className="cmux-custom-sidebar-swift-date-picker-input"
            type={inputType}
            value={inputValue}
            readOnly={!editable}
            disabled={presentation.disabled}
            aria-readonly={editable ? undefined : "true"}
            data-swift-state-binding={node.stateBindingKey}
            data-swift-on-change={hasOnChange ? "true" : undefined}
            onChange={
              editable
                ? (event) => {
                    setLocalStateValue(node.stateBindingKey!, event.currentTarget.value);
                  }
                : undefined
            }
          />
          {node.children.length > 0 ? (
            <span className="cmux-custom-sidebar-swift-date-picker-label">
              {node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </span>
          ) : null}
        </label>
      );
    }
    case "colorPicker": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      const inputValue = swiftColorPickerInputValue(node.value);
      return (
        <label
          className={className(
            `cmux-custom-sidebar-swift-color-picker${
              editable ? " cmux-custom-sidebar-swift-control-editable" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          <span
            className="cmux-custom-sidebar-swift-color-picker-swatch"
            style={{ background: inputValue }}
            aria-hidden="true"
          />
          <span className="cmux-custom-sidebar-swift-color-picker-title">
            {node.title ?? "Color"}
          </span>
          <span className="cmux-custom-sidebar-swift-color-picker-value">
            {node.value || inputValue}
          </span>
          <input
            className="cmux-custom-sidebar-swift-color-picker-input"
            type="color"
            value={inputValue}
            disabled={!editable}
            aria-readonly={editable ? undefined : "true"}
            data-swift-state-binding={node.stateBindingKey}
            data-swift-on-change={hasOnChange ? "true" : undefined}
            onChange={
              editable
                ? (event) => {
                    setLocalStateValue(node.stateBindingKey!, event.currentTarget.value);
                  }
                : undefined
            }
          />
          {node.children.length > 0 ? (
            <span className="cmux-custom-sidebar-swift-color-picker-label">
              {node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </span>
          ) : null}
        </label>
      );
    }
    case "toggle": {
      const editable = node.stateBindingKey !== undefined && !presentation.disabled;
      return (
        <div
          className={className(
            `cmux-custom-sidebar-swift-toggle${
              node.isOn ? " cmux-custom-sidebar-swift-toggle-on" : ""
            }${editable ? " cmux-custom-sidebar-swift-control-editable" : ""}`,
          )}
          style={presentation.style}
          title={presentation.title}
          role="switch"
          aria-checked={node.isOn}
          aria-readonly={editable ? undefined : "true"}
          aria-disabled={presentation.disabled ? "true" : undefined}
          tabIndex={editable ? 0 : undefined}
          data-swift-state-binding={node.stateBindingKey}
          data-swift-on-change={hasOnChange ? "true" : undefined}
          {...nodeProps}
          onClick={
            editable
              ? () => {
                  setLocalStateValue(node.stateBindingKey!, !node.isOn);
                }
              : undefined
          }
          onKeyDown={
            editable
              ? (event) => {
                  if (event.key === " " || event.key === "Enter") {
                    event.preventDefault();
                    setLocalStateValue(node.stateBindingKey!, !node.isOn);
                  }
                }
              : undefined
          }
        >
          <span className="cmux-custom-sidebar-swift-toggle-track" aria-hidden="true">
            <span className="cmux-custom-sidebar-swift-toggle-knob" />
          </span>
          {node.text ? (
            <span className="cmux-custom-sidebar-swift-toggle-label">
              {interpolateCustomSidebarTemplate(node.text, context)}
            </span>
          ) : (
            <span className="cmux-custom-sidebar-swift-toggle-label">
              {node.children.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </span>
          )}
        </div>
      );
    }
    case "list":
      return (
        <div
          className={className(
            [
              "cmux-custom-sidebar-swift-list",
              node.dataId !== undefined ? "cmux-custom-sidebar-swift-list-data" : "",
              node.form ? "cmux-custom-sidebar-swift-form" : "",
            ].filter(Boolean).join(" "),
          )}
          style={{
            ...presentation.style,
            ...(node.form
              ? {
                  border: "1px solid rgba(148, 163, 184, 0.18)",
                  borderRadius: "14px",
                  padding: "8px",
                }
              : {}),
          }}
          title={presentation.title}
          {...(node.dataId !== undefined ? { "data-swift-list-id": node.dataId } : {})}
          {...(node.form ? { "data-swift-form": "true" } : {})}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    case "section":
      return (
        <section
          className={className("cmux-custom-sidebar-swift-section")}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          {node.title ? (
            <div className="cmux-custom-sidebar-swift-section-title">{node.title}</div>
          ) : null}
          {node.header !== undefined && node.header.length > 0 ? (
            <div className="cmux-custom-sidebar-swift-section-header">
              {node.header.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </div>
          ) : null}
          <div className="cmux-custom-sidebar-swift-section-body">
            {node.children.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))}
          </div>
          {node.footer !== undefined && node.footer.length > 0 ? (
            <div className="cmux-custom-sidebar-swift-section-footer">
              {node.footer.map((child, index) => (
                <SwiftSidebarNodeView
                  key={index}
                  node={child}
                  context={context}
                  onCustomAction={onCustomAction}
                />
              ))}
            </div>
          ) : null}
        </section>
      );
    case "grid": {
      const gridClassNames = [
        "cmux-custom-sidebar-swift-grid",
        `cmux-custom-sidebar-swift-grid-${node.gridKind}`,
        ...(node.gridItems ?? []).map(
          (item) => `cmux-custom-sidebar-swift-grid-item-${item.size}`,
        ),
      ];
      return (
        <div
          className={className(gridClassNames.join(" "))}
          style={swiftGridStyle(node, presentation.style)}
          title={presentation.title}
          data-swift-grid-kind={node.gridKind}
          data-swift-grid-items={swiftGridItemSummary(node.gridItems)}
          {...(node.pinnedViews !== undefined ? { "data-swift-pinned-views": node.pinnedViews } : {})}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    }
    case "gridRow":
      return (
        <div
          className={className("cmux-custom-sidebar-swift-grid-row")}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          {node.children.map((child, index) => (
            <SwiftSidebarNodeView
              key={index}
              node={child}
              context={context}
              onCustomAction={onCustomAction}
            />
          ))}
        </div>
      );
    case "menu":
      return (
        <details
          className={className("cmux-custom-sidebar-swift-menu")}
          style={presentation.style}
          title={presentation.title}
          {...nodeProps}
        >
          <summary>{node.title ?? "Menu"}</summary>
          <div className="cmux-custom-sidebar-swift-menu-body">
            {node.children.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))}
          </div>
        </details>
      );
    case "shape":
      return (
        <span
          className={className(
            [
              `cmux-custom-sidebar-swift-shape cmux-custom-sidebar-swift-shape-${node.shape}`,
              node.cornerStyle !== undefined
                ? `cmux-custom-sidebar-swift-shape-style-${node.cornerStyle}`
                : undefined,
            ]
              .filter(Boolean)
              .join(" "),
          )}
          style={{
            ...presentation.style,
            ...(node.radius !== undefined ? { borderRadius: `${node.radius}px` } : {}),
            ...(node.pathWidth !== undefined ? { width: `${node.pathWidth}px` } : {}),
            ...(node.pathHeight !== undefined ? { height: `${node.pathHeight}px` } : {}),
          }}
          title={presentation.title}
          aria-hidden="true"
          data-swift-path={
            node.shape === "pathRoundedRect"
              ? "roundedRect"
              : node.shape === "pathEllipse"
                ? "ellipseIn"
                : undefined
          }
          data-swift-path-x={node.pathX}
          data-swift-path-y={node.pathY}
          data-swift-path-width={node.pathWidth}
          data-swift-path-height={node.pathHeight}
          data-swift-shape-style={node.cornerStyle}
          {...nodeProps}
        />
      );
    case "divider":
      return (
        <div
          className={className("cmux-custom-sidebar-swift-divider")}
          style={presentation.style}
          title={presentation.title}
          aria-hidden="true"
          {...nodeProps}
        />
      );
    case "spacer":
      return (
        <span
          className={className("cmux-custom-sidebar-swift-spacer")}
          style={{
            ...presentation.style,
            ...(node.minLength !== undefined
              ? { minWidth: `${node.minLength}px`, minHeight: `${node.minLength}px` }
              : {}),
          }}
          title={presentation.title}
          aria-hidden="true"
          data-swift-spacer-min-length={node.minLength}
          {...nodeProps}
        />
      );
    case "empty":
      return null;
    case "button": {
      const isLongPressAction = node.actionTrigger === "longPress";
      const tapCount = Math.max(1, Math.floor(node.tapCount ?? 1));
      const canRunLocalAction =
        node.localAction === "dismissPresentation" && presentationContext !== null;
      const canRunAction = node.action !== undefined || canRunLocalAction;
      return (
        <button
          type="button"
          className={className(
            `cmux-custom-sidebar-swift-button${
              node.role ? ` cmux-custom-sidebar-swift-button-${node.role}` : ""
            }${isLongPressAction ? " cmux-custom-sidebar-swift-button-long-press" : ""}${
              tapCount > 1 ? " cmux-custom-sidebar-swift-button-multi-tap" : ""
            }`,
          )}
          style={presentation.style}
          title={presentation.title}
          disabled={!canRunAction || presentation.disabled}
          data-swift-local-action={node.localAction}
          data-swift-gesture={node.actionTrigger}
          data-swift-tap-count={tapCount > 1 ? tapCount : undefined}
          data-swift-long-press-duration-ms={isLongPressAction ? 550 : undefined}
          data-tap-count={tapCount > 1 ? tapCount : undefined}
          {...nodeProps}
          onClick={() => {
            if (
              node.action !== undefined &&
              !presentation.disabled &&
              !isLongPressAction &&
              tapCount <= 1
            ) {
              onCustomAction(node.action);
            } else if (
              canRunLocalAction &&
              !presentation.disabled &&
              !isLongPressAction &&
              tapCount <= 1
            ) {
              presentationContext.dismiss();
            }
          }}
          onDoubleClick={() => {
            if (
              node.action !== undefined &&
              !presentation.disabled &&
              !isLongPressAction &&
              tapCount > 1
            ) {
              onCustomAction(node.action);
            }
          }}
          onPointerCancel={clearLongPressTimer}
          onPointerDown={() => {
            if (node.action === undefined || presentation.disabled || !isLongPressAction) {
              return;
            }
            clearLongPressTimer();
            longPressTimer.current = setTimeout(() => {
              if (node.action !== undefined && !presentation.disabled) {
                onCustomAction(node.action);
              }
              clearLongPressTimer();
            }, 550);
          }}
          onPointerLeave={clearLongPressTimer}
          onPointerUp={clearLongPressTimer}
        >
          {node.text ? (
            <span>{interpolateCustomSidebarTemplate(node.text, context)}</span>
          ) : (
            node.children.map((child, index) => (
              <SwiftSidebarNodeView
                key={index}
                node={child}
                context={context}
                onCustomAction={onCustomAction}
              />
            ))
          )}
        </button>
      );
    }
  }
}

function renderSwiftTextRuns(runs: CustomSidebarSwiftTextRun[]): ReactNode {
  return runs.map((run, index) => {
    switch (run.kind) {
      case "strong":
        return <strong key={index}>{run.text}</strong>;
      case "emphasis":
        return <em key={index}>{run.text}</em>;
      case "code":
        return <code key={index}>{run.text}</code>;
      case "link":
        return (
          <a key={index} href={run.href} rel="noreferrer" target="_blank">
            {run.text}
          </a>
        );
      case "text":
      default:
        return run.text;
    }
  });
}

function swiftPresentationTitle(name: string): string {
  switch (name) {
    case "confirmationDialog":
      return "Confirmation";
    case "fullScreenCover":
      return "Full screen cover";
    default:
      return name.slice(0, 1).toUpperCase() + name.slice(1);
  }
}

function swiftToolbarChromeClassName(
  presentation: ReturnType<typeof swiftModifierPresentation>,
): string {
  const classes: string[] = [];
  if (presentation.toolbarBackground !== undefined) {
    classes.push(
      "cmux-custom-sidebar-swift-toolbar-background",
      `cmux-custom-sidebar-swift-toolbar-background-${presentation.toolbarBackground}`,
    );
    if (presentation.toolbarBackgroundBars !== undefined) {
      classes.push(
        `cmux-custom-sidebar-swift-toolbar-background-for-${swiftToken(
          presentation.toolbarBackgroundBars,
          "automatic",
        )}`,
      );
    }
  }
  if (presentation.toolbarColorScheme !== undefined) {
    classes.push(
      "cmux-custom-sidebar-swift-toolbar-color-scheme",
      `cmux-custom-sidebar-swift-toolbar-color-scheme-${presentation.toolbarColorScheme}`,
    );
    if (presentation.toolbarColorSchemeBars !== undefined) {
      classes.push(
        `cmux-custom-sidebar-swift-toolbar-color-scheme-for-${swiftToken(
          presentation.toolbarColorSchemeBars,
          "automatic",
        )}`,
      );
    }
  }
  return classes.length === 0 ? "" : ` ${classes.join(" ")}`;
}

function swiftToolbarChromeStyle(
  presentation: ReturnType<typeof swiftModifierPresentation>,
): CSSProperties | undefined {
  const style: CSSProperties = {
    ...(presentation.toolbarBackgroundStyle ?? {}),
  };
  if (
    presentation.toolbarColorScheme === "dark" ||
    presentation.toolbarColorScheme === "light"
  ) {
    style.colorScheme = presentation.toolbarColorScheme;
  }
  return Object.keys(style).length === 0 ? undefined : style;
}

function swiftToolbarChromeProps(
  presentation: ReturnType<typeof swiftModifierPresentation>,
): {
  "data-swift-toolbar-background"?: string;
  "data-swift-toolbar-background-for"?: string;
  "data-swift-toolbar-color-scheme"?: string;
  "data-swift-toolbar-color-scheme-for"?: string;
} {
  return {
    ...(presentation.toolbarBackground !== undefined
      ? { "data-swift-toolbar-background": presentation.toolbarBackground }
      : {}),
    ...(presentation.toolbarBackgroundBars !== undefined
      ? { "data-swift-toolbar-background-for": presentation.toolbarBackgroundBars }
      : {}),
    ...(presentation.toolbarColorScheme !== undefined
      ? { "data-swift-toolbar-color-scheme": presentation.toolbarColorScheme }
      : {}),
    ...(presentation.toolbarColorSchemeBars !== undefined
      ? { "data-swift-toolbar-color-scheme-for": presentation.toolbarColorSchemeBars }
      : {}),
  };
}

function swiftModifierPresentation(
  modifiers: CustomSidebarSwiftModifier[] | undefined,
): {
  ariaHidden?: boolean;
  ariaLabel?: string;
  ariaValueText?: string;
  accessibilityHint?: string;
  accessibilityTraits?: string;
  accessibilityElementChildren?: string;
  accessibilityActionName?: string;
  hasAccessibilityAction?: boolean;
  accessibilityActivationPoint?: string;
  accessibilityActivationPointX?: string;
  accessibilityActivationPointY?: string;
  accessibilitySortPriority?: string;
  animationName?: string;
  animationValue?: string;
  transitionName?: string;
  contentTransitionName?: string;
  contentShape?: string;
  safeAreaPaddingEdges?: string;
  safeAreaPaddingLength?: string;
  safeAreaPaddingInsets?: string;
  contentMarginsEdges?: string;
  contentMarginsLength?: string;
  contentMarginsPlacement?: string;
  contentMarginsInsets?: string;
  clipShapeStyle?: string;
  clipShapeAntialiased?: boolean;
  coordinateSpace?: string;
  alignmentGuide?: string;
  alignmentGuideOffset?: string;
  hasVisualEffect?: boolean;
  containerRelativeFrameAxis?: string;
  containerRelativeFrameCount?: string;
  containerRelativeFrameSpan?: string;
  containerRelativeFrameSpacing?: string;
  containerRelativeFrameAlignment?: string;
  gridCellAnchor?: string;
  gridCellColumns?: string;
  gridColumnAlignment?: string;
  scrollTargetBehavior?: string;
  scrollTargetLayout?: string;
  scrollBounceBehavior?: string;
  scrollBounceAxes?: string;
  scrollDisabled?: boolean;
  scrollPositionId?: string;
  scrollPositionAnchor?: string;
  scrollPositionBinding?: string;
  defaultScrollAnchor?: string;
  tabViewStyle?: string;
  shapeStroke?: string;
  shapeStrokeColor?: string;
  shapeStrokeWidth?: string;
  symbolEffectName?: string;
  symbolEffectValue?: string;
  symbolEffectActive?: boolean;
  symbolEffectsRemoved?: boolean;
  labelsHidden?: boolean;
  controlGroupStyle?: string;
  groupBoxStyle?: string;
  dynamicTypeSize?: string;
  preferredColorScheme?: string;
  environmentColorScheme?: string;
  environmentLayoutDirection?: string;
  flipsForRightToLeftLayoutDirection?: boolean;
  redacted: boolean;
  redactionReason?: string;
  privacySensitive: boolean;
  unredacted: boolean;
  allowsHitTesting?: boolean;
  hidden?: boolean;
  badge?: string;
  hoverEffect?: string;
  hoverEffectEnabled?: boolean;
  defaultHoverEffect?: string;
  hasOnAppear: boolean;
  hasOnDisappear: boolean;
  hasTask: boolean;
  hasOnHover: boolean;
  hasOnGeometryChange: boolean;
  onGeometryChangeType?: string;
  isFocusable: boolean;
  focusedStateBindingKey?: string;
  focusedValue?: boolean;
  taskId?: string;
  taskIdExpression?: string;
  className: string;
  disabled: boolean;
  draggableValue?: string;
  dropDestinationType?: string;
  identityValue?: string;
  keyboardShortcut?: string;
  toolbarBackground?: string;
  toolbarBackgroundBars?: string;
  toolbarBackgroundStyle?: CSSProperties;
  toolbarColorScheme?: string;
  toolbarColorSchemeBars?: string;
  navigationDisplayMode?: string;
  navigationSubtitle?: string;
  navigationTitle?: string;
  style: CSSProperties;
  title?: string;
} {
  const style: CSSProperties = {};
  const classes: string[] = [];
  const transforms: string[] = [];
  const filters: string[] = [];
  let ariaHidden: boolean | undefined;
  let ariaLabel: string | undefined;
  let ariaValueText: string | undefined;
  let accessibilityHint: string | undefined;
  let accessibilityTraits: string | undefined;
  let accessibilityElementChildren: string | undefined;
  let accessibilityActionName: string | undefined;
  let hasAccessibilityAction = false;
  let accessibilityActivationPoint: string | undefined;
  let accessibilityActivationPointX: string | undefined;
  let accessibilityActivationPointY: string | undefined;
  let accessibilitySortPriority: string | undefined;
  let animationName: string | undefined;
  let animationValue: string | undefined;
  let transitionName: string | undefined;
  let contentTransitionName: string | undefined;
  let contentShape: string | undefined;
  let safeAreaPaddingEdges: string | undefined;
  let safeAreaPaddingLength: string | undefined;
  let safeAreaPaddingInsets: string | undefined;
  let contentMarginsEdges: string | undefined;
  let contentMarginsLength: string | undefined;
  let contentMarginsPlacement: string | undefined;
  let contentMarginsInsets: string | undefined;
  let clipShapeStyle: string | undefined;
  let clipShapeAntialiased: boolean | undefined;
  let coordinateSpace: string | undefined;
  let alignmentGuide: string | undefined;
  let alignmentGuideOffset: string | undefined;
  let hasVisualEffect = false;
  let containerRelativeFrameAxis: string | undefined;
  let containerRelativeFrameCount: string | undefined;
  let containerRelativeFrameSpan: string | undefined;
  let containerRelativeFrameSpacing: string | undefined;
  let containerRelativeFrameAlignment: string | undefined;
  let gridCellAnchor: string | undefined;
  let gridCellColumns: string | undefined;
  let gridColumnAlignment: string | undefined;
  let scrollTargetBehavior: string | undefined;
  let scrollTargetLayout: string | undefined;
  let scrollBounceBehavior: string | undefined;
  let scrollBounceAxes: string | undefined;
  let scrollDisabled: boolean | undefined;
  let scrollPositionId: string | undefined;
  let scrollPositionAnchor: string | undefined;
  let scrollPositionBinding: string | undefined;
  let defaultScrollAnchor: string | undefined;
  let tabViewStyle: string | undefined;
  let shapeStroke: string | undefined;
  let shapeStrokeColor: string | undefined;
  let shapeStrokeWidth: string | undefined;
  let symbolEffectName: string | undefined;
  let symbolEffectValue: string | undefined;
  let symbolEffectActive: boolean | undefined;
  let symbolEffectsRemoved: boolean | undefined;
  let labelsHidden: boolean | undefined;
  let controlGroupStyle: string | undefined;
  let groupBoxStyle: string | undefined;
  let dynamicTypeSize: string | undefined;
  let preferredColorScheme: string | undefined;
  let environmentColorScheme: string | undefined;
  let environmentLayoutDirection: string | undefined;
  let flipsForRightToLeftLayoutDirection: boolean | undefined;
  let hasOnAppear = false;
  let hasOnDisappear = false;
  let hasTask = false;
  let hasOnHover = false;
  let hasOnGeometryChange = false;
  let onGeometryChangeType: string | undefined;
  let isFocusable = false;
  let focusedStateBindingKey: string | undefined;
  let focusedValue: boolean | undefined;
  let taskId: string | undefined;
  let taskIdExpression: string | undefined;
  let draggableValue: string | undefined;
  let dropDestinationType: string | undefined;
  let identityValue: string | undefined;
  let disabled = false;
  let keyboardShortcut: string | undefined;
  let toolbarBackground: string | undefined;
  let toolbarBackgroundBars: string | undefined;
  let toolbarBackgroundStyle: CSSProperties | undefined;
  let toolbarColorScheme: string | undefined;
  let toolbarColorSchemeBars: string | undefined;
  let navigationDisplayMode: string | undefined;
  let navigationSubtitle: string | undefined;
  let navigationTitle: string | undefined;
  let redacted = false;
  let redactionReason: string | undefined;
  let privacySensitive = false;
  let unredacted = false;
  let allowsHitTesting: boolean | undefined;
  let hidden: boolean | undefined;
  let badge: string | undefined;
  let hoverEffect: string | undefined;
  let hoverEffectEnabled: boolean | undefined;
  let defaultHoverEffect: string | undefined;
  let title: string | undefined;
  for (const modifier of modifiers ?? []) {
    switch (modifier.name) {
      case "font":
        Object.assign(style, swiftFontStyle(modifier.value));
        break;
      case "fontWeight":
        style.fontWeight = swiftFontWeight(modifier.value);
        break;
      case "fontDesign": {
        const design = swiftToken(modifier.value, "");
        if (design) classes.push(`cmux-custom-sidebar-swift-font-design-${design}`);
        break;
      }
      case "fontWidth": {
        const width = swiftFontWidth(modifier.value);
        if (width !== undefined) {
          style.fontStretch = width;
          classes.push(
            `cmux-custom-sidebar-swift-font-width-${swiftToken(modifier.value, "standard")}`,
          );
        }
        break;
      }
      case "dynamicTypeSize": {
        const token = swiftToken(modifier.value, "medium");
        const fontSize = swiftDynamicTypeFontSize(token);
        dynamicTypeSize = token;
        classes.push(`cmux-custom-sidebar-swift-dynamic-type-${token}`);
        if (fontSize !== undefined) {
          style.fontSize = fontSize;
        }
        break;
      }
      case "bold":
        if (modifier.boolValue !== false) {
          style.fontWeight = 760;
        }
        break;
      case "italic":
        if (modifier.boolValue !== false) {
          style.fontStyle = "italic";
        }
        break;
      case "monospaced":
        classes.push("cmux-custom-sidebar-swift-monospace");
        break;
      case "monospacedDigit":
        classes.push("cmux-custom-sidebar-swift-monospaced-digit");
        style.fontVariantNumeric = "tabular-nums";
        break;
      case "foregroundColor":
        applySwiftForegroundStyle(style, classes, modifier.value);
        break;
      case "padding": {
        if (
          modifier.paddingTop !== undefined ||
          modifier.paddingLeading !== undefined ||
          modifier.paddingBottom !== undefined ||
          modifier.paddingTrailing !== undefined
        ) {
          if (modifier.paddingTop !== undefined) {
            style.paddingTop = `${Math.max(0, modifier.paddingTop)}px`;
          }
          if (modifier.paddingLeading !== undefined) {
            style.paddingLeft = `${Math.max(0, modifier.paddingLeading)}px`;
          }
          if (modifier.paddingBottom !== undefined) {
            style.paddingBottom = `${Math.max(0, modifier.paddingBottom)}px`;
          }
          if (modifier.paddingTrailing !== undefined) {
            style.paddingRight = `${Math.max(0, modifier.paddingTrailing)}px`;
          }
          break;
        }
        const amount = modifier.value === undefined ? 8 : Number(modifier.value);
        const cssAmount = `${Number.isFinite(amount) ? amount : 8}px`;
        const edges = modifier.edge?.split(",").filter(Boolean) ?? [];
        if (edges.length === 0 || edges.length >= 4) {
          style.padding = cssAmount;
          break;
        }
        if (edges.includes("top")) style.paddingTop = cssAmount;
        if (edges.includes("bottom")) style.paddingBottom = cssAmount;
        if (edges.includes("leading")) style.paddingLeft = cssAmount;
        if (edges.includes("trailing")) style.paddingRight = cssAmount;
        break;
      }
      case "safeAreaPadding": {
        const metadata = applySwiftPaddingLikeStyle(style, modifier);
        safeAreaPaddingEdges = metadata.edges;
        safeAreaPaddingLength = metadata.length;
        safeAreaPaddingInsets = metadata.insets;
        classes.push("cmux-custom-sidebar-swift-safe-area-padding");
        for (const edge of (metadata.edges ?? "custom").split(",").filter(Boolean)) {
          classes.push(`cmux-custom-sidebar-swift-safe-area-padding-${swiftToken(edge, "custom")}`);
        }
        break;
      }
      case "contentMargins": {
        const metadata = applySwiftPaddingLikeStyle(style, modifier);
        contentMarginsEdges = metadata.edges;
        contentMarginsLength = metadata.length;
        contentMarginsInsets = metadata.insets;
        contentMarginsPlacement = modifier.placement;
        classes.push("cmux-custom-sidebar-swift-content-margins");
        for (const edge of (metadata.edges ?? "custom").split(",").filter(Boolean)) {
          classes.push(`cmux-custom-sidebar-swift-content-margins-${swiftToken(edge, "custom")}`);
        }
        if (modifier.placement !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-content-margins-${swiftToken(
              modifier.placement,
              "automatic",
            )}`,
          );
        }
        break;
      }
      case "gridCellColumns": {
        const columns = Math.max(1, Math.floor(Number(modifier.value ?? 1)));
        gridCellColumns = String(columns);
        style.gridColumn = `span ${columns}`;
        classes.push("cmux-custom-sidebar-swift-grid-cell-columns");
        classes.push(`cmux-custom-sidebar-swift-grid-cell-columns-${columns}`);
        break;
      }
      case "gridColumnAlignment": {
        const alignment = swiftHorizontalAlignmentToken(modifier.value);
        if (alignment !== undefined) {
          gridColumnAlignment = alignment;
          style.justifySelf = swiftGridSelfAlignment(alignment);
          classes.push(
            `cmux-custom-sidebar-swift-grid-column-alignment-${swiftToken(alignment, "center")}`,
          );
        }
        break;
      }
      case "gridCellAnchor": {
        const anchor = swiftGridCellAnchorToken(modifier.value);
        if (anchor !== undefined) {
          gridCellAnchor = anchor;
          style.placeSelf = swiftGridCellAnchorPlaceSelf(anchor);
          classes.push(
            `cmux-custom-sidebar-swift-grid-cell-anchor-${swiftToken(anchor, "center")}`,
          );
        }
        break;
      }
      case "background":
        style.background = swiftBackgroundStyle(modifier.value);
        if (swiftMaterialToken(modifier.value) !== undefined) {
          (style as CSSProperties & Record<string, string>).backdropFilter =
            swiftMaterialBackdropFilter(modifier.value);
          (style as CSSProperties & Record<string, string>).WebkitBackdropFilter =
            swiftMaterialBackdropFilter(modifier.value);
          classes.push(
            `cmux-custom-sidebar-swift-material-${swiftToken(modifier.value, "regularMaterial")}`,
          );
        }
        break;
      case "cornerRadius": {
        const radius = modifier.value === undefined ? 10 : Number(modifier.value);
        style.borderRadius = `${Number.isFinite(radius) ? radius : 10}px`;
        break;
      }
      case "containerRelativeFrame": {
        const axis = modifier.value === "both" ? "both" : swiftToken(modifier.value, "vertical");
        containerRelativeFrameAxis = axis;
        containerRelativeFrameAlignment = modifier.frameAlignment;
        classes.push("cmux-custom-sidebar-swift-container-relative-frame");
        classes.push(`cmux-custom-sidebar-swift-container-relative-frame-${axis}`);
        const count =
          modifier.count !== undefined
            ? Math.max(1, Math.floor(modifier.count))
            : undefined;
        const span =
          modifier.span !== undefined
            ? Math.max(1, Math.floor(modifier.span))
            : undefined;
        const spacing =
          modifier.spacing !== undefined
            ? Math.max(0, modifier.spacing)
            : undefined;
        if (count !== undefined) containerRelativeFrameCount = String(count);
        if (span !== undefined) containerRelativeFrameSpan = String(span);
        if (spacing !== undefined) containerRelativeFrameSpacing = String(spacing);
        const cssSize = swiftContainerRelativeFrameSize(count, span, spacing);
        if (axis === "horizontal" || axis === "both") {
          style.width = cssSize;
          style.flexBasis = cssSize;
        }
        if (axis === "vertical" || axis === "both") {
          style.minHeight = cssSize;
        }
        if (modifier.frameAlignment !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-container-relative-frame-${swiftToken(
              modifier.frameAlignment,
              "center",
            )}`,
          );
        }
        break;
      }
      case "frame":
        if (modifier.frameWidth !== undefined) style.width = `${Math.max(0, modifier.frameWidth)}px`;
        if (modifier.frameHeight !== undefined) style.height = `${Math.max(0, modifier.frameHeight)}px`;
        if (modifier.frameMinWidth !== undefined) {
          style.minWidth = `${Math.max(0, modifier.frameMinWidth)}px`;
        }
        if (modifier.frameMinHeight !== undefined) {
          style.minHeight = `${Math.max(0, modifier.frameMinHeight)}px`;
        }
        if (modifier.frameMaxWidth !== undefined) {
          style.maxWidth = `${Math.max(0, modifier.frameMaxWidth)}px`;
        }
        if (modifier.frameMaxHeight !== undefined) {
          style.maxHeight = `${Math.max(0, modifier.frameMaxHeight)}px`;
        }
        if (modifier.frameIdealWidth !== undefined && modifier.frameWidth === undefined) {
          style.width = `${Math.max(0, modifier.frameIdealWidth)}px`;
        }
        if (modifier.frameIdealHeight !== undefined && modifier.frameHeight === undefined) {
          style.height = `${Math.max(0, modifier.frameIdealHeight)}px`;
        }
        if (modifier.frameAlignment !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-frame-${swiftToken(modifier.frameAlignment, "center")}`,
          );
          switch (modifier.frameAlignment) {
            case "trailing":
            case "topTrailing":
            case "bottomTrailing":
              style.textAlign = "right";
              style.justifyContent = "flex-end";
              break;
            case "center":
            case "top":
            case "bottom":
              style.textAlign = "center";
              style.justifyContent = "center";
              break;
            default:
              style.textAlign = "left";
              style.justifyContent = "flex-start";
              break;
          }
          switch (modifier.frameAlignment) {
            case "top":
            case "topLeading":
            case "topTrailing":
              style.alignItems = "flex-start";
              break;
            case "bottom":
            case "bottomLeading":
            case "bottomTrailing":
              style.alignItems = "flex-end";
              break;
            default:
              style.alignItems = "center";
              break;
          }
        }
        if (modifier.maxWidthInfinity === true) {
          classes.push("cmux-custom-sidebar-swift-fill");
        }
        break;
      case "layoutPriority": {
        const priority = Number(modifier.value);
        if (Number.isFinite(priority)) {
          style.flexGrow = Math.max(0, priority);
        }
        break;
      }
      case "offset":
        transforms.push(`translate(${modifier.x ?? 0}px, ${modifier.y ?? 0}px)`);
        break;
      case "position":
        style.position = "relative";
        style.left = `${modifier.x ?? 0}px`;
        style.top = `${modifier.y ?? 0}px`;
        classes.push("cmux-custom-sidebar-swift-positioned");
        break;
      case "zIndex": {
        const zIndex = Number(modifier.value);
        if (Number.isFinite(zIndex)) {
          style.position = "relative";
          style.zIndex = zIndex;
        }
        break;
      }
      case "aspectRatio": {
        const ratio = Number(modifier.value);
        if (Number.isFinite(ratio) && ratio > 0) {
          style.aspectRatio = String(ratio);
        }
        if (modifier.secondaryValue === "fit") {
          classes.push("cmux-custom-sidebar-swift-aspect-fit");
        }
        if (modifier.secondaryValue === "fill") {
          classes.push("cmux-custom-sidebar-swift-aspect-fill");
        }
        break;
      }
      case "clipped":
        style.overflow = "hidden";
        break;
      case "compositingGroup":
        style.isolation = "isolate";
        classes.push("cmux-custom-sidebar-swift-compositing-group");
        break;
      case "clipShape":
        style.overflow = "hidden";
        classes.push(
          `cmux-custom-sidebar-swift-clip-${swiftToken(modifier.value, "roundedRectangle")}`,
        );
        clipShapeStyle = modifier.fillStyle;
        clipShapeAntialiased = modifier.antialiased;
        if (modifier.fillStyle !== undefined) {
          classes.push(`cmux-custom-sidebar-swift-clip-style-${modifier.fillStyle}`);
        }
        if (modifier.antialiased !== undefined) {
          classes.push(
            modifier.antialiased
              ? "cmux-custom-sidebar-swift-clip-antialiased"
              : "cmux-custom-sidebar-swift-clip-antialiased-off",
          );
        }
        break;
      case "shadow":
        style.boxShadow = `${modifier.x ?? 0}px ${modifier.y ?? 3}px ${
          modifier.radius ?? 8
        }px ${swiftShadowColor(modifier.value)}`;
        break;
      case "border":
        style.border = `${Math.max(0, modifier.width ?? 1)}px solid ${swiftColor(
          modifier.value,
        )}`;
        break;
      case "strokeBorder":
        shapeStroke = "strokeBorder";
        shapeStrokeColor = modifier.value;
        shapeStrokeWidth = `${Math.max(0, modifier.width ?? 1)}`;
        style.border = `${Math.max(0, modifier.width ?? 1)}px solid ${swiftColor(
          modifier.value,
        )}`;
        classes.push("cmux-custom-sidebar-swift-shape-stroke-border");
        break;
      case "blur":
        filters.push(`blur(${Math.max(0, modifier.radius ?? 0)}px)`);
        break;
      case "brightness": {
        const amount = Number(modifier.value);
        if (Number.isFinite(amount)) filters.push(`brightness(${Math.max(0, 1 + amount)})`);
        break;
      }
      case "contrast": {
        const amount = Number(modifier.value);
        if (Number.isFinite(amount)) filters.push(`contrast(${Math.max(0, amount)})`);
        break;
      }
      case "saturation": {
        const amount = Number(modifier.value);
        if (Number.isFinite(amount)) filters.push(`saturate(${Math.max(0, amount)})`);
        break;
      }
      case "grayscale": {
        const amount = Number(modifier.value);
        if (Number.isFinite(amount)) {
          filters.push(`grayscale(${Math.max(0, Math.min(1, amount))})`);
        }
        break;
      }
      case "hueRotation": {
        const degrees = Number(modifier.value);
        if (Number.isFinite(degrees)) filters.push(`hue-rotate(${degrees}deg)`);
        break;
      }
      case "blendMode": {
        const blendMode = swiftBlendMode(modifier.value);
        if (blendMode !== undefined) {
          style.mixBlendMode = blendMode;
          classes.push(`cmux-custom-sidebar-swift-blend-${swiftToken(modifier.value, "normal")}`);
        }
        break;
      }
      case "rotationEffect": {
        const degrees = Number(modifier.value);
        if (Number.isFinite(degrees)) transforms.push(`rotate(${degrees}deg)`);
        break;
      }
      case "scaleEffect": {
        const scale = Number(modifier.value);
        if (Number.isFinite(scale)) transforms.push(`scale(${scale})`);
        break;
      }
      case "rotation3DEffect": {
        const degrees = Number(modifier.value);
        const x = modifier.x ?? 0;
        const y = modifier.y ?? 0;
        const z = modifier.z ?? 0;
        if (Number.isFinite(degrees) && (x !== 0 || y !== 0 || z !== 0)) {
          const perspective = modifier.perspective;
          if (perspective !== undefined && Number.isFinite(perspective) && perspective > 0) {
            transforms.push(`perspective(${Math.max(1, perspective * 1000)}px)`);
          }
          transforms.push(`rotate3d(${x}, ${y}, ${z}, ${degrees}deg)`);
          classes.push("cmux-custom-sidebar-swift-rotation3d");
          if (modifier.secondaryValue !== undefined) {
            style.transformOrigin = swiftTransformOrigin(modifier.secondaryValue);
            classes.push(
              `cmux-custom-sidebar-swift-rotation3d-anchor-${swiftToken(
                modifier.secondaryValue,
                "center",
              )}`,
            );
          }
        }
        break;
      }
      case "visualEffect":
        if (modifier.boolValue !== false) {
          hasVisualEffect = true;
          classes.push("cmux-custom-sidebar-swift-visual-effect");
        }
        break;
      case "lineLimit": {
        const lines = Number(modifier.value);
        if (Number.isFinite(lines) && lines > 0) {
          const clampedLines = Math.floor(lines);
          style.display = "-webkit-box";
          style.overflow = "hidden";
          style.WebkitBoxOrient = "vertical";
          style.WebkitLineClamp = clampedLines;
          if (modifier.boolValue === true) {
            classes.push("cmux-custom-sidebar-swift-line-limit-reserves-space");
            style.minHeight = `calc(${clampedLines} * 1.35em)`;
          }
        }
        break;
      }
      case "truncationMode":
        classes.push(
          `cmux-custom-sidebar-swift-truncate-${swiftToken(modifier.value, "tail")}`,
        );
        style.overflow = "hidden";
        style.textOverflow = "ellipsis";
        break;
      case "multilineTextAlignment":
        style.textAlign = swiftTextAlign(modifier.value);
        break;
      case "textCase": {
        const textCase = swiftToken(modifier.value, "");
        if (textCase === "uppercase" || textCase === "lowercase") {
          style.textTransform = textCase;
        }
        break;
      }
      case "tracking":
      case "kerning": {
        const spacing = Number(modifier.value);
        if (Number.isFinite(spacing)) {
          style.letterSpacing = `${spacing}px`;
          classes.push(`cmux-custom-sidebar-swift-${modifier.name}`);
        }
        break;
      }
      case "baselineOffset": {
        const offset = Number(modifier.value);
        if (Number.isFinite(offset)) {
          style.verticalAlign = `${offset}px`;
          classes.push("cmux-custom-sidebar-swift-baseline-offset");
        }
        break;
      }
      case "underline":
        if (modifier.boolValue !== false) {
          classes.push("cmux-custom-sidebar-swift-underline");
          if (modifier.secondaryValue !== undefined) {
            classes.push(
              `cmux-custom-sidebar-swift-underline-${swiftToken(
                modifier.secondaryValue,
                "solid",
              )}`,
            );
            style.textDecorationStyle = swiftTextDecorationStyle(modifier.secondaryValue);
          }
          if (modifier.value !== undefined) {
            style.textDecorationColor = swiftColor(modifier.value);
          }
        }
        break;
      case "strikethrough":
        if (modifier.boolValue !== false) {
          classes.push("cmux-custom-sidebar-swift-strikethrough");
          if (modifier.secondaryValue !== undefined) {
            classes.push(
              `cmux-custom-sidebar-swift-strikethrough-${swiftToken(
                modifier.secondaryValue,
                "solid",
              )}`,
            );
            style.textDecorationStyle = swiftTextDecorationStyle(modifier.secondaryValue);
          }
          if (modifier.value !== undefined) {
            style.textDecorationColor = swiftColor(modifier.value);
          }
        }
        break;
      case "opacity": {
        const opacity = Number(modifier.value);
        if (Number.isFinite(opacity)) {
          style.opacity = Math.max(0, Math.min(1, opacity));
        }
        break;
      }
      case "hidden":
        hidden = true;
        classes.push("cmux-custom-sidebar-swift-hidden");
        style.visibility = "hidden";
        break;
      case "fixedSize":
        classes.push("cmux-custom-sidebar-swift-fixed-size");
        break;
      case "badge": {
        const badgeValue = modifier.value?.trim();
        if (badgeValue !== undefined && badgeValue !== "") {
          badge = badgeValue;
          classes.push("cmux-custom-sidebar-swift-badge");
        }
        break;
      }
      case "allowsHitTesting":
        allowsHitTesting = modifier.boolValue ?? true;
        classes.push(
          allowsHitTesting
            ? "cmux-custom-sidebar-swift-allows-hit-testing"
            : "cmux-custom-sidebar-swift-allows-hit-testing-off",
        );
        if (!allowsHitTesting) {
          style.pointerEvents = "none";
        }
        break;
      case "disabled":
        disabled = modifier.boolValue ?? true;
        break;
      case "hoverEffect":
        hoverEffect = swiftToken(modifier.value, "automatic");
        hoverEffectEnabled = modifier.boolValue ?? true;
        classes.push(
          hoverEffectEnabled
            ? "cmux-custom-sidebar-swift-hover-effect"
            : "cmux-custom-sidebar-swift-hover-effect-disabled",
        );
        classes.push(`cmux-custom-sidebar-swift-hover-effect-${hoverEffect}`);
        break;
      case "defaultHoverEffect":
        defaultHoverEffect = swiftToken(modifier.value, "automatic");
        classes.push("cmux-custom-sidebar-swift-default-hover-effect");
        classes.push(
          `cmux-custom-sidebar-swift-default-hover-effect-${defaultHoverEffect}`,
        );
        break;
      case "help":
        title = modifier.value;
        break;
      case "accessibilityLabel":
        ariaLabel = modifier.value;
        break;
      case "accessibilityHidden":
        ariaHidden = modifier.boolValue ?? true;
        break;
      case "accessibilityValue":
        ariaValueText = modifier.value;
        break;
      case "accessibilityHint":
        accessibilityHint = modifier.value;
        break;
      case "accessibilityAddTraits":
        accessibilityTraits = modifier.value;
        if (modifier.value?.split(",").some((trait) => swiftToken(trait, "") === "isButton")) {
          classes.push("cmux-custom-sidebar-swift-accessibility-trait-button");
        }
        break;
      case "accessibilityElement":
        accessibilityElementChildren = modifier.value;
        if (modifier.value !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-accessibility-element-${swiftToken(
              modifier.value,
              "automatic",
            )}`,
          );
        }
        break;
      case "accessibilityAction":
        hasAccessibilityAction = true;
        accessibilityActionName = modifier.value ?? "default";
        classes.push("cmux-custom-sidebar-swift-accessibility-action");
        classes.push(
          `cmux-custom-sidebar-swift-accessibility-action-${swiftToken(
            accessibilityActionName,
            "default",
          )}`,
        );
        break;
      case "accessibilityActivationPoint":
        accessibilityActivationPoint = modifier.value;
        accessibilityActivationPointX =
          modifier.x !== undefined ? String(modifier.x) : undefined;
        accessibilityActivationPointY =
          modifier.y !== undefined ? String(modifier.y) : undefined;
        classes.push("cmux-custom-sidebar-swift-accessibility-activation-point");
        if (modifier.value !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-accessibility-activation-point-${swiftToken(
              modifier.value,
              "center",
            )}`,
          );
        }
        break;
      case "accessibilitySortPriority":
        accessibilitySortPriority = modifier.value;
        break;
      case "redacted":
        redacted = true;
        unredacted = false;
        redactionReason = swiftRedactionReasons(modifier.value);
        classes.push("cmux-custom-sidebar-swift-redacted-reason");
        if (redactionReason !== undefined) {
          for (const reason of redactionReason.split(",").filter(Boolean)) {
            classes.push(`cmux-custom-sidebar-swift-redacted-${swiftToken(reason, "placeholder")}`);
          }
        }
        break;
      case "privacySensitive":
        redacted = true;
        privacySensitive = true;
        unredacted = false;
        redactionReason = swiftRedactionReasons(redactionReason ?? "privacy");
        classes.push("cmux-custom-sidebar-swift-privacy-sensitive");
        break;
      case "unredacted":
        redacted = false;
        unredacted = true;
        classes.push("cmux-custom-sidebar-swift-unredacted");
        break;
      case "trim": {
        const from = Math.max(0, Math.min(1, modifier.x ?? 0));
        const to = Math.max(from, Math.min(1, modifier.y ?? 1));
        classes.push("cmux-custom-sidebar-swift-shape-trim");
        (
          style as CSSProperties & Record<string, string>
        )["--cmux-custom-sidebar-swift-shape-trim-from"] = `${from * 100}%`;
        (
          style as CSSProperties & Record<string, string>
        )["--cmux-custom-sidebar-swift-shape-trim-to"] = `${to * 100}%`;
        break;
      }
      case "navigationTitle":
        navigationTitle = modifier.value;
        break;
      case "navigationSubtitle":
        navigationSubtitle = modifier.value;
        break;
      case "navigationBarTitleDisplayMode":
        navigationDisplayMode = swiftToken(modifier.value, "automatic");
        break;
      case "navigationDestination":
        break;
      case "toolbarBackground": {
        const background = swiftToken(modifier.value, "automatic");
        toolbarBackground = background;
        toolbarBackgroundBars = modifier.secondaryValue;
        classes.push(`cmux-custom-sidebar-swift-toolbar-background-${background}`);
        if (modifier.secondaryValue !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-toolbar-background-for-${swiftToken(
              modifier.secondaryValue,
              "automatic",
            )}`,
          );
        }
        if (background !== "hidden" && background !== "visible" && background !== "automatic") {
          toolbarBackgroundStyle = { background: swiftBackgroundStyle(modifier.value) };
        }
        break;
      }
      case "toolbarColorScheme": {
        const colorScheme = swiftToken(modifier.value, "automatic");
        toolbarColorScheme = colorScheme;
        toolbarColorSchemeBars = modifier.secondaryValue;
        classes.push(`cmux-custom-sidebar-swift-toolbar-color-scheme-${colorScheme}`);
        if (modifier.secondaryValue !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-toolbar-color-scheme-for-${swiftToken(
              modifier.secondaryValue,
              "automatic",
            )}`,
          );
        }
        break;
      }
      case "keyboardShortcut":
        keyboardShortcut = swiftKeyboardShortcutAria(modifier.value, modifier.secondaryValue);
        classes.push("cmux-custom-sidebar-swift-keyboard-shortcut");
        for (const shortcutModifier of modifier.secondaryValue?.split(",").filter(Boolean) ?? []) {
          classes.push(
            `cmux-custom-sidebar-swift-keyboard-shortcut-${swiftToken(
              shortcutModifier,
              "modifier",
            )}`,
          );
        }
        break;
      case "id":
        identityValue = modifier.value;
        classes.push("cmux-custom-sidebar-swift-identified");
        break;
      case "contentShape":
        contentShape = swiftToken(modifier.value, "rectangle");
        classes.push(`cmux-custom-sidebar-swift-content-shape-${contentShape}`);
        break;
      case "coordinateSpace":
        coordinateSpace = modifier.value;
        classes.push("cmux-custom-sidebar-swift-coordinate-space");
        classes.push(
          `cmux-custom-sidebar-swift-coordinate-space-${swiftToken(
            modifier.value,
            "local",
          )}`,
        );
        break;
      case "alignmentGuide":
        alignmentGuide = modifier.value;
        alignmentGuideOffset = modifier.secondaryValue;
        classes.push("cmux-custom-sidebar-swift-alignment-guide");
        classes.push(
          `cmux-custom-sidebar-swift-alignment-guide-${swiftToken(
            modifier.value,
            "center",
          )}`,
        );
        if (modifier.secondaryValue !== undefined) {
          const offset = Number(modifier.secondaryValue);
          if (Number.isFinite(offset)) {
            if (swiftAlignmentGuideIsVertical(modifier.value)) {
              style.marginTop = `${offset}px`;
            } else {
              style.marginLeft = `${offset}px`;
            }
          }
        }
        break;
      case "draggable":
        draggableValue = modifier.value;
        classes.push("cmux-custom-sidebar-swift-draggable");
        break;
      case "dropDestination":
        dropDestinationType = modifier.value ?? "unknown";
        classes.push("cmux-custom-sidebar-swift-drop-destination");
        classes.push(
          `cmux-custom-sidebar-swift-drop-destination-${swiftToken(
            modifier.value,
            "unknown",
          )}`,
        );
        break;
      case "focusable":
        if (modifier.boolValue !== false) {
          isFocusable = true;
          classes.push("cmux-custom-sidebar-swift-focusable");
        }
        break;
      case "focused":
        isFocusable = true;
        focusedStateBindingKey = modifier.stateBindingKey;
        focusedValue = modifier.boolValue;
        classes.push("cmux-custom-sidebar-swift-focusable");
        if (modifier.boolValue === true) {
          classes.push("cmux-custom-sidebar-swift-focused");
        }
        break;
      case "controlSize":
        classes.push(
          `cmux-custom-sidebar-swift-control-size-${swiftToken(modifier.value, "regular")}`,
        );
        break;
      case "buttonBorderShape":
        classes.push(
          `cmux-custom-sidebar-swift-button-border-shape-${swiftToken(
            modifier.value,
            "automatic",
          )}`,
        );
        break;
      case "listRowBackground":
        style.background = swiftBackgroundColor(modifier.value);
        break;
      case "listRowSeparator":
        if (modifier.value?.replace(/^\./, "") === "hidden") {
          classes.push("cmux-custom-sidebar-swift-list-row-separator-hidden");
        }
        break;
      case "labelsHidden":
        labelsHidden = true;
        classes.push("cmux-custom-sidebar-swift-labels-hidden");
        break;
      case "labelStyle":
        classes.push(
          `cmux-custom-sidebar-swift-label-style-${swiftToken(modifier.value, "automatic")}`,
        );
        break;
      case "listStyle":
        classes.push(
          `cmux-custom-sidebar-swift-list-style-${swiftToken(modifier.value, "plain")}`,
        );
        break;
      case "menuStyle":
        classes.push(
          `cmux-custom-sidebar-swift-menu-style-${swiftToken(modifier.value, "automatic")}`,
        );
        break;
      case "controlGroupStyle":
        controlGroupStyle = swiftToken(modifier.value, "automatic");
        classes.push(
          `cmux-custom-sidebar-swift-control-group-style-${controlGroupStyle}`,
        );
        break;
      case "groupBoxStyle":
        groupBoxStyle = swiftToken(modifier.value, "automatic");
        classes.push(`cmux-custom-sidebar-swift-group-box-style-${groupBoxStyle}`);
        break;
      case "tabViewStyle":
        tabViewStyle = swiftTabViewStyleToken(modifier.value);
        classes.push(`cmux-custom-sidebar-swift-tab-view-style-${tabViewStyle}`);
        break;
      case "pickerStyle":
        classes.push(
          `cmux-custom-sidebar-swift-picker-style-${swiftToken(modifier.value, "automatic")}`,
        );
        break;
      case "toggleStyle":
        classes.push(
          `cmux-custom-sidebar-swift-toggle-style-${swiftToken(modifier.value, "automatic")}`,
        );
        break;
      case "textFieldStyle":
        classes.push(
          `cmux-custom-sidebar-swift-text-field-style-${swiftToken(
            modifier.value,
            "automatic",
          )}`,
        );
        break;
      case "scrollContentBackground":
        if (swiftToken(modifier.value, "") === "hidden") {
          classes.push("cmux-custom-sidebar-swift-scroll-content-background-hidden");
        }
        break;
      case "scrollIndicators":
        classes.push(
          `cmux-custom-sidebar-swift-scroll-indicators-${swiftToken(
            modifier.value,
            "automatic",
          )}`,
        );
        if (modifier.secondaryValue !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-scroll-indicators-axis-${swiftToken(
              modifier.secondaryValue,
              "all",
            )}`,
          );
        }
        break;
      case "scrollClipDisabled":
        if (modifier.boolValue !== false) {
          classes.push("cmux-custom-sidebar-swift-scroll-clip-disabled");
        }
        break;
      case "scrollTargetBehavior": {
        const behavior = swiftToken(modifier.value, "automatic");
        scrollTargetBehavior = behavior;
        classes.push(`cmux-custom-sidebar-swift-scroll-target-behavior-${behavior}`);
        break;
      }
      case "scrollTargetLayout":
        scrollTargetLayout = modifier.boolValue === false ? "false" : "true";
        if (modifier.boolValue !== false) {
          classes.push("cmux-custom-sidebar-swift-scroll-target-layout");
        }
        break;
      case "scrollBounceBehavior": {
        const behavior = swiftToken(modifier.value, "automatic");
        scrollBounceBehavior = behavior;
        scrollBounceAxes = modifier.secondaryValue;
        classes.push(`cmux-custom-sidebar-swift-scroll-bounce-${behavior}`);
        if (modifier.secondaryValue !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-scroll-bounce-axis-${swiftToken(
              modifier.secondaryValue,
              "all",
            )}`,
          );
        }
        break;
      }
      case "scrollDisabled":
        scrollDisabled = modifier.boolValue !== false;
        if (scrollDisabled) {
          classes.push("cmux-custom-sidebar-swift-scroll-disabled");
          style.overflow = "hidden";
        }
        break;
      case "scrollPosition":
        scrollPositionId = modifier.value;
        scrollPositionAnchor = modifier.secondaryValue;
        scrollPositionBinding = modifier.stateBindingKey;
        classes.push("cmux-custom-sidebar-swift-scroll-position");
        if (modifier.secondaryValue !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-scroll-position-anchor-${swiftToken(
              modifier.secondaryValue,
              "center",
            )}`,
          );
        }
        break;
      case "defaultScrollAnchor":
        defaultScrollAnchor = modifier.value;
        classes.push(
          `cmux-custom-sidebar-swift-default-scroll-anchor-${swiftToken(
            modifier.value,
            "center",
          )}`,
        );
        break;
      case "preferredColorScheme":
        if (modifier.value === "dark" || modifier.value === "light") {
          preferredColorScheme = modifier.value;
          style.colorScheme = modifier.value;
          classes.push(`cmux-custom-sidebar-swift-preferred-color-scheme-${modifier.value}`);
        }
        break;
      case "environment":
        if (
          modifier.value === "colorScheme" &&
          (modifier.secondaryValue === "dark" || modifier.secondaryValue === "light")
        ) {
          environmentColorScheme = modifier.secondaryValue;
          style.colorScheme = modifier.secondaryValue;
          classes.push(
            `cmux-custom-sidebar-swift-environment-color-scheme-${modifier.secondaryValue}`,
          );
        }
        if (
          modifier.value === "layoutDirection" &&
          (modifier.secondaryValue === "rightToLeft" || modifier.secondaryValue === "leftToRight")
        ) {
          environmentLayoutDirection = modifier.secondaryValue;
          style.direction = modifier.secondaryValue === "rightToLeft" ? "rtl" : "ltr";
          classes.push(
            `cmux-custom-sidebar-swift-environment-layout-direction-${modifier.secondaryValue}`,
          );
        }
        break;
      case "resizable":
        classes.push("cmux-custom-sidebar-swift-image-resizable");
        if (modifier.secondaryValue !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-image-resizing-${swiftToken(
              modifier.secondaryValue,
              "stretch",
            )}`,
          );
        }
        if (
          modifier.capInsetTop !== undefined ||
          modifier.capInsetLeading !== undefined ||
          modifier.capInsetBottom !== undefined ||
          modifier.capInsetTrailing !== undefined
        ) {
          classes.push("cmux-custom-sidebar-swift-image-cap-insets");
          const capInsetStyle = style as CSSProperties & Record<string, string>;
          if (modifier.capInsetTop !== undefined) {
            capInsetStyle["--cmux-custom-sidebar-swift-cap-inset-top"] = `${Math.max(
              0,
              modifier.capInsetTop,
            )}px`;
          }
          if (modifier.capInsetLeading !== undefined) {
            capInsetStyle["--cmux-custom-sidebar-swift-cap-inset-leading"] = `${Math.max(
              0,
              modifier.capInsetLeading,
            )}px`;
          }
          if (modifier.capInsetBottom !== undefined) {
            capInsetStyle["--cmux-custom-sidebar-swift-cap-inset-bottom"] = `${Math.max(
              0,
              modifier.capInsetBottom,
            )}px`;
          }
          if (modifier.capInsetTrailing !== undefined) {
            capInsetStyle["--cmux-custom-sidebar-swift-cap-inset-trailing"] = `${Math.max(
              0,
              modifier.capInsetTrailing,
            )}px`;
          }
        }
        break;
      case "renderingMode":
        classes.push(
          `cmux-custom-sidebar-swift-image-rendering-${swiftToken(
            modifier.value,
            "automatic",
          )}`,
        );
        break;
      case "interpolation":
        classes.push(
          `cmux-custom-sidebar-swift-image-interpolation-${swiftToken(
            modifier.value,
            "medium",
          )}`,
        );
        break;
      case "antialiased":
        classes.push(
          modifier.boolValue === false
            ? "cmux-custom-sidebar-swift-image-antialiased-off"
            : "cmux-custom-sidebar-swift-image-antialiased-on",
        );
        break;
      case "flipsForRightToLeftLayoutDirection":
        flipsForRightToLeftLayoutDirection = modifier.boolValue !== false;
        classes.push(
          flipsForRightToLeftLayoutDirection
            ? "cmux-custom-sidebar-swift-flips-for-rtl"
            : "cmux-custom-sidebar-swift-flips-for-rtl-disabled",
        );
        if (flipsForRightToLeftLayoutDirection) {
          style.scale = "-1 1";
        }
        break;
      case "imageScale":
        classes.push(
          `cmux-custom-sidebar-swift-image-scale-${swiftToken(modifier.value, "medium")}`,
        );
        break;
      case "symbolRenderingMode":
        classes.push(
          `cmux-custom-sidebar-swift-symbol-rendering-${swiftToken(
            modifier.value,
            "monochrome",
          )}`,
        );
        break;
      case "symbolVariant":
        classes.push(
          `cmux-custom-sidebar-swift-symbol-variant-${swiftToken(modifier.value, "none")}`,
        );
        break;
      case "buttonStyle":
        if (swiftToken(modifier.value, "") === "plain") {
          classes.push("cmux-custom-sidebar-swift-button-plain");
        } else {
          classes.push(
            `cmux-custom-sidebar-swift-button-style-${swiftToken(
              modifier.value,
              "automatic",
            )}`,
          );
        }
        break;
      case "animation":
        animationName = modifier.value;
        animationValue = modifier.secondaryValue;
        classes.push(
          `cmux-custom-sidebar-swift-animation-${swiftToken(modifier.value, "default")}`,
        );
        break;
      case "transition":
        transitionName = modifier.value;
        classes.push(
          `cmux-custom-sidebar-swift-transition-${swiftToken(modifier.value, "identity")}`,
        );
        break;
      case "contentTransition":
        contentTransitionName = modifier.value;
        classes.push(
          `cmux-custom-sidebar-swift-content-transition-${swiftToken(
            modifier.value,
            "identity",
          )}`,
        );
        break;
      case "symbolEffect":
        symbolEffectName = modifier.value;
        symbolEffectValue = modifier.secondaryValue;
        symbolEffectActive = modifier.boolValue;
        symbolEffectsRemoved = false;
        classes.push(
          `cmux-custom-sidebar-swift-symbol-effect-${swiftToken(modifier.value, "default")}`,
        );
        break;
      case "symbolEffectsRemoved":
        if (modifier.boolValue !== false) {
          symbolEffectName = undefined;
          symbolEffectValue = undefined;
          symbolEffectActive = undefined;
          symbolEffectsRemoved = true;
          classes.push("cmux-custom-sidebar-swift-symbol-effects-removed");
        }
        break;
      case "onSubmit":
      case "onChange":
        break;
      case "onAppear":
        hasOnAppear = true;
        break;
      case "onDisappear":
        hasOnDisappear = true;
        break;
      case "task":
        hasTask = true;
        taskId = modifier.value;
        taskIdExpression = modifier.secondaryValue;
        break;
      case "onHover":
        hasOnHover = true;
        classes.push("cmux-custom-sidebar-swift-hoverable");
        break;
      case "onGeometryChange":
        hasOnGeometryChange = true;
        onGeometryChangeType = modifier.value;
        classes.push("cmux-custom-sidebar-swift-on-geometry-change");
        if (modifier.value !== undefined) {
          classes.push(
            `cmux-custom-sidebar-swift-on-geometry-change-${swiftToken(
              modifier.value,
              "value",
            )}`,
          );
        }
        break;
    }
  }
  if (transforms.length > 0) {
    style.transform = transforms.join(" ");
  }
  if (filters.length > 0) {
    style.filter = filters.join(" ");
  }
  if (redacted) {
    classes.push("cmux-custom-sidebar-swift-redacted");
  }
  const className = (symbolEffectsRemoved === true
    ? classes.filter(
        (className) =>
          !className.startsWith("cmux-custom-sidebar-swift-symbol-effect-") ||
          className === "cmux-custom-sidebar-swift-symbol-effects-removed",
      )
    : classes
  ).join(" ");
  return {
    ariaHidden,
    ariaLabel,
    ariaValueText,
    accessibilityHint,
    accessibilityTraits,
    accessibilityElementChildren,
    accessibilityActionName,
    hasAccessibilityAction,
    accessibilityActivationPoint,
    accessibilityActivationPointX,
    accessibilityActivationPointY,
    accessibilitySortPriority,
    animationName,
    animationValue,
    transitionName,
    contentTransitionName,
    contentShape,
    safeAreaPaddingEdges,
    safeAreaPaddingLength,
    safeAreaPaddingInsets,
    contentMarginsEdges,
    contentMarginsLength,
    contentMarginsPlacement,
    contentMarginsInsets,
    clipShapeStyle,
    clipShapeAntialiased,
    coordinateSpace,
    alignmentGuide,
    alignmentGuideOffset,
    hasVisualEffect,
    containerRelativeFrameAxis,
    containerRelativeFrameCount,
    containerRelativeFrameSpan,
    containerRelativeFrameSpacing,
    containerRelativeFrameAlignment,
    gridCellAnchor,
    gridCellColumns,
    gridColumnAlignment,
    scrollTargetBehavior,
    scrollTargetLayout,
    scrollBounceBehavior,
    scrollBounceAxes,
    scrollDisabled,
    scrollPositionId,
    scrollPositionAnchor,
    scrollPositionBinding,
    defaultScrollAnchor,
    tabViewStyle,
    shapeStroke,
    shapeStrokeColor,
    shapeStrokeWidth,
    symbolEffectName,
    symbolEffectValue,
    symbolEffectActive,
    symbolEffectsRemoved,
    labelsHidden,
    controlGroupStyle,
    groupBoxStyle,
    dynamicTypeSize,
    preferredColorScheme,
    environmentColorScheme,
    environmentLayoutDirection,
    flipsForRightToLeftLayoutDirection,
    redacted,
    redactionReason,
    privacySensitive,
    unredacted,
    allowsHitTesting,
    hidden,
    badge,
    hoverEffect,
    hoverEffectEnabled,
    defaultHoverEffect,
    hasOnAppear,
    hasOnDisappear,
    hasTask,
    hasOnHover,
    hasOnGeometryChange,
    onGeometryChangeType,
    isFocusable,
    focusedStateBindingKey,
    focusedValue,
    taskId,
    taskIdExpression,
    className,
    disabled,
    draggableValue,
    dropDestinationType,
    identityValue,
    keyboardShortcut,
    toolbarBackground,
    toolbarBackgroundBars,
    toolbarBackgroundStyle,
    toolbarColorScheme,
    toolbarColorSchemeBars,
    navigationDisplayMode,
    navigationSubtitle,
    navigationTitle,
    style,
    title,
  };
}

function swiftAccessibilityProps(
  presentation: ReturnType<typeof swiftModifierPresentation>,
): {
  "aria-hidden"?: boolean;
  "aria-keyshortcuts"?: string;
  "aria-label"?: string;
  "aria-valuetext"?: string;
  "data-swift-accessibility-element-children"?: string;
  "data-swift-accessibility-action"?: string;
  "data-swift-accessibility-action-enabled"?: string;
  "data-swift-accessibility-activation-point"?: string;
  "data-swift-accessibility-activation-point-x"?: string;
  "data-swift-accessibility-activation-point-y"?: string;
  "data-swift-accessibility-hint"?: string;
  "data-swift-accessibility-sort-priority"?: string;
  "data-swift-accessibility-traits"?: string;
  "data-swift-animation"?: string;
  "data-swift-animation-value"?: string;
  "data-swift-alignment-guide"?: string;
  "data-swift-alignment-guide-offset"?: string;
  "data-swift-visual-effect"?: string;
  "data-swift-content-shape"?: string;
  "data-swift-coordinate-space"?: string;
  "data-swift-container-relative-frame-axis"?: string;
  "data-swift-container-relative-frame-count"?: string;
  "data-swift-container-relative-frame-span"?: string;
  "data-swift-container-relative-frame-spacing"?: string;
  "data-swift-container-relative-frame-alignment"?: string;
  "data-swift-content-transition"?: string;
  "data-swift-dynamic-type-size"?: string;
  "data-swift-environment-color-scheme"?: string;
  "data-swift-environment-layout-direction"?: string;
  "data-swift-safe-area-padding-edges"?: string;
  "data-swift-safe-area-padding-length"?: string;
  "data-swift-safe-area-padding-insets"?: string;
  "data-swift-content-margins-edges"?: string;
  "data-swift-content-margins-length"?: string;
  "data-swift-content-margins-placement"?: string;
  "data-swift-content-margins-insets"?: string;
  "data-swift-grid-cell-anchor"?: string;
  "data-swift-grid-cell-columns"?: string;
  "data-swift-grid-column-alignment"?: string;
  "data-swift-scroll-target-behavior"?: string;
  "data-swift-scroll-target-layout"?: string;
  "data-swift-scroll-bounce-behavior"?: string;
  "data-swift-scroll-bounce-axes"?: string;
  "data-swift-scroll-disabled"?: string;
  "data-swift-scroll-position-id"?: string;
  "data-swift-scroll-position-anchor"?: string;
  "data-swift-scroll-position-binding"?: string;
  "data-swift-default-scroll-anchor"?: string;
  "data-swift-tab-view-style"?: string;
  "data-swift-shape-stroke"?: string;
  "data-swift-shape-stroke-color"?: string;
  "data-swift-shape-stroke-width"?: string;
  "data-swift-labels-hidden"?: string;
  "data-swift-control-group-style"?: string;
  "data-swift-draggable"?: string;
  "data-swift-focusable"?: string;
  "data-swift-focused"?: string;
  "data-swift-focused-binding"?: string;
  "data-swift-id"?: string;
  "data-swift-preferred-color-scheme"?: string;
  "data-swift-drop-destination"?: string;
  "data-swift-symbol-effect"?: string;
  "data-swift-symbol-effect-active"?: boolean;
  "data-swift-symbol-effect-value"?: string;
  "data-swift-symbol-effects-removed"?: string;
  "data-swift-on-appear"?: string;
  "data-swift-on-disappear"?: string;
  "data-swift-on-hover"?: string;
  "data-swift-on-geometry-change"?: string;
  "data-swift-on-geometry-change-type"?: string;
  "data-swift-task"?: string;
  "data-swift-task-id"?: string;
  "data-swift-task-id-expression"?: string;
  "data-swift-transition"?: string;
  "data-swift-flips-for-rtl"?: string;
  "data-swift-clip-style"?: string;
  "data-swift-clip-antialiased"?: string;
  "data-swift-allows-hit-testing"?: string;
  "data-swift-hidden"?: string;
  "data-swift-badge"?: string;
  "data-swift-hover-effect"?: string;
  "data-swift-hover-effect-enabled"?: string;
  "data-swift-default-hover-effect"?: string;
  "data-swift-redacted"?: string;
  "data-swift-redaction-reason"?: string;
  "data-swift-privacy-sensitive"?: string;
  "data-swift-unredacted"?: string;
  draggable?: boolean;
} {
  return {
    ...(presentation.ariaHidden !== undefined ? { "aria-hidden": presentation.ariaHidden } : {}),
    ...(presentation.keyboardShortcut !== undefined
      ? { "aria-keyshortcuts": presentation.keyboardShortcut }
      : {}),
    ...(presentation.ariaLabel !== undefined ? { "aria-label": presentation.ariaLabel } : {}),
    ...(presentation.ariaValueText !== undefined
      ? { "aria-valuetext": presentation.ariaValueText }
      : {}),
    ...(presentation.accessibilityHint !== undefined
      ? { "data-swift-accessibility-hint": presentation.accessibilityHint }
      : {}),
    ...(presentation.accessibilityTraits !== undefined
      ? { "data-swift-accessibility-traits": presentation.accessibilityTraits }
      : {}),
    ...(presentation.accessibilityElementChildren !== undefined
      ? {
          "data-swift-accessibility-element-children":
            presentation.accessibilityElementChildren,
        }
      : {}),
    ...(presentation.hasAccessibilityAction
      ? { "data-swift-accessibility-action": presentation.accessibilityActionName ?? "default" }
      : {}),
    ...(presentation.hasAccessibilityAction
      ? { "data-swift-accessibility-action-enabled": "true" }
      : {}),
    ...(presentation.accessibilityActivationPoint !== undefined
      ? {
          "data-swift-accessibility-activation-point":
            presentation.accessibilityActivationPoint,
        }
      : {}),
    ...(presentation.accessibilityActivationPointX !== undefined
      ? {
          "data-swift-accessibility-activation-point-x":
            presentation.accessibilityActivationPointX,
        }
      : {}),
    ...(presentation.accessibilityActivationPointY !== undefined
      ? {
          "data-swift-accessibility-activation-point-y":
            presentation.accessibilityActivationPointY,
        }
      : {}),
    ...(presentation.accessibilitySortPriority !== undefined
      ? { "data-swift-accessibility-sort-priority": presentation.accessibilitySortPriority }
      : {}),
    ...(presentation.animationName !== undefined
      ? { "data-swift-animation": presentation.animationName }
      : {}),
    ...(presentation.animationValue !== undefined
      ? { "data-swift-animation-value": presentation.animationValue }
      : {}),
    ...(presentation.alignmentGuide !== undefined
      ? { "data-swift-alignment-guide": presentation.alignmentGuide }
      : {}),
    ...(presentation.alignmentGuideOffset !== undefined
      ? { "data-swift-alignment-guide-offset": presentation.alignmentGuideOffset }
      : {}),
    ...(presentation.hasVisualEffect ? { "data-swift-visual-effect": "true" } : {}),
    ...(presentation.transitionName !== undefined
      ? { "data-swift-transition": presentation.transitionName }
      : {}),
    ...(presentation.contentTransitionName !== undefined
      ? { "data-swift-content-transition": presentation.contentTransitionName }
      : {}),
    ...(presentation.contentShape !== undefined
      ? { "data-swift-content-shape": presentation.contentShape }
      : {}),
    ...(presentation.safeAreaPaddingEdges !== undefined
      ? { "data-swift-safe-area-padding-edges": presentation.safeAreaPaddingEdges }
      : {}),
    ...(presentation.safeAreaPaddingLength !== undefined
      ? { "data-swift-safe-area-padding-length": presentation.safeAreaPaddingLength }
      : {}),
    ...(presentation.safeAreaPaddingInsets !== undefined
      ? { "data-swift-safe-area-padding-insets": presentation.safeAreaPaddingInsets }
      : {}),
    ...(presentation.contentMarginsEdges !== undefined
      ? { "data-swift-content-margins-edges": presentation.contentMarginsEdges }
      : {}),
    ...(presentation.contentMarginsLength !== undefined
      ? { "data-swift-content-margins-length": presentation.contentMarginsLength }
      : {}),
    ...(presentation.contentMarginsPlacement !== undefined
      ? { "data-swift-content-margins-placement": presentation.contentMarginsPlacement }
      : {}),
    ...(presentation.contentMarginsInsets !== undefined
      ? { "data-swift-content-margins-insets": presentation.contentMarginsInsets }
      : {}),
    ...(presentation.clipShapeStyle !== undefined
      ? { "data-swift-clip-style": presentation.clipShapeStyle }
      : {}),
    ...(presentation.clipShapeAntialiased !== undefined
      ? { "data-swift-clip-antialiased": String(presentation.clipShapeAntialiased) }
      : {}),
    ...(presentation.allowsHitTesting !== undefined
      ? { "data-swift-allows-hit-testing": String(presentation.allowsHitTesting) }
      : {}),
    ...(presentation.hidden === true ? { "data-swift-hidden": "true" } : {}),
    ...(presentation.badge !== undefined ? { "data-swift-badge": presentation.badge } : {}),
    ...(presentation.hoverEffect !== undefined
      ? { "data-swift-hover-effect": presentation.hoverEffect }
      : {}),
    ...(presentation.hoverEffectEnabled !== undefined
      ? { "data-swift-hover-effect-enabled": String(presentation.hoverEffectEnabled) }
      : {}),
    ...(presentation.defaultHoverEffect !== undefined
      ? { "data-swift-default-hover-effect": presentation.defaultHoverEffect }
      : {}),
    ...(presentation.coordinateSpace !== undefined
      ? { "data-swift-coordinate-space": presentation.coordinateSpace }
      : {}),
    ...(presentation.containerRelativeFrameAxis !== undefined
      ? {
          "data-swift-container-relative-frame-axis":
            presentation.containerRelativeFrameAxis,
        }
      : {}),
    ...(presentation.containerRelativeFrameCount !== undefined
      ? {
          "data-swift-container-relative-frame-count":
            presentation.containerRelativeFrameCount,
        }
      : {}),
    ...(presentation.containerRelativeFrameSpan !== undefined
      ? {
          "data-swift-container-relative-frame-span":
            presentation.containerRelativeFrameSpan,
        }
      : {}),
    ...(presentation.containerRelativeFrameSpacing !== undefined
      ? {
          "data-swift-container-relative-frame-spacing":
            presentation.containerRelativeFrameSpacing,
        }
      : {}),
    ...(presentation.containerRelativeFrameAlignment !== undefined
      ? {
          "data-swift-container-relative-frame-alignment":
            presentation.containerRelativeFrameAlignment,
        }
      : {}),
    ...(presentation.dynamicTypeSize !== undefined
      ? { "data-swift-dynamic-type-size": presentation.dynamicTypeSize }
      : {}),
    ...(presentation.preferredColorScheme !== undefined
      ? { "data-swift-preferred-color-scheme": presentation.preferredColorScheme }
      : {}),
    ...(presentation.environmentColorScheme !== undefined
      ? { "data-swift-environment-color-scheme": presentation.environmentColorScheme }
      : {}),
    ...(presentation.environmentLayoutDirection !== undefined
      ? { "data-swift-environment-layout-direction": presentation.environmentLayoutDirection }
      : {}),
    ...(presentation.flipsForRightToLeftLayoutDirection !== undefined
      ? {
          "data-swift-flips-for-rtl": String(
            presentation.flipsForRightToLeftLayoutDirection,
          ),
        }
      : {}),
    ...(presentation.redacted ? { "data-swift-redacted": "true" } : {}),
    ...(presentation.redactionReason !== undefined
      ? { "data-swift-redaction-reason": presentation.redactionReason }
      : {}),
    ...(presentation.privacySensitive ? { "data-swift-privacy-sensitive": "true" } : {}),
    ...(presentation.unredacted ? { "data-swift-unredacted": "true" } : {}),
    ...(presentation.gridCellAnchor !== undefined
      ? { "data-swift-grid-cell-anchor": presentation.gridCellAnchor }
      : {}),
    ...(presentation.gridCellColumns !== undefined
      ? { "data-swift-grid-cell-columns": presentation.gridCellColumns }
      : {}),
    ...(presentation.gridColumnAlignment !== undefined
      ? { "data-swift-grid-column-alignment": presentation.gridColumnAlignment }
      : {}),
    ...(presentation.scrollTargetBehavior !== undefined
      ? { "data-swift-scroll-target-behavior": presentation.scrollTargetBehavior }
      : {}),
    ...(presentation.scrollTargetLayout !== undefined
      ? { "data-swift-scroll-target-layout": presentation.scrollTargetLayout }
      : {}),
    ...(presentation.scrollBounceBehavior !== undefined
      ? { "data-swift-scroll-bounce-behavior": presentation.scrollBounceBehavior }
      : {}),
    ...(presentation.scrollBounceAxes !== undefined
      ? { "data-swift-scroll-bounce-axes": presentation.scrollBounceAxes }
      : {}),
    ...(presentation.scrollDisabled !== undefined
      ? { "data-swift-scroll-disabled": String(presentation.scrollDisabled) }
      : {}),
    ...(presentation.scrollPositionId !== undefined
      ? { "data-swift-scroll-position-id": presentation.scrollPositionId }
      : {}),
    ...(presentation.scrollPositionAnchor !== undefined
      ? { "data-swift-scroll-position-anchor": presentation.scrollPositionAnchor }
      : {}),
    ...(presentation.scrollPositionBinding !== undefined
      ? { "data-swift-scroll-position-binding": presentation.scrollPositionBinding }
      : {}),
    ...(presentation.defaultScrollAnchor !== undefined
      ? { "data-swift-default-scroll-anchor": presentation.defaultScrollAnchor }
      : {}),
    ...(presentation.tabViewStyle !== undefined
      ? { "data-swift-tab-view-style": presentation.tabViewStyle }
      : {}),
    ...(presentation.shapeStroke !== undefined
      ? { "data-swift-shape-stroke": presentation.shapeStroke }
      : {}),
    ...(presentation.shapeStrokeColor !== undefined
      ? { "data-swift-shape-stroke-color": presentation.shapeStrokeColor }
      : {}),
    ...(presentation.shapeStrokeWidth !== undefined
      ? { "data-swift-shape-stroke-width": presentation.shapeStrokeWidth }
      : {}),
    ...(presentation.labelsHidden === true ? { "data-swift-labels-hidden": "true" } : {}),
    ...(presentation.controlGroupStyle !== undefined
      ? { "data-swift-control-group-style": presentation.controlGroupStyle }
      : {}),
    ...(presentation.groupBoxStyle !== undefined
      ? { "data-swift-group-box-style": presentation.groupBoxStyle }
      : {}),
    ...(presentation.isFocusable ? { "data-swift-focusable": "true" } : {}),
    ...(presentation.focusedValue !== undefined
      ? { "data-swift-focused": String(presentation.focusedValue) }
      : {}),
    ...(presentation.focusedStateBindingKey !== undefined
      ? { "data-swift-focused-binding": presentation.focusedStateBindingKey }
      : {}),
    ...(presentation.symbolEffectName !== undefined
      ? { "data-swift-symbol-effect": presentation.symbolEffectName }
      : {}),
    ...(presentation.symbolEffectValue !== undefined
      ? { "data-swift-symbol-effect-value": presentation.symbolEffectValue }
      : {}),
    ...(presentation.symbolEffectActive !== undefined
      ? { "data-swift-symbol-effect-active": presentation.symbolEffectActive }
      : {}),
    ...(presentation.symbolEffectsRemoved === true
      ? { "data-swift-symbol-effects-removed": "true" }
      : {}),
    ...(presentation.hasOnAppear ? { "data-swift-on-appear": "true" } : {}),
    ...(presentation.hasOnDisappear ? { "data-swift-on-disappear": "true" } : {}),
    ...(presentation.hasOnHover ? { "data-swift-on-hover": "true" } : {}),
    ...(presentation.hasOnGeometryChange
      ? { "data-swift-on-geometry-change": "true" }
      : {}),
    ...(presentation.onGeometryChangeType !== undefined
      ? { "data-swift-on-geometry-change-type": presentation.onGeometryChangeType }
      : {}),
    ...(presentation.hasTask ? { "data-swift-task": "true" } : {}),
    ...(presentation.taskId !== undefined ? { "data-swift-task-id": presentation.taskId } : {}),
    ...(presentation.taskIdExpression !== undefined
      ? { "data-swift-task-id-expression": presentation.taskIdExpression }
      : {}),
    ...(presentation.identityValue !== undefined ? { "data-swift-id": presentation.identityValue } : {}),
    ...(presentation.dropDestinationType !== undefined
      ? { "data-swift-drop-destination": presentation.dropDestinationType }
      : {}),
    ...(presentation.draggableValue !== undefined
      ? { draggable: true, "data-swift-draggable": presentation.draggableValue }
      : {}),
  };
}
