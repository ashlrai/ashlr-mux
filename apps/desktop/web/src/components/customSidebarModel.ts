import { createContext } from "react";

export interface TabPreview {
  id: string;
  title: string;
  directory?: string;
  branch?: string;
  dirty: boolean;
  ports: number[];
  focused: boolean;
}

export interface WorkspacePreview {
  id: string;
  index: number;
  title: string;
  directory?: string;
  tabs: TabPreview[];
  tabCount: number;
  unreadCount: number;
  ports: number[];
  branch?: string;
  dirty: boolean;
  progress?: number;
  statusCount: number;
  metadataCount: number;
  logCount: number;
  remoteState?: string;
  selected: boolean;
}

export interface CustomSidebarJsonDocument {
  title?: string;
  subtitle?: string;
  footer?: string;
  blocks?: CustomSidebarJsonBlock[];
}

export type SwiftSidebarStatePrimitive = string | number | boolean | null;
export type SwiftSidebarStateValue =
  | SwiftSidebarStatePrimitive
  | SwiftSidebarStateValue[]
  | { [key: string]: SwiftSidebarStateValue };

export interface SwiftMeasurementValue {
  __swiftMeasurement: true;
  value: number;
  unit: string;
}

export interface SwiftDateValue {
  __swiftDate: true;
  epochMs: number;
}

export interface SwiftDateIntervalValue {
  start: SwiftDateValue;
  end: SwiftDateValue;
}

export interface SwiftAngleValue {
  __swiftAngle: true;
  degrees: number;
}

export interface SwiftUnitPointValue {
  __swiftUnitPoint: true;
  x: number;
  y: number;
  token?: string;
}

export interface SwiftPointValue {
  __swiftPoint: true;
  x: number;
  y: number;
}

export interface SwiftSizeValue {
  __swiftSize: true;
  width: number;
  height: number;
}

export interface SwiftRectValue {
  __swiftRect: true;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SwiftGridItemValue {
  __swiftGridItem: true;
  size: "fixed" | "flexible" | "adaptive";
  minimum?: number;
  maximum?: number;
  spacing?: number;
  alignment?: string;
}

export function isSwiftGridItem(value: unknown): value is SwiftGridItemValue {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as SwiftGridItemValue).__swiftGridItem === true &&
    ((value as SwiftGridItemValue).size === "fixed" ||
      (value as SwiftGridItemValue).size === "flexible" ||
      (value as SwiftGridItemValue).size === "adaptive")
  );
}

export interface CustomSidebarSwiftStateAssignment {
  key: string;
  value: SwiftSidebarStateValue;
}

export interface CustomSidebarSwiftEventHandler {
  eventName?: string;
  eventCategory?: string;
  assignments: CustomSidebarSwiftStateAssignment[];
  action?: CustomSidebarJsonAction;
}

export interface CustomSidebarSwiftLocalHandler {
  assignments: CustomSidebarSwiftStateAssignment[];
  action?: CustomSidebarJsonAction;
}

export type CustomSidebarSwiftLocalHandlerModifierName =
  | "onSubmit"
  | "onChange"
  | "onAppear"
  | "onDisappear"
  | "task"
  | "onHover";

export interface CustomSidebarSwiftModifier {
  name:
    | "font"
    | "fontWeight"
    | "fontDesign"
    | "fontWidth"
    | "dynamicTypeSize"
    | "bold"
    | "italic"
    | "monospaced"
    | "monospacedDigit"
    | "foregroundColor"
    | "padding"
    | "safeAreaPadding"
    | "contentMargins"
    | "gridCellColumns"
    | "gridColumnAlignment"
    | "gridCellAnchor"
    | "background"
    | "overlay"
    | "mask"
    | "safeAreaInset"
    | "contextMenu"
    | "refreshable"
    | "swipeActions"
    | "sheet"
    | "popover"
    | "fullScreenCover"
    | "alert"
    | "confirmationDialog"
    | "presentationDetents"
    | "presentationDragIndicator"
    | "presentationBackground"
    | "presentationCornerRadius"
    | "toolbar"
    | "toolbarBackground"
    | "toolbarColorScheme"
    | "searchable"
    | "navigationTitle"
    | "navigationSubtitle"
    | "navigationBarTitleDisplayMode"
    | "navigationDestination"
    | "tag"
    | "tabItem"
    | "id"
    | "keyboardShortcut"
    | "contentShape"
    | "coordinateSpace"
    | "draggable"
    | "dropDestination"
    | "focusable"
    | "focused"
    | "controlSize"
    | "buttonBorderShape"
    | "cornerRadius"
    | "containerRelativeFrame"
    | "frame"
    | "alignmentGuide"
    | "layoutPriority"
    | "offset"
    | "position"
    | "zIndex"
    | "aspectRatio"
    | "clipped"
    | "compositingGroup"
    | "clipShape"
    | "shadow"
    | "border"
    | "strokeBorder"
    | "blur"
    | "brightness"
    | "contrast"
    | "saturation"
    | "grayscale"
    | "hueRotation"
    | "blendMode"
    | "rotationEffect"
    | "scaleEffect"
    | "rotation3DEffect"
    | "visualEffect"
    | "lineLimit"
    | "truncationMode"
    | "multilineTextAlignment"
    | "textCase"
    | "tracking"
    | "kerning"
    | "baselineOffset"
    | "underline"
    | "strikethrough"
    | "opacity"
    | "hidden"
    | "fixedSize"
    | "badge"
    | "allowsHitTesting"
    | "disabled"
    | "hoverEffect"
    | "defaultHoverEffect"
    | "help"
    | "accessibilityLabel"
    | "accessibilityHidden"
    | "accessibilityValue"
    | "accessibilityHint"
    | "accessibilityAddTraits"
    | "accessibilityElement"
    | "accessibilityAction"
    | "accessibilityActivationPoint"
    | "accessibilityRepresentation"
    | "accessibilitySortPriority"
    | "redacted"
    | "privacySensitive"
    | "unredacted"
    | "trim"
    | "listRowBackground"
    | "listRowSeparator"
    | "labelsHidden"
    | "labelStyle"
    | "listStyle"
    | "menuStyle"
    | "controlGroupStyle"
    | "groupBoxStyle"
    | "pickerStyle"
    | "tabViewStyle"
    | "toggleStyle"
    | "textFieldStyle"
    | "scrollContentBackground"
    | "scrollIndicators"
    | "scrollClipDisabled"
    | "scrollTargetBehavior"
    | "scrollTargetLayout"
    | "scrollBounceBehavior"
    | "scrollDisabled"
    | "scrollPosition"
    | "defaultScrollAnchor"
    | "preferredColorScheme"
    | "environment"
    | "resizable"
    | "renderingMode"
    | "interpolation"
    | "antialiased"
    | "flipsForRightToLeftLayoutDirection"
    | "imageScale"
    | "symbolRenderingMode"
    | "symbolVariant"
    | "onTapGesture"
    | "onLongPressGesture"
    | "onHover"
    | "onEvent"
    | "onSubmit"
    | "onChange"
    | "onGeometryChange"
    | "onAppear"
    | "onDisappear"
    | "task"
    | "animation"
    | "transition"
    | "contentTransition"
    | "symbolEffect"
    | "symbolEffectsRemoved"
    | "buttonStyle";
  value?: string;
  secondaryValue?: string;
  x?: number;
  y?: number;
  z?: number;
  width?: number;
  radius?: number;
  perspective?: number;
  fillStyle?: string;
  antialiased?: boolean;
  action?: CustomSidebarJsonAction;
  children?: CustomSidebarSwiftNode[];
  count?: number;
  span?: number;
  spacing?: number;
  edge?: string;
  placement?: string;
  frameWidth?: number;
  frameHeight?: number;
  frameMinWidth?: number;
  frameMinHeight?: number;
  frameMaxWidth?: number;
  frameMaxHeight?: number;
  frameIdealWidth?: number;
  frameIdealHeight?: number;
  frameAlignment?: string;
  paddingTop?: number;
  paddingLeading?: number;
  paddingBottom?: number;
  paddingTrailing?: number;
  capInsetTop?: number;
  capInsetLeading?: number;
  capInsetBottom?: number;
  capInsetTrailing?: number;
  maxWidthInfinity?: boolean;
  boolValue?: boolean;
  stateBindingKey?: string;
  eventHandler?: CustomSidebarSwiftEventHandler;
  localHandler?: CustomSidebarSwiftLocalHandler;
  falseLocalHandler?: CustomSidebarSwiftLocalHandler;
  itemParam?: string;
  itemValue?: SwiftSidebarStateValue;
  tagValue?: SwiftSidebarStateValue;
  presentationBindingKind?: "isPresented" | "item";
  routeParam?: string;
  routeBody?: string;
  routeValueType?: string;
}

export type CustomSidebarSwiftTextRun =
  | { kind: "text"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "emphasis"; text: string }
  | { kind: "code"; text: string }
  | { kind: "link"; text: string; href: string };

export interface CustomSidebarSwiftPickerOption {
  label: string;
  value: SwiftSidebarStateValue;
  encodedValue: string;
}

export type CustomSidebarSwiftNode = (
  | {
      kind: "vstack" | "hstack" | "zstack" | "group";
      children: CustomSidebarSwiftNode[];
      spacing?: number;
      alignment?: string;
      lazyStack?: boolean;
      pinnedViews?: string;
      groupRole?: "group" | "controlGroup" | "viewThatFits" | "toolbarItem";
      fitAxis?: "vertical" | "horizontal" | "both";
      toolbarPlacement?: string;
    }
  | {
      kind: "splitView";
      axis: "horizontal" | "vertical";
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "navigationStack";
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "navigationLink";
      title: string;
      children: CustomSidebarSwiftNode[];
      destination: CustomSidebarSwiftNode[];
      value?: string;
    }
  | {
      kind: "externalLink";
      title?: string;
      href?: string;
      labelChildren: CustomSidebarSwiftNode[];
    }
  | {
      kind: "contentUnavailable";
      title?: string;
      systemImage?: string;
      labelChildren: CustomSidebarSwiftNode[];
      descriptionChildren: CustomSidebarSwiftNode[];
      actionsChildren: CustomSidebarSwiftNode[];
    }
  | {
      kind: "tabView";
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "text";
      text: string;
      markdownRuns?: CustomSidebarSwiftTextRun[];
      textStyle?: string;
      timerIntervalStartMs?: number;
      timerIntervalEndMs?: number;
      timerCountsDown?: boolean;
    }
  | { kind: "image"; systemName: string }
  | { kind: "assetImage"; name: string; url?: string; decorative?: boolean }
  | {
      kind: "asyncImage";
      url?: string;
      successChildren?: CustomSidebarSwiftNode[];
      placeholderChildren?: CustomSidebarSwiftNode[];
    }
  | { kind: "label"; text: string; systemImage?: string }
  | {
      kind: "labeledContent";
      title?: string;
      value?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "groupBox";
      title?: string;
      labelChildren: CustomSidebarSwiftNode[];
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "disclosureGroup";
      title?: string;
      isExpanded: boolean;
      stateBindingKey?: string;
      labelChildren: CustomSidebarSwiftNode[];
      children: CustomSidebarSwiftNode[];
    }
  | { kind: "progress"; value?: number; total?: number }
  | {
      kind: "textField";
      placeholder?: string;
      text: string;
      secure?: boolean;
      multiline?: boolean;
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "stepper";
      title?: string;
      value?: number;
      lowerBound: number;
      upperBound: number;
      step: number;
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "slider";
      value?: number;
      lowerBound: number;
      upperBound: number;
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "picker";
      title?: string;
      selection: string;
      selectedValue?: SwiftSidebarStateValue;
      options?: CustomSidebarSwiftPickerOption[];
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "datePicker";
      title?: string;
      value: string;
      displayedComponents?: string;
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "colorPicker";
      title?: string;
      value: string;
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "toggle";
      text?: string;
      isOn: boolean;
      stateBindingKey?: string;
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "scrollView";
      axis: "vertical" | "horizontal" | "both";
      showsIndicators: boolean;
      children: CustomSidebarSwiftNode[];
    }
  | { kind: "list"; dataId?: string; form?: boolean; children: CustomSidebarSwiftNode[] }
  | {
      kind: "section";
      title?: string;
      header?: CustomSidebarSwiftNode[];
      footer?: CustomSidebarSwiftNode[];
      children: CustomSidebarSwiftNode[];
    }
  | {
      kind: "grid";
      gridKind: "grid" | "lazyVGrid" | "lazyHGrid";
      gridItems?: SwiftGridItemValue[];
      spacing?: number;
      alignment?: string;
      pinnedViews?: string;
      children: CustomSidebarSwiftNode[];
    }
  | { kind: "gridRow"; children: CustomSidebarSwiftNode[] }
  | { kind: "menu"; title?: string; children: CustomSidebarSwiftNode[] }
  | {
      kind: "shape";
      shape:
        | "circle"
        | "rectangle"
        | "roundedRectangle"
        | "unevenRoundedRectangle"
        | "capsule"
        | "ellipse"
        | "containerRelativeShape"
        | "pathRoundedRect"
        | "pathEllipse";
      radius?: number;
      cornerStyle?: string;
      pathX?: number;
      pathY?: number;
      pathWidth?: number;
      pathHeight?: number;
    }
  | { kind: "divider" }
  | { kind: "spacer"; minLength?: number }
  | { kind: "empty" }
  | {
      kind: "modified";
      base: CustomSidebarSwiftNode;
      childModifiers: CustomSidebarSwiftModifier[];
    }
  | {
      kind: "button";
      text?: string;
      role?: "destructive" | "cancel";
      children: CustomSidebarSwiftNode[];
      action?: CustomSidebarJsonAction;
      localAction?: "dismissPresentation";
      actionTrigger?: "tap" | "longPress";
      tapCount?: number;
    }
) & { modifiers?: CustomSidebarSwiftModifier[]; eventHandlers?: CustomSidebarSwiftEventHandler[] };

export interface CustomSidebarSwiftDocument {
  root: CustomSidebarSwiftNode;
  warnings: string[];
  eventHandlers: CustomSidebarSwiftEventHandler[];
}

export interface SwiftSidebarStateContextValue {
  values: Record<string, SwiftSidebarStateValue>;
  setValue: (key: string, value: SwiftSidebarStateValue) => void;
  runLocalHandler: (handler: CustomSidebarSwiftLocalHandler) => void;
}

export const SwiftSidebarStateContext = createContext<SwiftSidebarStateContextValue>({
  values: {},
  setValue: () => {},
  runLocalHandler: () => {},
});

export interface SwiftNavigationEntry {
  title: string;
  destination: CustomSidebarSwiftNode[];
}

export interface SwiftNavigationContextValue {
  push: (entry: SwiftNavigationEntry) => void;
}

export const SwiftNavigationContext = createContext<SwiftNavigationContextValue | null>(null);

export interface SwiftPresentationContextValue {
  dismiss: () => void;
}

export const SwiftPresentationContext = createContext<SwiftPresentationContextValue | null>(null);

export interface CustomSidebarReloadEvent {
  all?: boolean;
  name?: string | null;
  paths?: string[];
  sidebars?: Array<{
    name?: string;
    path?: string;
  }>;
}

export interface CustomSidebarEventFrame {
  category?: string | null;
  name?: string | null;
  seq?: number | null;
  [key: string]: unknown;
}

export interface CustomSidebarEventsContext {
  latest: CustomSidebarEventFrame | null;
  recent: CustomSidebarEventFrame[];
  category_counts: Record<string, number>;
  name_counts: Record<string, number>;
  latest_seq: number;
  oldest_seq: number;
  next_seq: number;
  retained_count: number;
  boot_id: string;
}

export type WorkspaceListFilter = "all" | "selected" | "unread" | "ports" | "dirty" | "remote";

export interface CustomSidebarJsonAction {
  method: string;
  params?: Record<string, unknown>;
}

export type CustomSidebarJsonBlock =
  | { type: "heading"; text?: string }
  | { type: "text"; text?: string }
  | { type: "divider" }
  | { type: "stat"; label?: string; value?: string }
  | {
      type: "button";
      label?: string;
      detail?: string;
      action?: CustomSidebarJsonAction;
    }
  | {
      type: "workspaceList";
      title?: string;
      limit?: number;
      filter?: WorkspaceListFilter;
      action?: "workspace.select" | "none" | CustomSidebarJsonAction;
    }
  | {
      type: "selectedTabs";
      title?: string;
      limit?: number;
      action?: "surface.focus" | "none" | CustomSidebarJsonAction;
    };

export interface JsonTemplateContext {
  sourceName: string;
  workspaceCount: number;
  selectedTitle: string;
  selectedId: string;
  unreadTotal: number;
  portTotal: number;
  events?: CustomSidebarEventsContext;
  assets?: CustomSidebarAssetMap;
  latestEventName?: string;
  latestEventCategory?: string;
  latestEventSeq?: number;
}

export type CustomSidebarAssetMap = Record<string, string>;

export type TemplateContext = JsonTemplateContext & {
  workspace?: WorkspacePreview;
  tab?: TabPreview;
};

export function customSidebarSwiftEventHandlerMatches(
  handler: CustomSidebarSwiftEventHandler,
  event: CustomSidebarEventFrame | null,
): boolean {
  if (event === null) return false;
  if (handler.eventName !== undefined && event.name !== handler.eventName) {
    return false;
  }
  if (
    handler.eventCategory !== undefined &&
    event.category !== handler.eventCategory
  ) {
    return false;
  }
  return handler.eventName !== undefined || handler.eventCategory !== undefined;
}

export function customSidebarSourceName(sourcePath?: string): string {
  const trimmed = sourcePath?.trim();
  if (!trimmed) {
    return "Unsaved custom sidebar";
  }
  return trimmed.split(/[\\/]/).filter(Boolean).pop() ?? trimmed;
}

export function customSidebarSourceKind(sourcePath?: string): "json" | "swift" | "custom" {
  const lower = customSidebarSourceName(sourcePath).toLowerCase();
  if (lower.endsWith(".json")) return "json";
  if (lower.endsWith(".swift")) return "swift";
  return "custom";
}

export function customSidebarSourceStem(sourcePath?: string): string {
  return customSidebarSourceName(sourcePath).replace(/\.[^/.\\]+$/, "");
}

export function normalizedCustomSidebarPath(sourcePath?: string): string {
  return sourcePath?.trim().replace(/\\/g, "/").toLowerCase() ?? "";
}

export function customSidebarReloadMatches(
  event: CustomSidebarReloadEvent,
  sourcePath?: string,
): boolean {
  if (!sourcePath?.trim()) {
    return false;
  }
  if (event.all === true) {
    return true;
  }

  const sourceName = customSidebarSourceStem(sourcePath);
  if (event.name?.trim() === sourceName) {
    return true;
  }

  const normalizedSourcePath = normalizedCustomSidebarPath(sourcePath);
  if (
    event.paths?.some(
      (path) => normalizedCustomSidebarPath(path) === normalizedSourcePath,
    ) === true
  ) {
    return true;
  }

  return (
    event.sidebars?.some(
      (sidebar) =>
        sidebar.name === sourceName ||
        normalizedCustomSidebarPath(sidebar.path) === normalizedSourcePath,
    ) === true
  );
}

export function emptyCustomSidebarEventsContext(): CustomSidebarEventsContext {
  return {
    latest: null,
    recent: [],
    category_counts: {},
    name_counts: {},
    latest_seq: 0,
    oldest_seq: 1,
    next_seq: 1,
    retained_count: 0,
    boot_id: "",
  };
}

export function nextCustomSidebarEventsContext(
  current: CustomSidebarEventsContext,
  event: CustomSidebarEventFrame,
): CustomSidebarEventsContext {
  const seq =
    typeof event.seq === "number" && Number.isFinite(event.seq)
      ? event.seq
      : current.latest_seq + 1;
  const nextRecent = [...current.recent, event].slice(-50);
  const categoryCounts = { ...current.category_counts };
  const category = typeof event.category === "string" ? event.category : undefined;
  if (category) {
    categoryCounts[category] = (categoryCounts[category] ?? 0) + 1;
  }
  const nameCounts = { ...current.name_counts };
  const name = typeof event.name === "string" ? event.name : undefined;
  if (name) {
    nameCounts[name] = (nameCounts[name] ?? 0) + 1;
  }
  const bootId =
    typeof event.boot_id === "string" && event.boot_id.trim() !== ""
      ? event.boot_id
      : current.boot_id;
  return {
    latest: event,
    recent: nextRecent,
    category_counts: categoryCounts,
    name_counts: nameCounts,
    latest_seq: Math.max(current.latest_seq, seq),
    oldest_seq: current.retained_count === 0 ? seq : current.oldest_seq,
    next_seq: Math.max(current.next_seq, seq + 1),
    retained_count: current.retained_count + 1,
    boot_id: bootId,
  };
}

export function customSidebarEventsContextFromSnapshot(
  value: unknown,
): CustomSidebarEventsContext {
  const empty = emptyCustomSidebarEventsContext();
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return empty;
  }
  const record = value as Record<string, unknown>;
  const recent = Array.isArray(record.recent)
    ? record.recent.filter(isCustomSidebarEventFrame)
    : [];
  const latest = isCustomSidebarEventFrame(record.latest)
    ? record.latest
    : recent.at(-1) ?? null;
  const latestSeq = numberField(record.latest_seq, latest?.seq ?? empty.latest_seq);
  return {
    latest,
    recent,
    category_counts: numberRecordField(record.category_counts),
    name_counts: numberRecordField(record.name_counts),
    latest_seq: latestSeq,
    oldest_seq: numberField(record.oldest_seq, recent[0]?.seq ?? empty.oldest_seq),
    next_seq: numberField(record.next_seq, latestSeq + 1),
    retained_count: numberField(record.retained_count, recent.length),
    boot_id: typeof record.boot_id === "string" ? record.boot_id : empty.boot_id,
  };
}

export function customSidebarAssetMapFromSnapshot(value: unknown): CustomSidebarAssetMap {
  if (typeof value !== "object" || value === null) {
    return {};
  }
  if (Array.isArray(value)) {
    return Object.fromEntries(
      value
        .map((entry): [string, string] | null => {
          if (typeof entry !== "object" || entry === null || Array.isArray(entry)) {
            return null;
          }
          const record = entry as Record<string, unknown>;
          const name = typeof record.name === "string" ? record.name.trim() : "";
          const url =
            typeof record.url === "string"
              ? record.url
              : typeof record.src === "string"
                ? record.src
                : typeof record.href === "string"
                  ? record.href
                  : "";
          const safeUrl = safeCustomSidebarAssetUrl(url);
          return name && safeUrl !== undefined ? [name, safeUrl] : null;
        })
        .filter((entry): entry is [string, string] => entry !== null),
    );
  }
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .map((entry): [string, string] | null => {
        if (typeof entry[1] !== "string") return null;
        const safeUrl = safeCustomSidebarAssetUrl(entry[1]);
        return safeUrl === undefined ? null : [entry[0], safeUrl];
      })
      .filter((entry): entry is [string, string] => entry !== null),
  );
}

export function safeCustomSidebarAssetUrl(url: string): string | undefined {
  const trimmed = url.trim();
  return /^(https?:\/\/|cmux-sidebar-asset:\/\/)/i.test(trimmed) ? trimmed : undefined;
}

export function isCustomSidebarEventFrame(value: unknown): value is CustomSidebarEventFrame {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function numberField(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function numberRecordField(value: unknown): Record<string, number> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return {};
  }
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).filter(
      (entry): entry is [string, number] =>
        typeof entry[1] === "number" && Number.isFinite(entry[1]),
    ),
  );
}

export function parseSwiftSidebarStatePath(path: string): Array<string | number> | null {
  const first = path.match(/^([A-Za-z_][A-Za-z0-9_]*)/);
  if (!first) return null;
  const parts: Array<string | number> = [first[1] ?? ""];
  let index = first[0].length;
  while (index < path.length) {
    const char = path[index];
    if (char === ".") {
      const member = path.slice(index + 1).match(/^([A-Za-z_][A-Za-z0-9_]*)/);
      if (!member) return null;
      parts.push(member[1] ?? "");
      index += 1 + member[0].length;
      continue;
    }
    if (char === "[") {
      const close = path.indexOf("]", index + 1);
      if (close < 0) return null;
      const raw = path.slice(index + 1, close).trim();
      if (/^\d+$/.test(raw)) {
        parts.push(Number(raw));
      } else if (raw.startsWith('"') && raw.endsWith('"')) {
        try {
          parts.push(JSON.parse(raw) as string);
        } catch {
          return null;
        }
      } else {
        return null;
      }
      index = close + 1;
      continue;
    }
    if (/\s/.test(char ?? "")) {
      index += 1;
      continue;
    }
    return null;
  }
  return parts;
}

export function setSwiftSidebarNestedStateValue(
  current: SwiftSidebarStateValue | undefined,
  path: Array<string | number>,
  value: SwiftSidebarStateValue,
): SwiftSidebarStateValue {
  const [head, ...tail] = path;
  if (head === undefined) return value;
  if (typeof head === "number") {
    const next = Array.isArray(current) ? current.slice() : [];
    next[head] = setSwiftSidebarNestedStateValue(next[head], tail, value);
    return next;
  }
  const source =
    typeof current === "object" && current !== null && !Array.isArray(current)
      ? current
      : {};
  return {
    ...source,
    [head]: setSwiftSidebarNestedStateValue(source[head], tail, value),
  };
}

export function setSwiftSidebarStateValue(
  current: Record<string, SwiftSidebarStateValue>,
  key: string,
  value: SwiftSidebarStateValue,
): Record<string, SwiftSidebarStateValue> {
  const path = parseSwiftSidebarStatePath(key);
  if (path === null || path.length <= 1) {
    return { ...current, [key]: value };
  }
  const [root, ...tail] = path;
  if (typeof root !== "string") return { ...current, [key]: value };
  return {
    ...current,
    [root]: setSwiftSidebarNestedStateValue(current[root], tail, value),
  };
}

export function parseCustomSidebarJson(source: string): CustomSidebarJsonDocument {
  const parsed = JSON.parse(source) as unknown;
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error("Custom sidebar JSON must be an object.");
  }
  const document = parsed as CustomSidebarJsonDocument;
  if (document.blocks !== undefined && !Array.isArray(document.blocks)) {
    throw new Error("Custom sidebar JSON field 'blocks' must be an array.");
  }
  return document;
}
