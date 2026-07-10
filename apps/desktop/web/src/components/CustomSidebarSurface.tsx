import {
  createContext,
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

const CUSTOM_SIDEBAR_RELOAD_EVENT = "cmux://custom-sidebar-reload";
const CONTROL_EVENTS_CHANGED_EVENT = "cmux://events-changed";

export interface CustomSidebarSurfaceProps {
  sourcePath?: string;
  sourceOverride?: string;
  assetOverride?: CustomSidebarAssetMap;
}

interface TabPreview {
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

type SwiftSidebarStatePrimitive = string | number | boolean | null;
type SwiftSidebarStateValue =
  | SwiftSidebarStatePrimitive
  | SwiftSidebarStateValue[]
  | { [key: string]: SwiftSidebarStateValue };

interface SwiftMeasurementValue {
  __swiftMeasurement: true;
  value: number;
  unit: string;
}

interface SwiftDateValue {
  __swiftDate: true;
  epochMs: number;
}

interface SwiftDateIntervalValue {
  start: SwiftDateValue;
  end: SwiftDateValue;
}

interface SwiftAngleValue {
  __swiftAngle: true;
  degrees: number;
}

interface SwiftUnitPointValue {
  __swiftUnitPoint: true;
  x: number;
  y: number;
  token?: string;
}

interface SwiftPointValue {
  __swiftPoint: true;
  x: number;
  y: number;
}

interface SwiftSizeValue {
  __swiftSize: true;
  width: number;
  height: number;
}

interface SwiftRectValue {
  __swiftRect: true;
  x: number;
  y: number;
  width: number;
  height: number;
}

interface SwiftGridItemValue {
  __swiftGridItem: true;
  size: "fixed" | "flexible" | "adaptive";
  minimum?: number;
  maximum?: number;
  spacing?: number;
  alignment?: string;
}

function isSwiftGridItem(value: unknown): value is SwiftGridItemValue {
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

type CustomSidebarSwiftLocalHandlerModifierName =
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

interface SwiftSidebarStateContextValue {
  values: Record<string, SwiftSidebarStateValue>;
  setValue: (key: string, value: SwiftSidebarStateValue) => void;
  runLocalHandler: (handler: CustomSidebarSwiftLocalHandler) => void;
}

const SwiftSidebarStateContext = createContext<SwiftSidebarStateContextValue>({
  values: {},
  setValue: () => {},
  runLocalHandler: () => {},
});

interface SwiftNavigationEntry {
  title: string;
  destination: CustomSidebarSwiftNode[];
}

interface SwiftNavigationContextValue {
  push: (entry: SwiftNavigationEntry) => void;
}

const SwiftNavigationContext = createContext<SwiftNavigationContextValue | null>(null);

interface SwiftPresentationContextValue {
  dismiss: () => void;
}

const SwiftPresentationContext = createContext<SwiftPresentationContextValue | null>(null);

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

type WorkspaceListFilter = "all" | "selected" | "unread" | "ports" | "dirty" | "remote";

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

type TemplateContext = JsonTemplateContext & {
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

function customSidebarSourceStem(sourcePath?: string): string {
  return customSidebarSourceName(sourcePath).replace(/\.[^/.\\]+$/, "");
}

function normalizedCustomSidebarPath(sourcePath?: string): string {
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

function safeCustomSidebarAssetUrl(url: string): string | undefined {
  const trimmed = url.trim();
  return /^(https?:\/\/|cmux-sidebar-asset:\/\/)/i.test(trimmed) ? trimmed : undefined;
}

function isCustomSidebarEventFrame(value: unknown): value is CustomSidebarEventFrame {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function numberField(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function numberRecordField(value: unknown): Record<string, number> {
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

function parseSwiftSidebarStatePath(path: string): Array<string | number> | null {
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

function setSwiftSidebarNestedStateValue(
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

function setSwiftSidebarStateValue(
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

interface SwiftParseScope {
  root: JsonTemplateContext & { clock: { time: string }; workspaces: WorkspacePreview[] };
  workspace?: WorkspacePreview;
  tab?: TabPreview;
  __functions?: Record<string, SwiftUserFunction>;
  __stateValues?: Record<string, SwiftSidebarStateValue>;
  __stateKeys?: Record<string, true>;
  __bindingKeys?: Record<string, string>;
  [key: string]: unknown;
}

interface SwiftUserFunction {
  params: string[];
  body: string;
  returnsView: boolean;
}

class SwiftSidebarParser {
  readonly warnings: string[] = [];

  constructor(
    private readonly source: string,
    private readonly scope: SwiftParseScope,
  ) {}

  parse(): CustomSidebarSwiftDocument {
    const nodes = this.parseStatements(this.source, this.scope);
    const root: CustomSidebarSwiftNode =
      nodes.length === 1
        ? nodes[0] ?? { kind: "group", children: [] }
        : { kind: "group", children: nodes };
    const eventHandlers = this.collectEventHandlers(root);
    if (nodes.length === 1) {
      return { root, warnings: this.warnings, eventHandlers };
    }
    return { root, warnings: this.warnings, eventHandlers };
  }

  private collectEventHandlers(node: CustomSidebarSwiftNode): CustomSidebarSwiftEventHandler[] {
    const handlers = [...(node.eventHandlers ?? [])];
    switch (node.kind) {
      case "modified":
        handlers.push(...this.collectEventHandlers(node.base));
        for (const modifier of node.childModifiers) {
          for (const child of modifier.children ?? []) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        break;
      case "vstack":
      case "hstack":
      case "zstack":
      case "group":
      case "splitView":
      case "navigationStack":
      case "navigationLink":
      case "tabView":
      case "scrollView":
      case "list":
      case "section":
      case "groupBox":
      case "disclosureGroup":
      case "grid":
      case "gridRow":
      case "menu":
      case "button":
      case "textField":
      case "stepper":
      case "slider":
      case "picker":
      case "datePicker":
      case "colorPicker":
      case "toggle":
        for (const child of node.children) {
          handlers.push(...this.collectEventHandlers(child));
        }
        if (node.kind === "navigationLink") {
          for (const child of node.destination) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        if (node.kind === "section") {
          for (const child of node.header ?? []) {
            handlers.push(...this.collectEventHandlers(child));
          }
          for (const child of node.footer ?? []) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        if (node.kind === "groupBox" || node.kind === "disclosureGroup") {
          for (const child of node.labelChildren) {
            handlers.push(...this.collectEventHandlers(child));
          }
        }
        break;
      case "externalLink":
        for (const child of node.labelChildren) {
          handlers.push(...this.collectEventHandlers(child));
        }
        break;
      case "contentUnavailable":
        for (const child of [
          ...node.labelChildren,
          ...node.descriptionChildren,
          ...node.actionsChildren,
        ]) {
          handlers.push(...this.collectEventHandlers(child));
        }
        break;
      case "asyncImage":
        for (const child of node.successChildren ?? []) {
          handlers.push(...this.collectEventHandlers(child));
        }
        for (const child of node.placeholderChildren ?? []) {
          handlers.push(...this.collectEventHandlers(child));
        }
        break;
      default:
        break;
    }
    return handlers;
  }

  private parseViewExpression(
    expression: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode | null {
    const text = expression.trim();
    if (!text) return null;
    const textConcatenation = this.parseTextConcatenation(text, scope);
    if (textConcatenation !== null) return textConcatenation;
    const call = this.readViewCall(text);
    if (call === null) {
      this.warnings.push(`Unsupported Swift expression: ${text.slice(0, 40)}`);
      return null;
    }
    const asyncTrailing =
      call.name === "AsyncImage" ||
      call.name === "GroupBox" ||
      call.name === "DisclosureGroup" ||
      call.name === "ContentUnavailableView"
        ? this.readTrailingClosureSequence(text.slice(call.end))
        : null;
    const trailing =
      asyncTrailing?.end === undefined || asyncTrailing.end === 0
        ? this.readTrailingClosure(text.slice(call.end))
        : {
            body: asyncTrailing.first ?? "",
            end: asyncTrailing.end,
          };
    const decorate = (node: CustomSidebarSwiftNode): CustomSidebarSwiftNode =>
      this.withModifiers(node, text, call.end + (trailing?.end ?? 0), scope);
    switch (call.name) {
      case "VStack":
      case "LazyVStack":
        return decorate({
          kind: "vstack",
          alignment: this.swiftAlignmentToken(this.namedArg(call.args, "alignment"), scope),
          spacing: this.stackSpacing(call.args, scope),
          ...(call.name === "LazyVStack" ? { lazyStack: true } : {}),
          ...(call.name === "LazyVStack"
            ? { pinnedViews: this.swiftPinnedViews(this.namedArg(call.args, "pinnedViews"), scope) }
            : {}),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "HStack":
      case "LazyHStack":
        return decorate({
          kind: "hstack",
          alignment: this.swiftAlignmentToken(this.namedArg(call.args, "alignment"), scope),
          spacing: this.stackSpacing(call.args, scope),
          ...(call.name === "LazyHStack" ? { lazyStack: true } : {}),
          ...(call.name === "LazyHStack"
            ? { pinnedViews: this.swiftPinnedViews(this.namedArg(call.args, "pinnedViews"), scope) }
            : {}),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "HSplitView":
        return decorate({
          kind: "splitView",
          axis: "horizontal",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "VSplitView":
        return decorate({
          kind: "splitView",
          axis: "vertical",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "NavigationStack":
        return decorate({
          kind: "navigationStack",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "NavigationLink":
        return decorate(this.parseNavigationLink(call.args, trailing?.body ?? "", scope));
      case "Link":
        return decorate(this.parseExternalLink(call.args, trailing?.body ?? "", scope));
      case "ContentUnavailableView":
        return decorate(this.parseContentUnavailableView(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "TabView":
        return decorate({
          kind: "tabView",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ZStack":
        return decorate({
          kind: "zstack",
          alignment: this.swiftAlignmentToken(this.namedArg(call.args, "alignment"), scope),
          spacing: this.stackSpacing(call.args, scope),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Group":
        return decorate({
          kind: "group",
          groupRole: "group",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ToolbarItem":
        return decorate({
          kind: "group",
          groupRole: "toolbarItem",
          toolbarPlacement: this.swiftTypeToken(this.namedArg(call.args, "placement")),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ControlGroup":
        return decorate({
          kind: "group",
          groupRole: "controlGroup",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "ViewThatFits":
        return decorate({
          kind: "group",
          groupRole: "viewThatFits",
          fitAxis: this.scrollAxis(
            this.namedArg(call.args, "in") ?? this.firstPositionalArg(call.args, scope),
          ),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "AnyView": {
        const node = this.parseViewExpression(call.args, scope);
        return node === null ? null : decorate(node);
      }
      case "ScrollView":
        return decorate(this.parseScrollView(call.args, trailing?.body ?? "", scope));
      case "List":
        return decorate(this.parseList(call.args, trailing?.body ?? "", scope));
      case "Form":
        return decorate({
          kind: "list",
          form: true,
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Section":
        return decorate(this.parseSection(call.args, trailing?.body ?? "", scope));
      case "Grid":
      case "LazyVGrid":
      case "LazyHGrid":
        return decorate(this.parseGrid(call.name, call.args, trailing?.body ?? "", scope));
      case "GridRow":
        return decorate({
          kind: "gridRow",
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Menu":
        return decorate({
          kind: "menu",
          title: this.firstPositionalArg(call.args, scope),
          children: this.parseStatements(trailing?.body ?? "", scope),
        });
      case "Text":
        return decorate(this.parseText(call.args, scope));
      case "Label":
        return decorate(this.parseLabel(call.args, scope));
      case "LabeledContent":
        return decorate(this.parseLabeledContent(call.args, trailing?.body ?? "", scope));
      case "GroupBox":
        return decorate(this.parseGroupBox(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "DisclosureGroup":
        return decorate(this.parseDisclosureGroup(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "Image":
        return decorate(this.parseImage(call.args, scope));
      case "AsyncImage":
        return decorate(this.parseAsyncImage(
          call.args,
          trailing?.body ?? "",
          scope,
          asyncTrailing?.labeled,
        ));
      case "ProgressView":
        return decorate(this.parseProgressView(call.args, scope));
      case "Gauge":
        return decorate(this.parseGauge(call.args, scope));
      case "TextField":
        return decorate(this.parseTextField(call.args, trailing?.body ?? "", scope));
      case "SecureField":
        return decorate(this.parseTextField(call.args, trailing?.body ?? "", scope, {
          secure: true,
        }));
      case "TextEditor":
        return decorate(this.parseTextField(call.args, trailing?.body ?? "", scope, {
          multiline: true,
        }));
      case "Stepper":
        return decorate(this.parseStepper(call.args, trailing?.body ?? "", scope));
      case "Slider":
        return decorate(this.parseSlider(call.args, trailing?.body ?? "", scope));
      case "Picker":
        return decorate(this.parsePicker(call.args, trailing?.body ?? "", scope));
      case "DatePicker":
        return decorate(this.parseDatePicker(call.args, trailing?.body ?? "", scope));
      case "ColorPicker":
        return decorate(this.parseColorPicker(call.args, trailing?.body ?? "", scope));
      case "Toggle":
        return decorate(this.parseToggle(call.args, trailing?.body ?? "", scope));
      case "Circle":
        return decorate({ kind: "shape", shape: "circle" });
      case "Ellipse":
        return decorate({ kind: "shape", shape: "ellipse" });
      case "Rectangle":
        return decorate({ kind: "shape", shape: "rectangle" });
      case "RoundedRectangle":
        return decorate({
          kind: "shape",
          shape: "roundedRectangle",
          radius: this.toNumber(this.namedArg(call.args, "cornerRadius"), scope),
          cornerStyle: this.swiftRoundedCornerStyleToken(this.namedArg(call.args, "style"), scope),
        });
      case "UnevenRoundedRectangle":
        return decorate({
          kind: "shape",
          shape: "unevenRoundedRectangle",
          radius: this.toNumber(this.namedArg(call.args, "cornerRadius"), scope),
        });
      case "Capsule":
        return decorate({
          kind: "shape",
          shape: "capsule",
          cornerStyle: this.swiftRoundedCornerStyleToken(this.namedArg(call.args, "style"), scope),
        });
      case "ContainerRelativeShape":
        return decorate({ kind: "shape", shape: "containerRelativeShape" });
      case "Path":
        return decorate(this.parsePath(call.args, scope));
      case "Divider":
        return decorate({ kind: "divider" });
      case "Spacer":
        return decorate({
          kind: "spacer",
          minLength: this.toNumber(this.namedArg(call.args, "minLength"), scope),
        });
      case "EmptyView":
        return decorate({ kind: "empty" });
      case "Button":
        return decorate(this.parseButton(call.args, trailing?.body ?? "", scope));
      default:
        if (this.userFunction(call.name, scope)?.returnsView === true) {
          return decorate(this.invokeViewFunction(call.name, call.args, scope));
        }
        this.warnings.push(`Unsupported Swift view '${call.name}'`);
        return null;
    }
  }

  private parseTextConcatenation(
    text: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode | null {
    const index = this.topLevelOperatorIndex(text, ["+"], true);
    if (index < 0) return null;
    const left = this.parseViewExpression(text.slice(0, index), scope);
    const right = this.parseViewExpression(text.slice(index + 1), scope);
    if (left?.kind !== "text" || right?.kind !== "text") {
      this.warnings.push(`Unsupported Swift Text concatenation: ${text.slice(0, 48)}`);
      return null;
    }
    return {
      kind: "text",
      text: `${left.text}${right.text}`,
      ...(left.markdownRuns !== undefined || right.markdownRuns !== undefined
        ? {
            markdownRuns: [
              ...(left.markdownRuns ?? [{ kind: "text" as const, text: left.text }]),
              ...(right.markdownRuns ?? [{ kind: "text" as const, text: right.text }]),
            ],
          }
        : {}),
    };
  }

  private parseStatements(body: string, scope: SwiftParseScope): CustomSidebarSwiftNode[] {
    const nodes: CustomSidebarSwiftNode[] = [];
    let index = 0;
    let currentScope = scope;
    while (index < body.length && nodes.length < 250) {
      index = this.skipWhitespaceAndSeparators(body, index);
      if (index >= body.length) break;
      if (body.startsWith("@State", index)) {
        const parsed = this.parseStateBinding(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("@Environment", index)) {
        const parsed = this.parseEnvironmentBinding(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("let ", index) || body.startsWith("let\t", index)) {
        const parsed = this.parseLetBinding(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("func ", index) || body.startsWith("func\t", index)) {
        const parsed = this.parseFunctionDeclaration(body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (body.startsWith("return ", index) || body.startsWith("return\t", index)) {
        const statement = this.readViewStatement(
          body,
          this.skipWhitespaceAndSeparators(body, index + "return".length),
        );
        if (statement === null) break;
        const node = this.parseViewExpression(statement.text, currentScope);
        if (node !== null) nodes.push(node);
        break;
      }
      if (body.startsWith("ForEach", index)) {
        const parsed = this.parseForEach(body, index, currentScope);
        nodes.push(...parsed.nodes);
        index = parsed.end;
        continue;
      }
      if (body.startsWith("for ", index) || body.startsWith("for\t", index)) {
        const parsed = this.parseForLoop(body, index, currentScope);
        nodes.push(...parsed.nodes);
        index = parsed.end;
        continue;
      }
      if (body.startsWith("if ", index) || body.startsWith("if\t", index)) {
        const parsed = this.parseIf(body, index, currentScope);
        nodes.push(...parsed.nodes);
        index = parsed.end;
        continue;
      }
      const statement = this.readViewStatement(body, index);
      if (statement === null) {
        index += 1;
        continue;
      }
      const node = this.parseViewExpression(statement.text, currentScope);
      if (node !== null) nodes.push(node);
      index = statement.end;
    }
    return nodes;
  }

  private parseStateBinding(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const statement = this.readSimpleStatement(body, index);
    const match = statement.text.match(
      /^@State\s+(?:(?:private|fileprivate|internal|public)\s+)?var\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*[^=]+)?\s*=\s*([\s\S]+)$/,
    );
    if (!match) {
      this.warnings.push(`Unsupported @State binding '${statement.text.slice(0, 48)}'`);
      return { scope, end: statement.end };
    }
    const key = match[1] ?? "value";
    const defaultValue = this.swiftStateValue(this.evalValue(match[2] ?? "", scope));
    const stateValue =
      scope.__stateValues !== undefined &&
      Object.prototype.hasOwnProperty.call(scope.__stateValues, key)
        ? scope.__stateValues[key]
        : defaultValue;
    return {
      scope: {
        ...scope,
        [key]: stateValue,
        __stateKeys: { ...(scope.__stateKeys ?? {}), [key]: true },
      },
      end: statement.end,
    };
  }

  private parseEnvironmentBinding(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const statement = this.readSimpleStatement(body, index);
    const match = statement.text.match(
      /^@Environment\s*\(([\s\S]+?)\)\s+(?:(?:private|fileprivate|internal|public)\s+)?var\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*.+)?$/,
    );
    if (!match) {
      this.warnings.push(`Unsupported @Environment binding '${statement.text.slice(0, 48)}'`);
      return { scope, end: statement.end };
    }
    const key = this.swiftEnvironmentReadKeyToken(match[1], scope);
    if (key === undefined) {
      this.warnings.push(`Unsupported @Environment key '${(match[1] ?? "").trim()}'`);
      return { scope, end: statement.end };
    }
    return {
      scope: {
        ...scope,
        [match[2] ?? key]: this.swiftEnvironmentReadValue(key),
      } as SwiftParseScope,
      end: statement.end,
    };
  }

  private parseLetBinding(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const statement = this.readSimpleStatement(body, index);
    const match = statement.text.match(/^let\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
    if (!match) {
      this.warnings.push(`Unsupported let binding '${statement.text.slice(0, 40)}'`);
      return { scope, end: statement.end };
    }
    return {
      scope: {
        ...scope,
        [match[1] ?? "value"]: this.evalValue(match[2] ?? "", scope),
      } as SwiftParseScope,
      end: statement.end,
    };
  }

  private parseFunctionDeclaration(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { scope: SwiftParseScope; end: number } {
    const source = body.slice(index);
    const match = source.match(/^func\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/);
    if (!match) return { scope, end: index + 4 };
    const open = index + source.indexOf("(");
    const params = this.readBalanced(body, open, "(", ")");
    if (params === null) return { scope, end: open + 1 };
    const brace = body.indexOf("{", params.end);
    if (brace < 0) return { scope, end: params.end };
    const block = this.readBalanced(body, brace, "{", "}");
    if (block === null) return { scope, end: brace + 1 };
    const returnSignature = body.slice(params.end, brace);
    const name = match[1] ?? "";
    return {
      scope: {
        ...scope,
        __functions: {
          ...(scope.__functions ?? {}),
          [name]: {
            params: this.parseFunctionParams(params.content),
            body: block.content,
            returnsView: /\bsome\s+View\b|\bView\b/.test(returnSignature),
          },
        },
      } as SwiftParseScope,
      end: block.end,
    };
  }

  private parseForEach(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { nodes: CustomSidebarSwiftNode[]; end: number } {
    const call = this.readCall(body.slice(index));
    if (call === null) return { nodes: [], end: index + 1 };
    const trailing = this.readTrailingClosure(body.slice(index + call.end));
    if (trailing === null) return { nodes: [], end: index + call.end };
    const collection = this.splitTopLevel(call.args, ",")[0]?.trim();
    const idExpression = this.namedArg(call.args, "id");
    const bindingCollectionKey = this.stateBindingKey(collection ?? "", scope);
    const values = this.evalSequence(bindingCollectionKey ?? collection ?? "", scope);
    if (values === null) {
      this.warnings.push(`Unsupported ForEach collection '${collection ?? ""}'`);
      return { nodes: [], end: index + call.end + trailing.end };
    }
    const closure = this.splitClosureParameter(trailing.body);
    const params = closure.params.length > 0 ? closure.params : ["workspace"];
    const nodes = values.flatMap((value, valueIndex) =>
      this.applyForEachIdentity(
        this.parseStatements(
          closure.body,
          this.scopeWithForEachValues(scope, params, value, bindingCollectionKey, valueIndex),
        ),
        this.forEachIdentityValue(idExpression, value),
      ),
    );
    return { nodes, end: index + call.end + trailing.end };
  }

  private applyForEachIdentity(
    nodes: CustomSidebarSwiftNode[],
    identity: string | undefined,
  ): CustomSidebarSwiftNode[] {
    if (identity === undefined) return nodes;
    return nodes.map((node) => ({
      ...node,
      modifiers: [
        ...(node.modifiers ?? []),
        { name: "id", value: identity } satisfies CustomSidebarSwiftModifier,
      ],
    }));
  }

  private forEachIdentityValue(
    expression: string | undefined,
    value: unknown,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = expression.trim().replace(/^\\?\./, "");
    if (token === "self") return this.swiftStringValue(value);
    if (typeof value === "object" && value !== null) {
      const resolved = (value as Record<string, unknown>)[token];
      return this.swiftStringValue(resolved);
    }
    return undefined;
  }

  private swiftStringValue(value: unknown): string | undefined {
    if (value === undefined || value === null) return undefined;
    if (typeof value === "string") return value;
    if (typeof value === "number" || typeof value === "boolean") return String(value);
    return undefined;
  }

  private parseForLoop(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { nodes: CustomSidebarSwiftNode[]; end: number } {
    const open = body.indexOf("{", index);
    if (open < 0) return { nodes: [], end: index + 3 };
    const header = body.slice(index + 3, open).trim();
    const block = this.readBalanced(body, open, "{", "}");
    if (block === null) return { nodes: [], end: open + 1 };
    const match = header.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+in\s+(.+)$/);
    if (!match) {
      this.warnings.push(`Unsupported for loop '${header}'`);
      return { nodes: [], end: block.end };
    }
    const values = this.evalSequence(match[2] ?? "", scope);
    if (values === null) {
      this.warnings.push(`Unsupported for loop sequence '${match[2] ?? ""}'`);
      return { nodes: [], end: block.end };
    }
    const param = match[1] ?? "item";
    return {
      nodes: values.flatMap((value) =>
        this.parseStatements(block.content, this.scopeWithLoopValue(scope, param, value)),
      ),
      end: block.end,
    };
  }

  private parseIf(
    body: string,
    index: number,
    scope: SwiftParseScope,
  ): { nodes: CustomSidebarSwiftNode[]; end: number } {
    const open = body.indexOf("{", index);
    if (open < 0) return { nodes: [], end: index + 2 };
    const condition = body.slice(index + 2, open).trim();
    const block = this.readBalanced(body, open, "{", "}");
    if (block === null) return { nodes: [], end: open + 1 };
    const afterThen = this.skipWhitespaceAndSeparators(body, block.end);
    let elseBlock: { content: string; end: number } | null = null;
    if (body.startsWith("else", afterThen)) {
      const elseOpen = body.indexOf("{", afterThen + 4);
      elseBlock = elseOpen >= 0 ? this.readBalanced(body, elseOpen, "{", "}") : null;
    }
    const optionalBinding = condition.match(/^let\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/);
    if (optionalBinding) {
      const value = this.evalOptionalBindingValue(optionalBinding[2] ?? "", scope);
      const passed = value !== undefined && value !== null;
      const bindingScope = passed
        ? ({ ...scope, [optionalBinding[1] ?? "value"]: value } as SwiftParseScope)
        : scope;
      return {
        nodes: this.parseStatements(
          passed ? block.content : (elseBlock?.content ?? ""),
          bindingScope,
        ),
        end: elseBlock?.end ?? block.end,
      };
    }
    const passed = this.evalCondition(condition, scope);
    return {
      nodes: this.parseStatements(
        passed ? block.content : (elseBlock?.content ?? ""),
        scope,
      ),
      end: elseBlock?.end ?? block.end,
    };
  }

  private evalOptionalBindingValue(expression: string, scope: SwiftParseScope): unknown {
    const resolved = this.resolvePath(expression.trim(), scope);
    if (resolved !== undefined) return resolved;
    const evaluated = this.evalExpression(expression, scope);
    return evaluated === expression.trim() ? undefined : evaluated;
  }

  private parseButton(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const actionArg = this.extractNamedClosure(args, "action");
    const role = this.parseButtonRole(args, scope);
    if (actionArg !== null) {
      const localAction = this.parseLocalButtonAction(actionArg);
      return {
        kind: "button",
        role,
        children: this.parseStatements(trailingBody, scope),
        action: this.parseCmuxAction(actionArg, scope),
        ...(localAction !== undefined ? { localAction } : {}),
      };
    }
    const localAction = this.parseLocalButtonAction(trailingBody);
    return {
      kind: "button",
      role,
      text: this.evalExpression(args.split(",")[0] ?? "", scope),
      children: [],
      action: this.parseCmuxAction(trailingBody, scope),
      ...(localAction !== undefined ? { localAction } : {}),
    };
  }

  private parseLocalButtonAction(body: string): "dismissPresentation" | undefined {
    return /\bdismiss\s*\(\s*\)/.test(body) ? "dismissPresentation" : undefined;
  }

  private parseNavigationLink(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const destinationArg = this.namedArg(args, "destination");
    const valueArg = this.namedArg(args, "value");
    const title = this.firstPositionalArg(args, scope) ?? "Open";
    const destination =
      destinationArg === undefined && valueArg === undefined
        ? this.parseStatements(trailingBody, scope)
        : destinationArg === undefined
          ? []
          : this.parseSingleViewArgument(destinationArg, scope);
    const labelChildren =
      destinationArg === undefined && valueArg === undefined
        ? [{ kind: "text", text: title } satisfies CustomSidebarSwiftNode]
        : this.parseStatements(trailingBody, scope);
    return {
      kind: "navigationLink",
      title,
      children:
        labelChildren.length > 0
          ? labelChildren
          : [{ kind: "text", text: title } satisfies CustomSidebarSwiftNode],
      destination,
      ...(valueArg !== undefined ? { value: this.evalExpression(valueArg, scope) } : {}),
    };
  }

  private parseExternalLink(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const destinationArg = this.namedArg(args, "destination");
    const title = this.firstPositionalArg(args, scope);
    const labelChildren = this.parseStatements(trailingBody, scope);
    return {
      kind: "externalLink",
      title,
      href:
        destinationArg === undefined
          ? undefined
          : this.safeExternalLinkUrl(destinationArg, scope),
      labelChildren,
    };
  }

  private parseContentUnavailableView(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const labelClosure =
      this.extractNamedClosure(args, "label") ??
      labeledTrailingClosures.label ??
      (trailingBody.trim() === "" ? null : trailingBody);
    const descriptionClosure =
      this.extractNamedClosure(args, "description") ??
      labeledTrailingClosures.description ??
      null;
    const actionsClosure =
      this.extractNamedClosure(args, "actions") ??
      labeledTrailingClosures.actions ??
      null;
    const descriptionArg = this.namedArg(args, "description");
    const descriptionChildren =
      descriptionClosure !== null && descriptionClosure.trim() !== ""
        ? this.parseStatements(descriptionClosure, scope)
        : descriptionArg !== undefined
          ? this.parseSingleViewArgument(descriptionArg, scope)
          : [];
    return {
      kind: "contentUnavailable",
      title: this.firstPositionalArg(args, scope),
      systemImage: this.namedStringArg(args, "systemImage", scope),
      labelChildren:
        labelClosure === null || labelClosure.trim() === ""
          ? []
          : this.parseStatements(labelClosure, scope),
      descriptionChildren,
      actionsChildren:
        actionsClosure === null || actionsClosure.trim() === ""
          ? []
          : this.parseStatements(actionsClosure, scope),
    };
  }

  private parseSingleViewArgument(
    expression: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode[] {
    const node = this.parseViewExpression(expression, scope);
    return node === null ? [] : [node];
  }

  private parsePresentationModifier(
    name: "sheet" | "popover" | "fullScreenCover" | "alert" | "confirmationDialog",
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftModifier | null {
    const isPresented = this.namedArg(args, "isPresented");
    const item = this.namedArg(args, "item");
    if (isPresented === undefined && item === undefined) return null;
    const binding = isPresented ?? item ?? "";
    const stateBindingKey = this.stateBindingKey(binding, scope);
    const title = this.firstPositionalArg(args, scope);
    if (item !== undefined) {
      const itemValue = this.swiftStateValue(this.evalBindingValue(item, scope));
      const closure = this.splitClosureParameter(trailingBody);
      const itemParam = closure.params[0] ?? "item";
      return {
        name,
        value: title,
        boolValue: itemValue !== null,
        itemParam,
        itemValue,
        presentationBindingKind: "item",
        ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
        children: this.parseStatements(
          closure.body,
          this.scopeWithLoopValue(scope, itemParam, itemValue),
        ),
      };
    }
    return {
      name,
      value: title,
      boolValue: this.evalBindingCondition(binding, scope),
      presentationBindingKind: "isPresented",
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseScrollView(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const first = this.firstPositionalArg(args, scope);
    const axis = this.scrollAxis(first);
    const showsIndicatorsArg = this.namedArg(args, "showsIndicators");
    return {
      kind: "scrollView",
      axis,
      showsIndicators:
        showsIndicatorsArg === undefined ? true : this.evalCondition(showsIndicatorsArg, scope),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private scrollAxis(value: string | undefined): "vertical" | "horizontal" | "both" {
    const normalized = value?.replace(/^\./, "");
    if (normalized === "horizontal") return "horizontal";
    if (normalized === "vertical") return "vertical";
    if (normalized?.includes("horizontal") && normalized.includes("vertical")) return "both";
    return "vertical";
  }

  private stackSpacing(args: string, scope: SwiftParseScope): number | undefined {
    return this.toNumber(this.namedArg(args, "spacing"), scope);
  }

  private parseButtonRole(
    args: string,
    scope: SwiftParseScope,
  ): "destructive" | "cancel" | undefined {
    const role = this.namedStringArg(args, "role", scope)?.replace(/^\./, "");
    return role === "destructive" || role === "cancel" ? role : undefined;
  }

  private swiftTypeToken(value: string | undefined): string | undefined {
    return value
      ?.trim()
      .replace(/\.self$/, "")
      .replace(/^\./, "")
      .replace(/^[A-Za-z_][A-Za-z0-9_]*\./, "");
  }

  private parseProgressView(
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const valueArg = this.namedArg(args, "value") ?? this.splitTopLevel(args, ",")[0];
    const totalArg = this.namedArg(args, "total");
    const value = this.toNumber(valueArg, scope);
    const total = this.toNumber(totalArg, scope);
    return {
      kind: "progress",
      value,
      total: total ?? (value === undefined ? undefined : 1),
    };
  }

  private parseGauge(
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const valueArg = this.namedArg(args, "value") ?? this.splitTopLevel(args, ",")[0];
    const value = this.toNumber(valueArg, scope);
    const bounds = this.parseRangeBounds(this.namedArg(args, "in"), scope);
    if (value !== undefined && bounds !== undefined) {
      return {
        kind: "progress",
        value: value - bounds.lower,
        total: bounds.upper - bounds.lower,
      };
    }
    return {
      kind: "progress",
      value,
      total: value === undefined ? undefined : 1,
    };
  }

  private parseTextField(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    options: { secure?: boolean; multiline?: boolean } = {},
  ): CustomSidebarSwiftNode {
    const textBinding = this.namedArg(args, "text") ?? "";
    const stateBindingKey = this.stateBindingKey(textBinding, scope);
    return {
      kind: "textField",
      placeholder: this.firstPositionalArg(args, scope),
      text: this.evalBindingExpression(textBinding, scope),
      ...(options.secure === true ? { secure: true } : {}),
      ...(options.multiline === true ? { multiline: true } : {}),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseStepper(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const valueBinding = this.namedArg(args, "value") ?? "";
    const bounds = this.parseRangeBounds(this.namedArg(args, "in"), scope);
    const stateBindingKey = this.stateBindingKey(valueBinding, scope);
    return {
      kind: "stepper",
      title: this.firstPositionalArg(args, scope),
      value: this.toNumber(this.unwrapBindingExpression(valueBinding), scope),
      lowerBound: bounds?.lower ?? Number.NEGATIVE_INFINITY,
      upperBound: bounds?.upper ?? Number.POSITIVE_INFINITY,
      step: this.toNumber(this.namedArg(args, "step"), scope) ?? 1,
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseSlider(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const bounds = this.parseRangeBounds(this.namedArg(args, "in"), scope);
    const valueBinding = this.namedArg(args, "value") ?? "";
    const stateBindingKey = this.stateBindingKey(valueBinding, scope);
    const value = this.toNumber(
      this.unwrapBindingExpression(valueBinding),
      scope,
    );
    return {
      kind: "slider",
      value,
      lowerBound: bounds?.lower ?? 0,
      upperBound: bounds?.upper ?? 1,
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parsePicker(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const selectionBinding = this.namedArg(args, "selection") ?? "";
    const stateBindingKey = this.stateBindingKey(selectionBinding, scope);
    const selectedValue = this.swiftStateValue(this.evalBindingValue(selectionBinding, scope));
    const children = this.parseStatements(trailingBody, scope);
    const options = this.swiftPickerOptions(children);
    return {
      kind: "picker",
      title: this.firstPositionalArg(args, scope),
      selection: this.swiftPickerSelectionLabel(selectedValue, options),
      selectedValue,
      options,
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children,
    };
  }

  private swiftPickerOptions(nodes: CustomSidebarSwiftNode[]): CustomSidebarSwiftPickerOption[] {
    const options = nodes.flatMap((node) => this.swiftPickerOptionsFromNode(node));
    const unique = new Map<string, CustomSidebarSwiftPickerOption>();
    for (const option of options) {
      if (!unique.has(option.encodedValue)) unique.set(option.encodedValue, option);
    }
    return [...unique.values()];
  }

  private swiftPickerOptionsFromNode(node: CustomSidebarSwiftNode): CustomSidebarSwiftPickerOption[] {
    const tagValue = this.swiftNodeTagValue(node);
    const label = this.swiftPickerNodeLabel(node);
    if (label !== undefined) {
      const value = tagValue ?? label;
      return [
        {
          label,
          value,
          encodedValue: this.swiftPickerEncodedValue(value),
        },
      ];
    }
    if (node.kind === "modified") return this.swiftPickerOptionsFromNode(node.base);
    if (
      node.kind === "group" ||
      node.kind === "vstack" ||
      node.kind === "hstack" ||
      node.kind === "gridRow"
    ) {
      return node.children.flatMap((child) => this.swiftPickerOptionsFromNode(child));
    }
    return [];
  }

  private swiftPickerNodeLabel(node: CustomSidebarSwiftNode): string | undefined {
    if (node.kind === "text" && node.text.trim() !== "") return node.text;
    if (node.kind === "label" && node.text.trim() !== "") return node.text;
    if (node.kind === "modified") return this.swiftPickerNodeLabel(node.base);
    if (
      node.kind === "group" ||
      node.kind === "vstack" ||
      node.kind === "hstack" ||
      node.kind === "gridRow"
    ) {
      return node.children
        .map((child) => this.swiftPickerNodeLabel(child))
        .find((childLabel) => childLabel !== undefined);
    }
    return undefined;
  }

  private swiftNodeTagValue(node: CustomSidebarSwiftNode): SwiftSidebarStateValue | undefined {
    const tag = node.modifiers?.find((modifier) => modifier.name === "tag");
    if (tag?.tagValue !== undefined) return tag.tagValue;
    if (node.kind === "modified") return this.swiftNodeTagValue(node.base);
    return undefined;
  }

  private swiftPickerSelectionLabel(
    selectedValue: SwiftSidebarStateValue,
    options: CustomSidebarSwiftPickerOption[],
  ): string {
    const selected = this.swiftPickerEncodedValue(selectedValue);
    return options.find((option) => option.encodedValue === selected)?.label ?? String(selectedValue ?? "");
  }

  private swiftPickerEncodedValue(value: SwiftSidebarStateValue): string {
    return JSON.stringify(value);
  }

  private parseDatePicker(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const selectionBinding = this.namedArg(args, "selection") ?? "";
    const stateBindingKey = this.stateBindingKey(selectionBinding, scope);
    const displayedComponents = this.namedArg(args, "displayedComponents");
    return {
      kind: "datePicker",
      title: this.firstPositionalArg(args, scope),
      value: this.evalBindingExpression(selectionBinding, scope),
      ...(displayedComponents !== undefined
        ? { displayedComponents: this.evalExpression(displayedComponents, scope) }
        : {}),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseColorPicker(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const selectionBinding = this.namedArg(args, "selection") ?? "";
    const stateBindingKey = this.stateBindingKey(selectionBinding, scope);
    return {
      kind: "colorPicker",
      title: this.firstPositionalArg(args, scope),
      value: this.evalBindingExpression(selectionBinding, scope),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseList(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const collection = this.firstPositionalExpression(args);
    if (collection === undefined) {
      return {
        kind: "list",
        children: this.parseStatements(trailingBody, scope),
      };
    }
    const values = this.evalSequence(collection, scope);
    if (values === null) {
      this.warnings.push(`Unsupported List collection '${collection}'`);
      return {
        kind: "list",
        children: this.parseStatements(trailingBody, scope),
      };
    }
    const closure = this.splitClosureParameter(trailingBody);
    const params = closure.params.length > 0 ? closure.params : ["item"];
    return {
      kind: "list",
      dataId: this.swiftListIdToken(this.namedArg(args, "id"), scope),
      children: values.flatMap((value) =>
        this.parseStatements(closure.body, this.scopeWithLoopValues(scope, params, value)),
      ),
    };
  }

  private parseSection(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const headerExpression = this.namedArg(args, "header");
    const footerExpression = this.namedArg(args, "footer");
    return {
      kind: "section",
      title: this.firstPositionalArg(args, scope),
      ...(headerExpression !== undefined
        ? { header: this.parseViewExpressionList(headerExpression, scope) }
        : {}),
      ...(footerExpression !== undefined
        ? { footer: this.parseViewExpressionList(footerExpression, scope) }
        : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseGrid(
    name: "Grid" | "LazyVGrid" | "LazyHGrid",
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const gridKind =
      name === "LazyVGrid" ? "lazyVGrid" : name === "LazyHGrid" ? "lazyHGrid" : "grid";
    const itemExpression =
      name === "LazyHGrid"
        ? this.namedArg(args, "rows") ?? this.firstPositionalExpression(args)
        : name === "LazyVGrid"
          ? this.namedArg(args, "columns") ?? this.firstPositionalExpression(args)
          : undefined;
    return {
      kind: "grid",
      gridKind,
      ...(itemExpression !== undefined
        ? { gridItems: this.swiftGridItems(itemExpression, scope) }
        : {}),
      spacing: this.toNumber(this.namedArg(args, "spacing"), scope),
      alignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
      ...(gridKind !== "grid"
        ? { pinnedViews: this.swiftPinnedViews(this.namedArg(args, "pinnedViews"), scope) }
        : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private parseViewExpressionList(
    expression: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode[] {
    const trimmed = expression.trim();
    if (trimmed === "") return [];
    const node = this.parseViewExpression(trimmed, scope);
    return node === null ? [] : [node];
  }

  private parseToggle(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const isOnBinding = this.namedArg(args, "isOn") ?? "false";
    const stateBindingKey = this.stateBindingKey(isOnBinding, scope);
    return {
      kind: "toggle",
      text: this.firstPositionalArg(args, scope),
      isOn: this.evalBindingCondition(isOnBinding, scope),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      children: this.parseStatements(trailingBody, scope),
    };
  }

  private withModifiers(
    node: CustomSidebarSwiftNode,
    text: string,
    modifierStart: number,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const modifiers = this.parseModifiers(text, modifierStart, scope);
    if (modifiers.length === 0) return node;
    const nodeWithRouteDestinations =
      node.kind === "navigationStack"
        ? this.resolveNavigationValueDestinations(node, modifiers, scope)
        : node;
    const tapModifier = modifiers.find((modifier) => modifier.name === "onTapGesture");
    const longPressModifier = modifiers.find(
      (modifier) => modifier.name === "onLongPressGesture",
    );
    const eventHandlers = modifiers
      .map((modifier) => modifier.eventHandler)
      .filter((handler): handler is CustomSidebarSwiftEventHandler => handler !== undefined);
    const childModifiers = modifiers.filter((modifier) =>
      this.isChildModifier(modifier),
    );
    const hasNavigationChrome = modifiers.some((modifier) =>
      this.isNavigationModifier(modifier),
    );
    const visualModifiers = modifiers.filter(
      (modifier) =>
        modifier.name !== "onTapGesture" &&
        modifier.name !== "onLongPressGesture" &&
        modifier.name !== "onEvent" &&
        !this.isChildModifier(modifier),
    );
    const decorated =
      visualModifiers.length === 0 && eventHandlers.length === 0
        ? nodeWithRouteDestinations
        : {
            ...nodeWithRouteDestinations,
            ...(visualModifiers.length > 0 ? { modifiers: visualModifiers } : {}),
            ...(eventHandlers.length > 0
              ? { eventHandlers: [...(nodeWithRouteDestinations.eventHandlers ?? []), ...eventHandlers] }
              : {}),
          };
    const gestureAction = tapModifier?.action ?? longPressModifier?.action;
    const actionTrigger = longPressModifier?.action !== undefined ? "longPress" : "tap";
    const tappable =
      gestureAction === undefined
        ? decorated
        : {
            kind: "button",
            children: [decorated],
            action: gestureAction,
            actionTrigger,
            tapCount: tapModifier?.count,
            modifiers: [{ name: "buttonStyle", value: "plain" }],
          } satisfies CustomSidebarSwiftNode;
    if (childModifiers.length === 0 && !hasNavigationChrome) return tappable;
    return {
      kind: "modified",
      base: tappable,
      childModifiers,
    };
  }

  private resolveNavigationValueDestinations(
    node: Extract<CustomSidebarSwiftNode, { kind: "navigationStack" }>,
    modifiers: CustomSidebarSwiftModifier[],
    scope: SwiftParseScope,
  ): Extract<CustomSidebarSwiftNode, { kind: "navigationStack" }> {
    const routeDestinations = modifiers.filter(
      (modifier) => modifier.name === "navigationDestination" && modifier.routeBody !== undefined,
    );
    if (routeDestinations.length === 0) return node;
    return {
      ...node,
      children: node.children.map((child) =>
        this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
      ),
    };
  }

  private resolveNavigationValueDestinationNode(
    node: CustomSidebarSwiftNode,
    routeDestinations: CustomSidebarSwiftModifier[],
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    if (node.kind === "navigationLink" && node.value !== undefined && node.destination.length === 0) {
      const destination = routeDestinations[0];
      const routeParam = destination?.routeParam ?? "value";
      const routeBody = destination?.routeBody ?? "";
      return {
        ...node,
        destination: this.parseStatements(
          routeBody,
          this.scopeWithLoopValue(scope, routeParam, node.value),
        ),
      };
    }
    if (
      node.kind === "vstack" ||
      node.kind === "hstack" ||
      node.kind === "zstack" ||
      node.kind === "group" ||
      node.kind === "splitView" ||
      node.kind === "tabView" ||
      node.kind === "scrollView" ||
      node.kind === "list" ||
      node.kind === "section" ||
      node.kind === "groupBox" ||
      node.kind === "disclosureGroup" ||
      node.kind === "grid" ||
      node.kind === "gridRow" ||
      node.kind === "menu" ||
      node.kind === "button"
    ) {
      return {
        ...node,
        children: node.children.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
        ...(node.kind === "groupBox" || node.kind === "disclosureGroup"
          ? {
              labelChildren: node.labelChildren.map((child) =>
                this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
              ),
            }
          : {}),
      };
    }
    if (node.kind === "externalLink") {
      return {
        ...node,
        labelChildren: node.labelChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
      };
    }
    if (node.kind === "contentUnavailable") {
      return {
        ...node,
        labelChildren: node.labelChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
        descriptionChildren: node.descriptionChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
        actionsChildren: node.actionsChildren.map((child) =>
          this.resolveNavigationValueDestinationNode(child, routeDestinations, scope),
        ),
      };
    }
    return node;
  }

  private isNavigationModifier(modifier: CustomSidebarSwiftModifier): boolean {
    return (
      modifier.name === "navigationTitle" ||
      modifier.name === "navigationSubtitle" ||
      modifier.name === "navigationBarTitleDisplayMode"
    );
  }

  private isChildModifier(modifier: CustomSidebarSwiftModifier): boolean {
    return (
      (modifier.name === "background" ||
        modifier.name === "overlay" ||
        modifier.name === "mask" ||
        modifier.name === "safeAreaInset" ||
        modifier.name === "contextMenu" ||
        modifier.name === "refreshable" ||
        modifier.name === "swipeActions" ||
        modifier.name === "sheet" ||
        modifier.name === "popover" ||
        modifier.name === "fullScreenCover" ||
        modifier.name === "alert" ||
        modifier.name === "confirmationDialog" ||
        modifier.name === "toolbar" ||
        modifier.name === "searchable" ||
        modifier.name === "accessibilityRepresentation" ||
        modifier.name === "tabItem") &&
      ((modifier.children?.length ?? 0) > 0 ||
        modifier.action !== undefined ||
        modifier.name === "searchable" ||
        modifier.name === "refreshable" ||
        modifier.name === "alert" ||
        modifier.name === "confirmationDialog")
    );
  }

  private parseModifiers(
    text: string,
    start: number,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftModifier[] {
    const modifiers: CustomSidebarSwiftModifier[] = [];
    let index = start;
    while (index < text.length && modifiers.length < 40) {
      index = this.skipWhitespaceAndSeparators(text, index);
      if (text[index] !== ".") break;
      const call = this.readModifierCall(text.slice(index + 1));
      if (call === null) break;
      index += 1 + call.end;
      const trailing = this.readTrailingClosure(text.slice(index));
      const modifier = this.parseModifier(
        call.name,
        call.args,
        scope,
        trailing?.body,
      );
      if (modifier !== null) modifiers.push(modifier);
      if (trailing !== null) index += trailing.end;
    }
    return modifiers;
  }

  private parseModifier(
    name: string,
    args: string,
    scope: SwiftParseScope,
    trailingBody?: string,
  ): CustomSidebarSwiftModifier | null {
    switch (name) {
      case "onTapGesture": {
        const action = this.parseCmuxAction(trailingBody ?? args, scope);
        return action === undefined
          ? null
          : {
              name: "onTapGesture",
              action,
              count: Math.max(1, Math.floor(this.toNumber(this.namedArg(args, "count"), scope) ?? 1)),
            };
      }
      case "onLongPressGesture": {
        const action = this.parseCmuxAction(trailingBody ?? args, scope);
        return action === undefined ? null : { name: "onLongPressGesture", action };
      }
      case "onHover": {
        const closure = this.splitClosureParameter(trailingBody ?? "");
        const hoverParam = closure.params[0] ?? "hovering";
        const enterHandler = this.parseLocalHandler(
          closure.body,
          this.scopeWithLoopValue(scope, hoverParam, true),
          "onHover",
        );
        const leaveHandler = this.parseLocalHandler(
          closure.body,
          this.scopeWithLoopValue(scope, hoverParam, false),
          "onHover",
        );
        return enterHandler === null && leaveHandler === null
          ? null
          : {
              name: "onHover",
              value: hoverParam,
              ...(enterHandler !== null ? { localHandler: enterHandler } : {}),
              ...(leaveHandler !== null ? { falseLocalHandler: leaveHandler } : {}),
            };
      }
      case "onEvent": {
        const eventHandler = this.parseEventHandler(args, trailingBody ?? "", scope);
        return eventHandler === null ? null : { name: "onEvent", eventHandler };
      }
      case "onSubmit": {
        const localHandler = this.parseLocalHandler(trailingBody ?? args, scope, name);
        return localHandler === null ? null : { name: "onSubmit", localHandler };
      }
      case "onChange": {
        const localHandler = this.parseLocalHandler(trailingBody ?? "", scope, name);
        if (localHandler === null) return null;
        const observedExpression =
          this.namedArg(args, "of") ?? this.splitTopLevel(args, ",")[0]?.trim();
        return {
          name: "onChange",
          localHandler,
          ...(observedExpression !== undefined && observedExpression !== ""
            ? {
                value: this.evalExpression(observedExpression, scope),
                secondaryValue: observedExpression,
              }
            : {}),
        };
      }
      case "onGeometryChange": {
        return {
          name: "onGeometryChange",
          value: this.swiftTypeToken(this.namedArg(args, "for") ?? this.splitTopLevel(args, ",")[0]),
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      }
      case "onAppear":
      case "onDisappear": {
        const localHandler = this.parseLocalHandler(trailingBody ?? args, scope, name);
        return localHandler === null ? null : { name, localHandler };
      }
      case "task": {
        const localHandler = this.parseLocalHandler(trailingBody ?? "", scope, "task");
        if (localHandler === null) return null;
        const taskIdExpression = this.namedArg(args, "id");
        return {
          name: "task",
          localHandler,
          ...(taskIdExpression !== undefined && taskIdExpression !== ""
            ? {
                value: this.evalExpression(taskIdExpression, scope),
                secondaryValue: taskIdExpression,
              }
            : {}),
        };
      }
      case "tag": {
        const tagArg = this.splitTopLevel(args, ",")[0] ?? "";
        const tagValue = this.swiftStateValue(this.evalValue(tagArg, scope));
        return {
          name: "tag",
          value: String(tagValue ?? ""),
          tagValue,
        };
      }
      case "tabItem":
        return {
          name: "tabItem",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "id":
        return { name: "id", value: this.evalExpression(this.splitTopLevel(args, ",")[0] ?? "", scope) };
      case "animation":
        return {
          name: "animation",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "value"),
        };
      case "transition":
        return { name: "transition", value: this.firstPositionalArg(args, scope) };
      case "contentTransition":
        return { name: "contentTransition", value: this.firstPositionalArg(args, scope) };
      case "symbolEffect":
        return {
          name: "symbolEffect",
          value: this.firstPositionalArg(args, scope),
          boolValue:
            this.namedArg(args, "isActive") === undefined
              ? undefined
              : this.evalCondition(this.namedArg(args, "isActive") ?? "false", scope),
          secondaryValue: this.namedArg(args, "value"),
        };
      case "symbolEffectsRemoved":
        return {
          name: "symbolEffectsRemoved",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "buttonStyle":
        return { name: "buttonStyle", value: this.firstPositionalArg(args, scope) };
      case "font":
        return { name: "font", value: this.firstPositionalArg(args, scope) };
      case "fontWeight":
        return { name: "fontWeight", value: this.firstPositionalArg(args, scope) };
      case "fontDesign":
        return { name: "fontDesign", value: this.firstPositionalArg(args, scope) };
      case "fontWidth":
        return { name: "fontWidth", value: this.firstPositionalArg(args, scope) };
      case "dynamicTypeSize":
        return { name: "dynamicTypeSize", value: this.firstPositionalArg(args, scope) };
      case "bold":
        return args.trim() === ""
          ? { name: "bold" }
          : {
              name: "bold",
              boolValue: this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
            };
      case "italic":
        return args.trim() === ""
          ? { name: "italic" }
          : {
              name: "italic",
              boolValue: this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
            };
      case "monospaced":
        return { name: "monospaced" };
      case "monospacedDigit":
        return { name: "monospacedDigit" };
      case "foregroundColor":
      case "foregroundStyle":
      case "fill":
      case "tint":
        return {
          name: "foregroundColor",
          value: this.firstPositionalArg(args, scope),
        };
      case "padding": {
        const parts = this.splitTopLevel(args, ",");
        const namedEdges = this.namedArg(args, "edges");
        const namedLength = this.namedArg(args, "length");
        const first = parts[0];
        const insetPadding = this.swiftPaddingInsetsValue(
          namedEdges === undefined && namedLength === undefined ? first : undefined,
          scope,
        );
        if (Object.keys(insetPadding).length > 0) {
          return { name: "padding", ...insetPadding };
        }
        const firstIsEdgeSet = this.swiftEdgeSet(namedEdges ?? first, scope) !== undefined;
        const edge = this.swiftEdgeSet(namedEdges ?? (firstIsEdgeSet ? first : undefined), scope);
        const amountExpression =
          namedLength ?? (edge === undefined ? parts.at(-1) : (parts[1] ?? undefined));
        const amount = this.toNumber(amountExpression, scope)?.toString();
        return { name: "padding", value: amount, edge: edge?.join(",") };
      }
      case "safeAreaPadding":
      case "contentMargins": {
        const parts = this.splitTopLevel(args, ",");
        const namedEdges = this.namedArg(args, "edges");
        const namedLength = this.namedArg(args, "length");
        const first = parts[0];
        const insetPadding = this.swiftPaddingInsetsValue(
          namedEdges === undefined && namedLength === undefined ? first : undefined,
          scope,
        );
        const placement = this.swiftContentMarginPlacementToken(this.namedArg(args, "for"), scope);
        if (Object.keys(insetPadding).length > 0) {
          return {
            name,
            ...insetPadding,
            ...(placement !== undefined ? { placement } : {}),
          };
        }
        const firstIsEdgeSet = this.swiftEdgeSet(namedEdges ?? first, scope) !== undefined;
        const edge = this.swiftEdgeSet(namedEdges ?? (firstIsEdgeSet ? first : undefined), scope);
        const amountExpression =
          namedLength ?? (edge === undefined ? first : (parts[1] ?? undefined));
        const amount = this.toNumber(amountExpression, scope)?.toString();
        return {
          name,
          value: amount,
          edge: edge?.join(","),
          ...(placement !== undefined ? { placement } : {}),
        };
      }
      case "gridCellColumns":
        return {
          name: "gridCellColumns",
          value: this.toNumber(this.firstPositionalExpression(args), scope)?.toString(),
        };
      case "gridColumnAlignment":
        return { name: "gridColumnAlignment", value: this.firstPositionalArg(args, scope) };
      case "gridCellAnchor":
        return {
          name: "gridCellAnchor",
          value:
            this.swiftUnitPointToken(this.firstPositionalExpression(args), scope) ??
            this.firstPositionalArg(args, scope),
        };
      case "background":
        if (trailingBody !== undefined) {
          return {
            name: "background",
            children: this.parseStatements(trailingBody, scope),
          };
        }
        return { name: "background", value: this.firstPositionalArg(args, scope) };
      case "overlay":
        return {
          name: "overlay",
          value: this.namedArg(args, "alignment")?.replace(/^\./, ""),
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "mask":
        return {
          name: "mask",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "safeAreaInset":
        return {
          name: "safeAreaInset",
          edge: this.namedArg(args, "edge")?.replace(/^\./, ""),
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "contextMenu":
        return {
          name: "contextMenu",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "refreshable": {
        const action = this.parseCmuxAction(trailingBody ?? "", scope);
        return {
          name: "refreshable",
          ...(action !== undefined ? { action } : {}),
        };
      }
      case "swipeActions":
        return {
          name: "swipeActions",
          value: this.namedArg(args, "edge")?.replace(/^\./, ""),
          boolValue:
            this.namedArg(args, "allowsFullSwipe") === undefined
              ? undefined
              : this.evalCondition(this.namedArg(args, "allowsFullSwipe") ?? "true", scope),
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "sheet":
      case "popover":
      case "fullScreenCover":
      case "alert":
      case "confirmationDialog":
        return this.parsePresentationModifier(name, args, trailingBody ?? "", scope);
      case "presentationDetents":
        return { name: "presentationDetents", value: this.presentationDetentsValue(args, scope) };
      case "presentationDragIndicator":
        return {
          name: "presentationDragIndicator",
          value: this.firstPositionalArg(args, scope),
        };
      case "presentationBackground":
        return { name: "presentationBackground", value: this.firstPositionalArg(args, scope) };
      case "presentationCornerRadius":
        return {
          name: "presentationCornerRadius",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "toolbar":
        return {
          name: "toolbar",
          children: this.parseStatements(trailingBody ?? "", scope),
        };
      case "toolbarBackground":
        return {
          name: "toolbarBackground",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "for")?.replace(/^\./, ""),
        };
      case "toolbarColorScheme":
        return {
          name: "toolbarColorScheme",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "for")?.replace(/^\./, ""),
        };
      case "searchable": {
        const binding = this.namedArg(args, "text") ?? this.firstPositionalExpression(args) ?? "";
        return {
          name: "searchable",
          value: this.evalBindingExpression(binding, scope),
          secondaryValue: this.namedArgValue(args, "prompt", scope),
          placement: this.swiftTypeToken(this.namedArg(args, "placement")),
          stateBindingKey: this.stateBindingKey(binding, scope),
        };
      }
      case "navigationTitle":
        return { name: "navigationTitle", value: this.firstPositionalArg(args, scope) };
      case "navigationSubtitle":
        return { name: "navigationSubtitle", value: this.firstPositionalArg(args, scope) };
      case "navigationBarTitleDisplayMode":
        return {
          name: "navigationBarTitleDisplayMode",
          value: this.firstPositionalArg(args, scope),
        };
      case "navigationDestination": {
        const closure = this.splitClosureParameter(trailingBody ?? "");
        return {
          name: "navigationDestination",
          routeParam: closure.params[0] ?? "value",
          routeBody: closure.body,
          routeValueType: this.swiftTypeToken(
            this.namedArg(args, "for") ?? this.splitTopLevel(args, ",")[0],
          ),
        };
      }
      case "keyboardShortcut":
        return {
          name: "keyboardShortcut",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.swiftKeyboardShortcutModifiers(this.namedArg(args, "modifiers"), scope),
        };
      case "contentShape":
        return { name: "contentShape", value: this.swiftShapeToken(args) };
      case "coordinateSpace":
        return {
          name: "coordinateSpace",
          value: this.swiftCoordinateSpaceToken(
            this.namedArg(args, "name") ?? this.firstPositionalExpression(args),
            scope,
          ),
        };
      case "draggable":
        return { name: "draggable", value: this.firstPositionalArg(args, scope) };
      case "dropDestination": {
        const action = this.parseCmuxAction(trailingBody ?? "", scope);
        return action === undefined
          ? null
          : {
              name: "dropDestination",
              value: this.swiftTypeToken(
                this.namedArg(args, "for") ?? this.firstPositionalArg(args, scope),
              ),
              action,
            };
      }
      case "focusable": {
        const focusableArg = this.splitTopLevel(args, ",")[0]?.trim();
        return {
          name: "focusable",
          boolValue: focusableArg === undefined || focusableArg === "" ? true : this.evalCondition(focusableArg, scope),
        };
      }
      case "focused": {
        const bindingArg = this.splitTopLevel(args, ",")[0]?.trim() ?? "";
        const stateBindingKey = this.stateBindingKey(bindingArg, scope);
        return {
          name: "focused",
          value: bindingArg,
          ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
          boolValue: this.evalBindingCondition(bindingArg, scope),
        };
      }
      case "controlSize":
        return { name: "controlSize", value: this.firstPositionalArg(args, scope) };
      case "buttonBorderShape":
        return { name: "buttonBorderShape", value: this.firstPositionalArg(args, scope) };
      case "cornerRadius":
        return {
          name: "cornerRadius",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "containerRelativeFrame": {
        const axis = this.scrollAxis(this.firstPositionalArg(args, scope));
        return {
          name: "containerRelativeFrame",
          value: axis,
          count: this.toNumber(this.namedArg(args, "count"), scope),
          span: this.toNumber(this.namedArg(args, "span"), scope),
          spacing: this.toNumber(this.namedArg(args, "spacing"), scope),
          frameAlignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
        };
      }
      case "frame":
        return {
          name: "frame",
          frameWidth: this.toNumber(this.namedArg(args, "width"), scope),
          frameHeight: this.toNumber(this.namedArg(args, "height"), scope),
          frameMinWidth: this.toNumber(this.namedArg(args, "minWidth"), scope),
          frameMinHeight: this.toNumber(this.namedArg(args, "minHeight"), scope),
          frameMaxWidth: this.toNumber(this.namedArg(args, "maxWidth"), scope),
          frameMaxHeight: this.toNumber(this.namedArg(args, "maxHeight"), scope),
          frameIdealWidth: this.toNumber(this.namedArg(args, "idealWidth"), scope),
          frameIdealHeight: this.toNumber(this.namedArg(args, "idealHeight"), scope),
          frameAlignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
          maxWidthInfinity: this.namedArg(args, "maxWidth") === ".infinity",
        };
      case "alignmentGuide":
        return {
          name: "alignmentGuide",
          value: this.swiftAlignmentGuideToken(this.firstPositionalExpression(args), scope),
          secondaryValue: this.alignmentGuideOffsetValue(trailingBody ?? "", scope),
        };
      case "layoutPriority":
        return {
          name: "layoutPriority",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "offset":
        return {
          name: "offset",
          x: this.toNumber(this.namedArg(args, "x"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "y"), scope) ?? 0,
        };
      case "position":
        return {
          name: "position",
          x: this.toNumber(this.namedArg(args, "x"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "y"), scope) ?? 0,
        };
      case "zIndex":
        return {
          name: "zIndex",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "aspectRatio":
        return {
          name: "aspectRatio",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "contentMode")?.replace(/^\./, ""),
        };
      case "scaledToFit":
        return { name: "aspectRatio", secondaryValue: "fit" };
      case "scaledToFill":
        return { name: "aspectRatio", secondaryValue: "fill" };
      case "clipped":
        return { name: "clipped" };
      case "compositingGroup":
        return { name: "compositingGroup" };
      case "clipShape":
        return {
          name: "clipShape",
          value: this.swiftShapeToken(args),
          ...this.swiftFillStyleMetadata(this.namedArg(args, "style"), scope),
        };
      case "shadow":
        return {
          name: "shadow",
          value: this.namedStringArg(args, "color", scope),
          radius: this.toNumber(this.namedArg(args, "radius"), scope) ?? 8,
          x: this.toNumber(this.namedArg(args, "x"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "y"), scope) ?? 3,
        };
      case "border":
      case "stroke":
        return {
          name: "border",
          value: this.firstPositionalArg(args, scope),
          width: this.toNumber(this.namedArg(args, "width"), scope) ?? 1,
        };
      case "strokeBorder":
        return {
          name: "strokeBorder",
          value: this.firstPositionalArg(args, scope),
          width:
            this.toNumber(this.namedArg(args, "lineWidth"), scope) ??
            this.toNumber(this.namedArg(args, "width"), scope) ??
            1,
        };
      case "trim":
        return {
          name: "trim",
          x: this.toNumber(this.namedArg(args, "from"), scope) ?? 0,
          y: this.toNumber(this.namedArg(args, "to"), scope) ?? 1,
        };
      case "blur":
        return {
          name: "blur",
          radius: this.toNumber(this.namedArg(args, "radius"), scope) ?? 0,
        };
      case "brightness":
      case "contrast":
      case "saturation":
      case "grayscale":
        return {
          name,
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "hueRotation":
        return { name: "hueRotation", value: this.swiftDegrees(args, scope)?.toString() };
      case "blendMode":
        return { name: "blendMode", value: this.firstPositionalArg(args, scope) };
      case "rotationEffect":
        return { name: "rotationEffect", value: this.swiftDegrees(args, scope)?.toString() };
      case "scaleEffect":
        return {
          name: "scaleEffect",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "rotation3DEffect": {
        const axis = this.swiftAxisTuple(this.namedArg(args, "axis"), scope);
        return {
          name: "rotation3DEffect",
          value: this.swiftDegrees(args, scope)?.toString(),
          secondaryValue: this.swiftUnitPointToken(this.namedArg(args, "anchor"), scope),
          x: axis?.x ?? 0,
          y: axis?.y ?? 0,
          z: axis?.z ?? 0,
          perspective: this.toNumber(this.namedArg(args, "perspective"), scope),
        };
      }
      case "visualEffect":
        return {
          name: "visualEffect",
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      case "lineLimit":
        return {
          name: "lineLimit",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
          boolValue:
            this.namedArg(args, "reservesSpace") === undefined
              ? undefined
              : this.evalCondition(this.namedArg(args, "reservesSpace") ?? "false", scope),
        };
      case "truncationMode":
        return { name: "truncationMode", value: this.firstPositionalArg(args, scope) };
      case "multilineTextAlignment":
        return {
          name: "multilineTextAlignment",
          value: this.firstPositionalArg(args, scope),
        };
      case "textCase":
        return { name: "textCase", value: this.firstPositionalArg(args, scope) };
      case "tracking":
      case "kerning":
      case "baselineOffset":
        return {
          name,
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "underline":
      case "strikethrough":
        return this.parseTextDecorationModifier(name, args, scope);
      case "opacity":
        return {
          name: "opacity",
          value: this.toNumber(this.firstPositionalArg(args, scope), scope)?.toString(),
        };
      case "hidden":
        return { name: "hidden", boolValue: true };
      case "fixedSize":
        return { name: "fixedSize" };
      case "badge":
        return { name: "badge", value: this.firstPositionalArg(args, scope) };
      case "allowsHitTesting":
        return {
          name: "allowsHitTesting",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
        };
      case "disabled":
        return { name: "disabled", boolValue: this.evalCondition(args || "true", scope) };
      case "hoverEffect": {
        const isEnabled = this.namedArg(args, "isEnabled");
        return {
          name: "hoverEffect",
          value: this.firstPositionalArg(args, scope) ?? "automatic",
          boolValue: isEnabled === undefined ? true : this.evalCondition(isEnabled, scope),
        };
      }
      case "defaultHoverEffect":
        return {
          name: "defaultHoverEffect",
          value: this.firstPositionalArg(args, scope) ?? "automatic",
        };
      case "help":
        return { name: "help", value: this.firstPositionalArg(args, scope) };
      case "accessibilityLabel":
        return { name: "accessibilityLabel", value: this.firstPositionalArg(args, scope) };
      case "accessibilityHidden":
        return {
          name: "accessibilityHidden",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? args, scope),
        };
      case "accessibilityValue":
        return { name: "accessibilityValue", value: this.firstPositionalArg(args, scope) };
      case "accessibilityHint":
        return { name: "accessibilityHint", value: this.firstPositionalArg(args, scope) };
      case "accessibilityAddTraits":
        return { name: "accessibilityAddTraits", value: this.accessibilityTokenValue(args, scope) };
      case "accessibilityElement":
        return {
          name: "accessibilityElement",
          value: this.accessibilityElementChildrenValue(args, scope),
        };
      case "accessibilityAction":
        return {
          name: "accessibilityAction",
          value: this.accessibilityActionValue(args, scope),
          action: this.parseCmuxAction(trailingBody ?? "", scope),
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      case "accessibilityActivationPoint": {
        const pointExpression = this.firstPositionalExpression(args);
        const point = this.swiftPointValue(pointExpression, args, scope);
        return {
          name: "accessibilityActivationPoint",
          value: this.swiftUnitPointToken(pointExpression, scope),
          ...(point.x !== undefined ? { x: point.x } : {}),
          ...(point.y !== undefined ? { y: point.y } : {}),
        };
      }
      case "accessibilityRepresentation":
        return {
          name: "accessibilityRepresentation",
          children: this.parseStatements(trailingBody ?? "", scope),
          boolValue: trailingBody !== undefined && trailingBody.trim() !== "",
        };
      case "accessibilitySortPriority":
        return {
          name: "accessibilitySortPriority",
          value: String(this.toNumber(this.splitTopLevel(args, ",")[0], scope) ?? 0),
        };
      case "redacted":
        return { name: "redacted", value: this.namedArg(args, "reason") ?? this.firstPositionalArg(args, scope) };
      case "privacySensitive":
        return { name: "privacySensitive" };
      case "unredacted":
        return { name: "unredacted" };
      case "listRowBackground":
        return { name: "listRowBackground", value: this.firstPositionalArg(args, scope) };
      case "listRowSeparator":
        return {
          name: "listRowSeparator",
          value: this.firstPositionalArg(args, scope),
        };
      case "labelsHidden":
        return { name: "labelsHidden", boolValue: true };
      case "labelStyle":
        return { name: "labelStyle", value: this.firstPositionalArg(args, scope) };
      case "listStyle":
        return { name: "listStyle", value: this.firstPositionalArg(args, scope) };
      case "menuStyle":
        return { name: "menuStyle", value: this.firstPositionalArg(args, scope) };
      case "controlGroupStyle":
        return { name: "controlGroupStyle", value: this.firstPositionalArg(args, scope) };
      case "groupBoxStyle":
        return { name: "groupBoxStyle", value: this.firstPositionalArg(args, scope) };
      case "tabViewStyle":
        return { name: "tabViewStyle", value: this.swiftTypeToken(this.splitTopLevel(args, ",")[0]) };
      case "pickerStyle":
        return { name: "pickerStyle", value: this.firstPositionalArg(args, scope) };
      case "toggleStyle":
        return { name: "toggleStyle", value: this.firstPositionalArg(args, scope) };
      case "textFieldStyle":
        return { name: "textFieldStyle", value: this.firstPositionalArg(args, scope) };
      case "scrollContentBackground":
        return {
          name: "scrollContentBackground",
          value: this.firstPositionalArg(args, scope),
        };
      case "scrollIndicators":
        return {
          name: "scrollIndicators",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "axes")?.replace(/^\./, ""),
        };
      case "scrollClipDisabled":
        return {
          name: "scrollClipDisabled",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "scrollTargetBehavior":
        return {
          name: "scrollTargetBehavior",
          value: this.firstPositionalArg(args, scope),
        };
      case "scrollTargetLayout":
        return {
          name: "scrollTargetLayout",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.namedArg(args, "isEnabled") ?? this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "scrollBounceBehavior":
        return {
          name: "scrollBounceBehavior",
          value: this.firstPositionalArg(args, scope),
          secondaryValue: this.namedArg(args, "axes")?.replace(/^\./, ""),
        };
      case "scrollDisabled":
        return {
          name: "scrollDisabled",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? "true", scope),
        };
      case "scrollPosition": {
        const idExpression = this.namedArg(args, "id");
        const bindingKey =
          idExpression === undefined ? undefined : this.stateBindingKey(idExpression, scope);
        const initialAnchor = this.swiftUnitPointToken(this.namedArg(args, "initialAnchor"), scope);
        return {
          name: "scrollPosition",
          value:
            idExpression === undefined
              ? initialAnchor
              : bindingKey ?? this.evalExpression(idExpression, scope),
          secondaryValue:
            this.swiftUnitPointToken(this.namedArg(args, "anchor"), scope) ?? initialAnchor,
          ...(bindingKey !== undefined ? { stateBindingKey: bindingKey } : {}),
        };
      }
      case "defaultScrollAnchor":
        return {
          name: "defaultScrollAnchor",
          value: this.swiftUnitPointToken(this.firstPositionalExpression(args), scope),
        };
      case "preferredColorScheme":
        return {
          name: "preferredColorScheme",
          value: this.swiftColorSchemeToken(this.firstPositionalExpression(args), scope),
        };
      case "environment": {
        const parts = this.splitTopLevel(args, ",");
        const key = this.swiftEnvironmentKeyToken(parts[0], scope);
        if (key === undefined) return null;
        return {
          name: "environment",
          value: key,
          secondaryValue:
            key === "colorScheme"
              ? this.swiftColorSchemeToken(parts[1], scope)
              : this.swiftLayoutDirectionToken(parts[1], scope),
        };
      }
      case "resizable":
        return {
          name: "resizable",
          secondaryValue: this.swiftResizableModeToken(this.namedArg(args, "resizingMode"), scope),
          ...this.swiftEdgeInsetsValue(this.namedArg(args, "capInsets"), scope),
        };
      case "renderingMode":
        return { name: "renderingMode", value: this.firstPositionalArg(args, scope) };
      case "interpolation":
        return { name: "interpolation", value: this.firstPositionalArg(args, scope) };
      case "antialiased":
        return {
          name: "antialiased",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.splitTopLevel(args, ",")[0] ?? args, scope),
        };
      case "flipsForRightToLeftLayoutDirection":
        return {
          name: "flipsForRightToLeftLayoutDirection",
          boolValue:
            args.trim() === ""
              ? true
              : this.evalCondition(this.firstPositionalExpression(args) ?? "true", scope),
        };
      case "imageScale":
        return { name: "imageScale", value: this.firstPositionalArg(args, scope) };
      case "symbolRenderingMode":
        return {
          name: "symbolRenderingMode",
          value: this.firstPositionalArg(args, scope),
        };
      case "symbolVariant":
        return { name: "symbolVariant", value: this.firstPositionalArg(args, scope) };
      default:
        return null;
    }
  }

  private parseEventHandler(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftEventHandler | null {
    const eventName = this.firstPositionalArg(args, scope) || this.namedArgValue(args, "name", scope);
    const eventCategory = this.namedArgValue(args, "category", scope);
    const assignments = this.parseEventAssignments(trailingBody, scope);
    const action = this.parseCmuxAction(trailingBody, scope);
    if (!eventName && !eventCategory) {
      this.warnings.push("Unsupported onEvent handler without a name or category");
      return null;
    }
    if (assignments.length === 0 && action === undefined) {
      this.warnings.push(`Unsupported onEvent handler body '${trailingBody.trim().slice(0, 48)}'`);
      return null;
    }
    return {
      ...(eventName ? { eventName } : {}),
      ...(eventCategory ? { eventCategory } : {}),
      assignments,
      ...(action !== undefined ? { action } : {}),
    };
  }

  private parseLocalHandler(
    body: string,
    scope: SwiftParseScope,
    modifierName: CustomSidebarSwiftLocalHandlerModifierName,
  ): CustomSidebarSwiftLocalHandler | null {
    const assignments = this.parseEventAssignments(body, scope);
    const action = this.parseCmuxAction(body, scope);
    if (assignments.length === 0 && action === undefined) {
      this.warnings.push(`Unsupported ${modifierName} handler body '${body.trim().slice(0, 48)}'`);
      return null;
    }
    return {
      assignments,
      ...(action !== undefined ? { action } : {}),
    };
  }

  private parseEventAssignments(
    body: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftStateAssignment[] {
    const assignments: CustomSidebarSwiftStateAssignment[] = [];
    let index = 0;
    while (index < body.length && assignments.length < 25) {
      index = this.skipWhitespaceAndSeparators(body, index);
      if (index >= body.length) break;
      const statement = this.readSimpleStatement(body, index);
      index = statement.end;
      if (statement.text.trim().startsWith("cmux")) continue;
      const match = statement.text.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([\s\S]+)$/);
      if (!match) continue;
      const key = match[1] ?? "";
      if (scope.__stateKeys?.[key] !== true) continue;
      assignments.push({
        key,
        value: this.swiftStateValue(this.evalValue(match[2] ?? "", scope)),
      });
    }
    return assignments;
  }

  private swiftStateValue(value: unknown): SwiftSidebarStateValue {
    if (
      typeof value === "string" ||
      typeof value === "number" ||
      typeof value === "boolean" ||
      value === null
    ) {
      return value;
    }
    if (Array.isArray(value)) {
      return value.slice(0, 250).map((entry) => this.swiftStateValue(entry));
    }
    if (typeof value === "object" && value !== null) {
      return Object.fromEntries(
        Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
          key,
          this.swiftStateValue(entry),
        ]),
      );
    }
    return value === undefined ? null : String(value);
  }

  private namedArgValue(
    args: string,
    name: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const value = this.namedArg(args, name);
    return value === undefined ? undefined : this.evalExpression(value, scope);
  }

  private parseCmuxAction(
    source: string,
    scope: SwiftParseScope,
  ): CustomSidebarJsonAction | undefined {
    const cmuxIndex = source.indexOf("cmux");
    if (cmuxIndex < 0) return undefined;
    const call = this.readCall(source.slice(cmuxIndex));
    if (call === null || call.name !== "cmux") return undefined;
    const args = this.splitTopLevel(call.args, ",");
    const method = this.evalExpression(args[0] ?? "", scope).trim();
    if (!method) return undefined;
    const params: Record<string, unknown> = {};
    for (const arg of args.slice(1)) {
      const colon = this.topLevelIndexOf(arg, ":");
      if (colon < 0) continue;
      const key = arg.slice(0, colon).trim();
      params[key] = this.evalValue(arg.slice(colon + 1), scope);
    }
    return { method, params };
  }

  private userFunction(
    name: string,
    scope: SwiftParseScope,
  ): SwiftUserFunction | undefined {
    return scope.__functions?.[name];
  }

  private invokeViewFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const functionScope = this.scopeWithFunctionArgs(name, args, scope);
    const nodes = this.parseStatements(
      this.userFunction(name, scope)?.body ?? "",
      functionScope,
    );
    if (nodes.length === 1 && nodes[0] !== undefined) return nodes[0];
    return { kind: "group", children: nodes };
  }

  private invokeValueFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    const fn = this.userFunction(name, scope);
    if (fn === undefined || fn.returnsView) return undefined;
    let currentScope = this.scopeWithFunctionArgs(name, args, scope);
    let index = 0;
    let result: unknown;
    while (index < fn.body.length) {
      index = this.skipWhitespaceAndSeparators(fn.body, index);
      if (index >= fn.body.length) break;
      if (fn.body.startsWith("let ", index) || fn.body.startsWith("let\t", index)) {
        const parsed = this.parseLetBinding(fn.body, index, currentScope);
        currentScope = parsed.scope;
        index = parsed.end;
        continue;
      }
      if (fn.body.startsWith("return ", index) || fn.body.startsWith("return\t", index)) {
        const statement = this.readSimpleStatement(
          fn.body,
          this.skipWhitespaceAndSeparators(fn.body, index + "return".length),
        );
        return this.evalValue(statement.text, currentScope);
      }
      const statement = this.readSimpleStatement(fn.body, index);
      result = this.evalValue(statement.text, currentScope);
      index = statement.end;
    }
    return result;
  }

  private scopeWithFunctionArgs(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): SwiftParseScope {
    const fn = this.userFunction(name, scope);
    if (fn === undefined) return scope;
    let next = { ...scope } as SwiftParseScope;
    const values = this.splitTopLevel(args, ",").map((arg) => {
      const colon = this.topLevelIndexOf(arg, ":");
      return this.evalValue(colon >= 0 ? arg.slice(colon + 1) : arg, scope);
    });
    fn.params.forEach((param, index) => {
      next = this.scopeWithLoopValue(next, param, values[index]);
    });
    return next;
  }

  private parseFunctionParams(params: string): string[] {
    return this.splitTopLevel(params, ",")
      .map((param) => {
        const colon = this.topLevelIndexOf(param, ":");
        const beforeType = param.slice(0, colon < 0 ? param.length : colon).trim();
        const tokens = beforeType.split(/\s+/).filter((token) => token && token !== "_");
        return tokens.at(-1) ?? "";
      })
      .filter(Boolean);
  }

  private evalCondition(condition: string, scope: SwiftParseScope): boolean {
    return this.truthy(this.evalValue(condition, scope));
  }

  private evalBindingCondition(expression: string, scope: SwiftParseScope): boolean {
    const value = this.evalBindingValue(expression, scope);
    if (typeof value === "boolean") return value;
    if (typeof value === "number") return value !== 0;
    if (typeof value === "string") return value !== "" && value !== "false";
    return value !== undefined && value !== null;
  }

  private evalBindingExpression(expression: string, scope: SwiftParseScope): string {
    const value = this.evalBindingValue(expression, scope);
    return value === undefined || value === null ? "" : String(value);
  }

  private evalBindingValue(expression: string, scope: SwiftParseScope): unknown {
    return this.evalValue(this.unwrapBindingExpression(expression), scope);
  }

  private unwrapBindingExpression(expression: string): string {
    let trimmed = expression.trim();
    if (trimmed.startsWith("$")) trimmed = trimmed.slice(1).trim();
    for (const prefix of [".constant", "Binding.constant"]) {
      if (!trimmed.startsWith(`${prefix}(`)) continue;
      const open = trimmed.indexOf("(");
      const balanced = this.readBalanced(trimmed, open, "(", ")");
      if (balanced !== null && balanced.end === trimmed.length) {
        return balanced.content.trim();
      }
    }
    return trimmed;
  }

  private stateBindingKey(
    expression: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const trimmed = expression.trim();
    if (!trimmed.startsWith("$")) return undefined;
    return this.stateBindingPath(trimmed.slice(1).trim(), scope);
  }

  private stateBindingPath(expression: string, scope: SwiftParseScope): string | undefined {
    const first = expression.match(/^([A-Za-z_][A-Za-z0-9_]*)/);
    if (!first) return undefined;
    const firstName = first[1] ?? "";
    const base =
      scope.__stateKeys?.[firstName] === true ? firstName : scope.__bindingKeys?.[firstName];
    if (base === undefined) return undefined;
    let index = first[0].length;
    let key = base;
    while (index < expression.length) {
      const char = expression[index];
      if (char === ".") {
        const member = expression.slice(index + 1).match(/^([A-Za-z_][A-Za-z0-9_]*)/);
        if (!member) return undefined;
        key += `.${member[1] ?? ""}`;
        index += 1 + member[0].length;
        continue;
      }
      if (char === "[") {
        const balanced = this.readBalanced(expression, index, "[", "]");
        if (balanced === null) return undefined;
        const subscript = this.evalValue(balanced.content, scope);
        if (typeof subscript === "number" && Number.isInteger(subscript)) {
          key += `[${subscript}]`;
        } else if (typeof subscript === "string" && /^[A-Za-z_][A-Za-z0-9_]*$/.test(subscript)) {
          key += `.${subscript}`;
        } else if (typeof subscript === "string") {
          key += `[${JSON.stringify(subscript)}]`;
        } else {
          return undefined;
        }
        index = balanced.end;
        continue;
      }
      if (/\s/.test(char ?? "")) {
        index += 1;
        continue;
      }
      return undefined;
    }
    return key;
  }

  private parseRangeBounds(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): { lower: number; upper: number } | undefined {
    if (expression === undefined) return undefined;
    const closed = expression.match(/^(.+?)\s*\.\.\.\s*(.+)$/);
    const halfOpen = expression.match(/^(.+?)\s*\.\.<\s*(.+)$/);
    const range = closed ?? halfOpen;
    if (!range) return undefined;
    const lower = this.toNumber(range[1], scope);
    const upper = this.toNumber(range[2], scope);
    return lower === undefined || upper === undefined ? undefined : { lower, upper };
  }

  private evalExpression(expression: string, scope: SwiftParseScope): string {
    const value = this.evalValue(expression, scope);
    const swiftValue = this.swiftGeometryDisplayValue(value);
    if (swiftValue !== undefined) return swiftValue;
    if (value !== undefined && value !== null) return String(value);
    return expression.trim().replace(/^\./, "");
  }

  private parseText(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const textStyle = this.swiftTextStyleToken(this.namedArg(args, "style"));
    const timerIntervalArg = this.namedArg(args, "timerInterval");
    if (timerIntervalArg !== undefined) {
      const interval = this.evalDateInterval(timerIntervalArg, scope);
      const countsDown = this.evalCondition(this.namedArg(args, "countsDown") ?? "true", scope);
      return {
        kind: "text",
        text:
          interval === undefined
            ? ""
            : this.formatSwiftTimerInterval(interval, countsDown),
        textStyle: "timer",
        ...(interval !== undefined
          ? {
              timerIntervalStartMs: interval.start.epochMs,
              timerIntervalEndMs: interval.end.epochMs,
            }
          : {}),
        timerCountsDown: countsDown,
      };
    }
    const verbatim = this.namedArg(args, "verbatim");
    if (verbatim !== undefined) {
      return { kind: "text", text: this.evalExpression(verbatim, scope) };
    }
    const markdown = this.namedArg(args, "markdown");
    if (markdown !== undefined) {
      const text = this.evalExpression(markdown, scope);
      return { kind: "text", text, markdownRuns: this.parseInlineMarkdown(text) };
    }
    const firstArg = this.splitTopLevel(args, ",").find(
      (arg) => this.topLevelIndexOf(arg, ":") < 0,
    );
    const text = this.evalText(args, scope);
    if (textStyle !== undefined) {
      return { kind: "text", text, textStyle };
    }
    if (
      firstArg !== undefined &&
      this.isStaticSwiftStringLiteral(firstArg) &&
      this.containsInlineMarkdown(text)
    ) {
      return { kind: "text", text, markdownRuns: this.parseInlineMarkdown(text) };
    }
    return { kind: "text", text };
  }

  private parseTextDecorationModifier(
    name: "underline" | "strikethrough",
    args: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftModifier {
    const activeArg = this.splitTopLevel(args, ",").find(
      (arg) => arg.trim() !== "" && this.topLevelIndexOf(arg, ":") < 0,
    );
    return {
      name,
      boolValue:
        activeArg === undefined || activeArg.trim() === ""
          ? true
          : this.evalCondition(activeArg, scope),
      value: this.namedArg(args, "color")?.replace(/^\./, ""),
      secondaryValue: this.namedArg(args, "pattern")?.replace(/^\./, ""),
    };
  }

  private parseLabel(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const titleClosure = this.extractNamedClosure(args, "title");
    const iconClosure = this.extractNamedClosure(args, "icon");
    if (titleClosure !== null || iconClosure !== null) {
      const titleNode = this.parseStatements(titleClosure ?? "", scope).find(
        (node) => node.kind === "text",
      );
      const iconNode = this.parseStatements(iconClosure ?? "", scope).find(
        (node) => node.kind === "image",
      );
      return {
        kind: "label",
        text: titleNode?.kind === "text" ? titleNode.text : "",
        ...(iconNode?.kind === "image" ? { systemImage: iconNode.systemName } : {}),
      };
    }
    return {
      kind: "label",
      text: this.firstPositionalArg(args, scope) ?? "",
      systemImage: this.namedStringArg(args, "systemImage", scope),
    };
  }

  private parseImage(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const systemName = this.namedStringArg(args, "systemName", scope);
    if (systemName !== undefined) {
      return { kind: "image", systemName };
    }
    const assetName =
      this.namedStringArg(args, "decorative", scope) ??
      this.firstPositionalArg(args, scope);
    const name = assetName?.trim() || "missing";
    return {
      kind: "assetImage",
      name,
      url: safeCustomSidebarAssetUrl(scope.root.assets?.[name] ?? ""),
      decorative: this.namedArg(args, "decorative") !== undefined,
    };
  }

  private parseAsyncImage(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const url = this.safeRemoteImageUrl(this.namedArg(args, "url") ?? args, scope);
    const contentClosure =
      this.extractNamedClosure(args, "content") ??
      labeledTrailingClosures.content ??
      trailingBody;
    const placeholderClosure =
      this.extractNamedClosure(args, "placeholder") ??
      labeledTrailingClosures.placeholder ??
      null;
    const base: CustomSidebarSwiftNode = { kind: "asyncImage", url };
    const successChildren =
      contentClosure.trim() === ""
        ? undefined
        : this.parseAsyncImageContentChildren(contentClosure, base, scope);
    const placeholderChildren =
      placeholderClosure === null || placeholderClosure.trim() === ""
        ? undefined
        : this.parseStatements(placeholderClosure, scope);
    return {
      ...base,
      ...(successChildren !== undefined ? { successChildren } : {}),
      ...(placeholderChildren !== undefined ? { placeholderChildren } : {}),
    };
  }

  private parseAsyncImageContentChildren(
    closureBody: string,
    imageNode: CustomSidebarSwiftNode,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode[] {
    const closure = this.splitClosureParameter(closureBody);
    const imageParam = closure.params[0] ?? "image";
    const body = closure.body.trim();
    const imageParamPattern = new RegExp(`^${this.escapeRegExp(imageParam)}\\b`);
    if (imageParamPattern.test(body)) {
      return [this.withModifiers(imageNode, body, imageParam.length, scope)];
    }
    return this.parseStatements(body, scope);
  }

  private parsePath(args: string, scope: SwiftParseScope): CustomSidebarSwiftNode {
    const roundedRectExpression = this.namedArg(args, "roundedRect");
    const ellipseExpression = this.namedArg(args, "ellipseIn");
    const rect =
      this.swiftRectValue(roundedRectExpression, scope) ??
      this.swiftRectValue(ellipseExpression, scope);
    const rectFields =
      rect === undefined
        ? {}
        : {
            pathX: rect.x,
            pathY: rect.y,
            pathWidth: rect.width,
            pathHeight: rect.height,
          };
    if (ellipseExpression !== undefined) {
      return {
        kind: "shape",
        shape: "pathEllipse",
        ...rectFields,
      };
    }
    return {
      kind: "shape",
      shape: "pathRoundedRect",
      radius: this.toNumber(this.namedArg(args, "cornerRadius"), scope),
      ...rectFields,
    };
  }

  private parseLabeledContent(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
  ): CustomSidebarSwiftNode {
    const children = this.parseStatements(trailingBody, scope);
    const valueArg = this.namedArg(args, "value");
    return {
      kind: "labeledContent",
      title: this.firstPositionalArg(args, scope),
      value: valueArg === undefined ? undefined : this.evalExpression(valueArg, scope),
      children,
    };
  }

  private parseGroupBox(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const labelClosure =
      this.extractNamedClosure(args, "label") ??
      labeledTrailingClosures.label ??
      null;
    const contentClosure =
      this.extractNamedClosure(args, "content") ??
      labeledTrailingClosures.content ??
      trailingBody;
    const labelChildren =
      labelClosure === null || labelClosure.trim() === ""
        ? []
        : this.parseStatements(labelClosure, scope);
    return {
      kind: "groupBox",
      title: this.firstPositionalArg(args, scope),
      labelChildren,
      children: this.parseStatements(contentClosure, scope),
    };
  }

  private parseDisclosureGroup(
    args: string,
    trailingBody: string,
    scope: SwiftParseScope,
    labeledTrailingClosures: Record<string, string> = {},
  ): CustomSidebarSwiftNode {
    const isExpandedBinding = this.namedArg(args, "isExpanded");
    const stateBindingKey = this.stateBindingKey(isExpandedBinding ?? "", scope);
    const labelClosure =
      this.extractNamedClosure(args, "label") ??
      labeledTrailingClosures.label ??
      null;
    const contentClosure =
      this.extractNamedClosure(args, "content") ??
      labeledTrailingClosures.content ??
      trailingBody;
    return {
      kind: "disclosureGroup",
      title: this.firstPositionalArg(args, scope),
      isExpanded:
        isExpandedBinding === undefined
          ? true
          : this.evalBindingCondition(isExpandedBinding, scope),
      ...(stateBindingKey !== undefined ? { stateBindingKey } : {}),
      labelChildren:
        labelClosure === null || labelClosure.trim() === ""
          ? []
          : this.parseStatements(labelClosure, scope),
      children: this.parseStatements(contentClosure, scope),
    };
  }

  private evalText(args: string, scope: SwiftParseScope): string {
    const format = this.namedArg(args, "format");
    if (format !== undefined) {
      const valueArg = this.splitTopLevel(args, ",").find(
        (arg) => this.topLevelIndexOf(arg, ":") < 0,
      );
      if (valueArg !== undefined) {
        return this.formatSwiftValue(this.evalValue(valueArg, scope), format, scope);
      }
    }
    const style = this.swiftTextStyleToken(this.namedArg(args, "style"));
    if (style !== undefined) {
      const valueArg = this.firstPositionalExpression(args);
      const value = valueArg === undefined ? undefined : this.evalValue(valueArg, scope);
      if (this.isSwiftDate(value)) {
        return this.formatSwiftTextDateStyle(value, style);
      }
    }
    return this.evalExpression(args, scope);
  }

  private swiftTextStyleToken(expression: string | undefined): string | undefined {
    const token = expression?.trim().replace(/^\./, "");
    if (
      token === "date" ||
      token === "time" ||
      token === "relative" ||
      token === "timer" ||
      token === "offset"
    ) {
      return token;
    }
    return undefined;
  }

  private isStaticSwiftStringLiteral(expression: string): boolean {
    const trimmed = expression.trim();
    return (
      trimmed.startsWith('"') &&
      trimmed.endsWith('"') &&
      !trimmed.includes("\\(")
    );
  }

  private containsInlineMarkdown(text: string): boolean {
    return this.parseInlineMarkdown(text).some((run) => run.kind !== "text");
  }

  private parseInlineMarkdown(text: string): CustomSidebarSwiftTextRun[] {
    const runs: CustomSidebarSwiftTextRun[] = [];
    let scanIndex = 0;
    let textStart = 0;
    const pushText = (end: number) => {
      if (end > textStart) {
        runs.push({ kind: "text", text: text.slice(textStart, end) });
      }
    };

    while (scanIndex < text.length) {
      if (text.startsWith("**", scanIndex)) {
        const end = text.indexOf("**", scanIndex + 2);
        if (end > scanIndex + 2) {
          pushText(scanIndex);
          runs.push({ kind: "strong", text: text.slice(scanIndex + 2, end) });
          scanIndex = end + 2;
          textStart = scanIndex;
          continue;
        }
      }

      if (text[scanIndex] === "`") {
        const end = text.indexOf("`", scanIndex + 1);
        if (end > scanIndex + 1) {
          pushText(scanIndex);
          runs.push({ kind: "code", text: text.slice(scanIndex + 1, end) });
          scanIndex = end + 1;
          textStart = scanIndex;
          continue;
        }
      }

      if (text[scanIndex] === "[") {
        const labelEnd = text.indexOf("]", scanIndex + 1);
        if (labelEnd > scanIndex + 1 && text[labelEnd + 1] === "(") {
          const hrefEnd = text.indexOf(")", labelEnd + 2);
          if (hrefEnd > labelEnd + 2) {
            pushText(scanIndex);
            runs.push({
              kind: "link",
              text: text.slice(scanIndex + 1, labelEnd),
              href: this.safeMarkdownHref(text.slice(labelEnd + 2, hrefEnd)),
            });
            scanIndex = hrefEnd + 1;
            textStart = scanIndex;
            continue;
          }
        }
      }

      if (
        text[scanIndex] === "*" &&
        text[scanIndex - 1] !== "*" &&
        text[scanIndex + 1] !== "*"
      ) {
        const end = this.findSingleAsterisk(text, scanIndex + 1);
        if (end > scanIndex + 1) {
          pushText(scanIndex);
          runs.push({ kind: "emphasis", text: text.slice(scanIndex + 1, end) });
          scanIndex = end + 1;
          textStart = scanIndex;
          continue;
        }
      }

      scanIndex += 1;
    }

    pushText(text.length);
    return runs.length > 0 ? runs : [{ kind: "text", text }];
  }

  private findSingleAsterisk(text: string, startIndex: number): number {
    let index = startIndex;
    while (index < text.length) {
      if (
        text[index] === "*" &&
        text[index - 1] !== "*" &&
        text[index + 1] !== "*"
      ) {
        return index;
      }
      index += 1;
    }
    return -1;
  }

  private safeMarkdownHref(href: string): string {
    return /^https?:\/\//i.test(href) ? href : "#";
  }

  private safeRemoteImageUrl(
    expression: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const unwrapped = this.unwrapUrlInitializer(expression);
    const url = this.evalExpression(unwrapped, scope).trim();
    return /^https?:\/\//i.test(url) ? url : undefined;
  }

  private safeExternalLinkUrl(
    expression: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const unwrapped = this.unwrapUrlInitializer(expression);
    const url = this.evalExpression(unwrapped, scope).trim();
    return /^https?:\/\//i.test(url) ? url : undefined;
  }

  private unwrapUrlInitializer(expression: string): string {
    const trimmed = expression.trim().replace(/[!?]\s*$/, "");
    if (!trimmed.startsWith("URL(")) return trimmed;
    const call = this.readCall(trimmed);
    if (call === null || call.end !== trimmed.length || call.name !== "URL") {
      return trimmed;
    }
    return this.namedArg(call.args, "string") ?? this.splitTopLevel(call.args, ",")[0] ?? "";
  }

  private evalValue(expression: string, scope: SwiftParseScope): unknown {
    const trimmed = this.unwrapParenthesizedExpression(expression.trim());
    const ternary = this.splitTernary(trimmed);
    if (ternary !== null) {
      return this.evalCondition(ternary.condition, scope)
        ? this.evalValue(ternary.truthy, scope)
        : this.evalValue(ternary.falsy, scope);
    }
    const logical = this.evalLogicalExpression(trimmed, scope);
    if (logical !== undefined) return logical;
    if (trimmed.startsWith("!")) return !this.evalCondition(trimmed.slice(1), scope);
    const comparison = this.evalComparisonExpression(trimmed, scope);
    if (comparison !== undefined) return comparison;
    if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
      return this.interpolateSwiftString(this.unquote(trimmed), scope);
    }
    if (trimmed === "nil" || trimmed === "null") return null;
    if (trimmed === "true") return true;
    if (trimmed === "false") return false;
    if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) return Number(trimmed);
    const arithmetic = this.evalArithmeticExpression(trimmed, scope);
    if (arithmetic !== undefined) return arithmetic;
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      const balanced = this.readBalanced(trimmed, 0, "[", "]");
      if (balanced !== null && balanced.end === trimmed.length) {
        return this.evalCollectionLiteral(balanced.content, scope);
      }
    }
    const dottedLiteral = this.evalDottedSwiftGeometryLiteral(trimmed, scope);
    if (dottedLiteral !== undefined) return dottedLiteral;
    const functionCall = this.readCall(trimmed);
    if (functionCall !== null && functionCall.end === trimmed.length) {
      const builtinValue = this.evalBuiltinFunction(
        functionCall.name,
        functionCall.args,
        scope,
      );
      if (builtinValue !== undefined) return builtinValue;
      const functionValue = this.invokeValueFunction(
        functionCall.name,
        functionCall.args,
        scope,
      );
      if (functionValue !== undefined) return functionValue;
    }
    const resolved = this.resolvePath(trimmed, scope);
    if (resolved !== undefined) return resolved;
    return trimmed.replace(/^\./, "");
  }

  private unwrapParenthesizedExpression(expression: string): string {
    let trimmed = expression.trim();
    while (trimmed.startsWith("(")) {
      const balanced = this.readBalanced(trimmed, 0, "(", ")");
      if (balanced === null || balanced.end !== trimmed.length) break;
      trimmed = balanced.content.trim();
    }
    return trimmed;
  }

  private evalLogicalExpression(expression: string, scope: SwiftParseScope): boolean | undefined {
    const orIndex = this.topLevelOperatorIndex(expression, ["||"]);
    if (orIndex >= 0) {
      return (
        this.evalCondition(expression.slice(0, orIndex), scope) ||
        this.evalCondition(expression.slice(orIndex + 2), scope)
      );
    }
    const andIndex = this.topLevelOperatorIndex(expression, ["&&"]);
    if (andIndex >= 0) {
      return (
        this.evalCondition(expression.slice(0, andIndex), scope) &&
        this.evalCondition(expression.slice(andIndex + 2), scope)
      );
    }
    return undefined;
  }

  private evalComparisonExpression(
    expression: string,
    scope: SwiftParseScope,
  ): boolean | undefined {
    const operators = ["==", "!=", ">=", "<=", ">", "<"];
    const index = this.topLevelOperatorIndex(expression, operators);
    if (index < 0) return undefined;
    const operator = operators.find((candidate) => expression.startsWith(candidate, index));
    if (operator === undefined) return undefined;
    const left = this.evalComparisonOperand(expression.slice(0, index), scope);
    const right = this.evalComparisonOperand(expression.slice(index + operator.length), scope);
    const nullishEqual = left == null && right == null;
    if (operator === "==") return nullishEqual || left === right;
    if (operator === "!=") return !(nullishEqual || left === right);
    const leftNumber = Number(left);
    const rightNumber = Number(right);
    const comparable =
      Number.isFinite(leftNumber) && Number.isFinite(rightNumber)
        ? [leftNumber, rightNumber]
        : [String(left), String(right)];
    switch (operator) {
      case ">=":
        return comparable[0] >= comparable[1];
      case "<=":
        return comparable[0] <= comparable[1];
      case ">":
        return comparable[0] > comparable[1];
      case "<":
        return comparable[0] < comparable[1];
      default:
        return undefined;
    }
  }

  private evalArithmeticExpression(expression: string, scope: SwiftParseScope): unknown {
    const additiveIndex = this.topLevelOperatorIndex(expression, ["+", "-"], true);
    if (additiveIndex >= 0) {
      const operator = expression[additiveIndex];
      const left = this.evalValue(expression.slice(0, additiveIndex), scope);
      const right = this.evalValue(expression.slice(additiveIndex + 1), scope);
      if (operator === "+" && (typeof left === "string" || typeof right === "string")) {
        return `${left ?? ""}${right ?? ""}`;
      }
      const leftNumber = Number(left);
      const rightNumber = Number(right);
      if (!Number.isFinite(leftNumber) || !Number.isFinite(rightNumber)) return undefined;
      return operator === "+" ? leftNumber + rightNumber : leftNumber - rightNumber;
    }
    const multiplicativeIndex = this.topLevelOperatorIndex(expression, ["*", "/", "%"], true);
    if (multiplicativeIndex >= 0) {
      const operator = expression[multiplicativeIndex];
      const left = Number(this.evalValue(expression.slice(0, multiplicativeIndex), scope));
      const right = Number(this.evalValue(expression.slice(multiplicativeIndex + 1), scope));
      if (!Number.isFinite(left) || !Number.isFinite(right)) return undefined;
      if ((operator === "/" || operator === "%") && right === 0) return 0;
      if (operator === "*") return left * right;
      return operator === "/" ? left / right : left % right;
    }
    return undefined;
  }

  private truthy(value: unknown): boolean {
    if (typeof value === "boolean") return value;
    if (typeof value === "number") return value !== 0;
    if (typeof value === "string") return value !== "" && value !== "false";
    return value !== undefined && value !== null;
  }

  private evalComparisonOperand(expression: string, scope: SwiftParseScope): unknown {
    const trimmed = this.unwrapParenthesizedExpression(expression.trim());
    if (trimmed === "nil" || trimmed === "null") return null;
    const directPath =
      /^[A-Za-z_][A-Za-z0-9_]*(?:[.\[].*)?$/.test(trimmed) &&
      this.readCall(trimmed)?.end !== trimmed.length;
    if (directPath) {
      return this.resolvePath(trimmed, scope);
    }
    return this.evalValue(trimmed, scope);
  }

  private resolvePath(path: string, scope: SwiftParseScope): unknown {
    const normalized = path.trim();
    if (!normalized) return undefined;
    if (/^-?\d+(?:\.\d+)?$/.test(normalized)) return Number(normalized);
    if (normalized === "true") return true;
    if (normalized === "false") return false;
    const arrayCall = this.readCall(normalized);
    if (arrayCall?.name === "Array" && arrayCall.end === normalized.length) {
      return this.resolvePath(arrayCall.args, scope);
    }
    const parts = this.splitTopLevel(normalized, ".").filter(Boolean);
    let value: unknown = this.resolvePathPart(parts[0] ?? "", scope);
    for (const part of parts.slice(1)) {
      value = this.resolveMember(value, part, scope);
    }
    return value;
  }

  private resolvePathPart(part: string, scope: SwiftParseScope): unknown {
    const call = this.readCall(part);
    if (call !== null && call.end === part.trim().length) {
      return this.evalBuiltinFunction(call.name, call.args, scope);
    }
    const bracket = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\[(.+)\]$/);
    if (bracket) {
      const base =
        (scope as unknown as Record<string, unknown>)[bracket[1] ?? ""] ??
        (scope.root as unknown as Record<string, unknown>)[bracket[1] ?? ""];
      return this.resolveSubscript(base, bracket[2] ?? "", scope);
    }
    return (
      (scope as unknown as Record<string, unknown>)[part] ??
      (scope.root as unknown as Record<string, unknown>)[part]
    );
  }

  private resolveMember(value: unknown, part: string, scope: SwiftParseScope): unknown {
    if (Array.isArray(value)) {
      if (part === "count") return value.length;
      if (part === "indices") return value.map((_entry, index) => index);
      if (part === "first") return value[0];
      if (part === "last") return value.at(-1);
      const callWithTrailingClosure = part.match(
        /^([A-Za-z_][A-Za-z0-9_]*)\(([\s\S]*)\)\s*\{([\s\S]*)\}$/,
      );
      if (callWithTrailingClosure) {
        return this.resolveArrayMethod(
          value,
          callWithTrailingClosure[1] ?? "",
          `${callWithTrailingClosure[2] ?? ""}, {${callWithTrailingClosure[3] ?? ""}}`,
          scope,
        );
      }
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method) {
        if (method[1] === "formatted") {
          return this.formatSwiftValue(value, method[2] ?? "", scope);
        }
        return this.resolveArrayMethod(value, method[1] ?? "", method[2] ?? "", scope);
      }
      const closureMethod = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*\{([\s\S]*)\}$/);
      if (closureMethod) {
        return this.resolveArrayMethod(
          value,
          closureMethod[1] ?? "",
          `{${closureMethod[2] ?? ""}}`,
          scope,
        );
      }
    }
    if (typeof value === "number") {
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method?.[1] === "formatted") {
        return this.formatSwiftValue(value, method[2] ?? "", scope);
      }
    }
    if (typeof value === "string") {
      if (part === "count") return Array.from(value).length;
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method) {
        return this.resolveStringMethod(value, method[1] ?? "", method[2] ?? "", scope);
      }
    }
    if (typeof value !== "object" || value === null) return undefined;
    if (this.isSwiftDate(value)) {
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method?.[1] === "formatted") {
        return this.formatSwiftValue(value, method[2] ?? "", scope);
      }
    }
    if (this.isSwiftMeasurement(value)) {
      const method = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/);
      if (method?.[1] === "formatted") {
        return this.formatSwiftValue(value, method[2] ?? "", scope);
      }
    }
    if (this.isSwiftAngle(value)) {
      if (part === "degrees") return value.degrees;
      if (part === "radians") return value.degrees * (Math.PI / 180);
    }
    if (this.isSwiftPoint(value) || this.isSwiftUnitPoint(value)) {
      if (part === "x") return value.x;
      if (part === "y") return value.y;
    }
    if (this.isSwiftSize(value)) {
      if (part === "width") return value.width;
      if (part === "height") return value.height;
    }
    if (this.isSwiftRect(value)) {
      if (part === "x" || part === "minX") return value.x;
      if (part === "y" || part === "minY") return value.y;
      if (part === "width") return value.width;
      if (part === "height") return value.height;
      if (part === "maxX") return value.x + value.width;
      if (part === "maxY") return value.y + value.height;
      if (part === "midX") return value.x + value.width / 2;
      if (part === "midY") return value.y + value.height / 2;
      if (part === "origin") return { __swiftPoint: true, x: value.x, y: value.y };
      if (part === "size") return {
        __swiftSize: true,
        width: value.width,
        height: value.height,
      };
    }
    const bracket = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\[(.+)\]$/);
    if (bracket) {
      const child = (value as Record<string, unknown>)[bracket[1] ?? ""];
      return this.resolveSubscript(child, bracket[2] ?? "", scope);
    }
    return (value as Record<string, unknown>)[part];
  }

  private swiftKeyPathParts(expression: string | undefined): string[] | undefined {
    const trimmed = expression?.trim();
    if (trimmed === undefined || trimmed === "") return undefined;
    const withoutRoot = trimmed.replace(/^\\\./, "");
    if (withoutRoot === trimmed) return undefined;
    if (withoutRoot === "self") return [];
    if (!/^[A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)*$/.test(withoutRoot)) {
      return undefined;
    }
    return withoutRoot.split(".");
  }

  private resolveKeyPathValue(
    value: unknown,
    parts: string[],
    scope: SwiftParseScope,
  ): unknown {
    return parts.reduce(
      (current, part) => this.resolveMember(current, part, scope),
      value,
    );
  }

  private compareProjectedValues(left: unknown, right: unknown): number {
    if (typeof left === "number" && typeof right === "number") {
      return left - right;
    }
    if (typeof left === "boolean" && typeof right === "boolean") {
      return Number(left) - Number(right);
    }
    if (left === undefined || left === null) return right === undefined || right === null ? 0 : 1;
    if (right === undefined || right === null) return -1;
    return String(left).localeCompare(String(right));
  }

  private arrayProjection(
    args: string,
    scope: SwiftParseScope,
  ): ((entry: unknown, index: number) => unknown) | undefined {
    const keyPath = this.swiftKeyPathParts(
      this.namedArg(args, "by") ?? this.firstPositionalExpression(args),
    );
    if (keyPath !== undefined) {
      return (entry) => this.resolveKeyPathValue(entry, keyPath, scope);
    }
    const closure = this.arrayMethodClosure(args);
    if (closure === null) return undefined;
    return (entry, index) =>
      this.evalValue(
        closure.body,
        this.scopeWithClosureValues(scope, closure.params, [entry, index]),
      );
  }

  private arrayComparator(
    args: string,
    scope: SwiftParseScope,
  ): ((left: unknown, right: unknown) => number) | undefined {
    const keyPath = this.swiftKeyPathParts(
      this.namedArg(args, "by") ?? this.firstPositionalExpression(args),
    );
    if (keyPath !== undefined) {
      return (left, right) =>
        this.compareProjectedValues(
          this.resolveKeyPathValue(left, keyPath, scope),
          this.resolveKeyPathValue(right, keyPath, scope),
        );
    }
    const closure = this.arrayMethodClosure(args);
    if (closure === null) return undefined;
    return (left, right) => {
      const leftFirst = this.scopeWithClosureValues(scope, closure.params, [left, right]);
      if (this.evalCondition(closure.body, leftFirst)) return -1;
      const rightFirst = this.scopeWithClosureValues(scope, closure.params, [right, left]);
      if (this.evalCondition(closure.body, rightFirst)) return 1;
      return 0;
    };
  }

  private evalCollectionLiteral(content: string, scope: SwiftParseScope): unknown {
    const entries = this.splitTopLevel(content, ",").filter((entry) => entry.trim() !== "");
    const hasDictionaryEntry = entries.some((entry) => this.topLevelIndexOf(entry, ":") >= 0);
    if (!hasDictionaryEntry) {
      return entries.map((entry) => this.evalValue(entry, scope));
    }
    const record: Record<string, unknown> = {};
    for (const entry of entries) {
      const colon = this.topLevelIndexOf(entry, ":");
      if (colon < 0) continue;
      const key = this.evalValue(entry.slice(0, colon), scope);
      record[String(key)] = this.evalValue(entry.slice(colon + 1), scope);
    }
    return record;
  }

  private resolveSubscript(
    value: unknown,
    expression: string,
    scope: SwiftParseScope,
  ): unknown {
    const key = this.evalValue(expression, scope);
    if (Array.isArray(value)) {
      const index = typeof key === "number" ? key : Number(key);
      return Number.isInteger(index) ? value[index] : undefined;
    }
    if (typeof value === "object" && value !== null) {
      return (value as Record<string, unknown>)[String(key)];
    }
    if (typeof value === "string") {
      const index = typeof key === "number" ? key : Number(key);
      return Number.isInteger(index) ? Array.from(value)[index] : undefined;
    }
    return undefined;
  }

  private resolveArrayMethod(
    value: unknown[],
    method: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    const count = this.toNumber(this.splitTopLevel(args, ",")[0], scope);
    switch (method) {
      case "filter": {
        const closure = this.arrayMethodClosure(args);
        if (closure === null) return undefined;
        return value.filter((entry, index) =>
          this.evalCondition(
            closure.body,
            this.scopeWithClosureValues(scope, closure.params, [entry, index]),
          ),
        );
      }
      case "map": {
        const projection = this.arrayProjection(args, scope);
        return projection === undefined
          ? undefined
          : value.map((entry, index) => projection(entry, index));
      }
      case "compactMap": {
        const projection = this.arrayProjection(args, scope);
        return projection === undefined
          ? undefined
          : value
          .map((entry, index) => projection(entry, index))
          .filter((entry) => entry !== undefined && entry !== null);
      }
      case "flatMap": {
        const projection = this.arrayProjection(args, scope);
        if (projection === undefined) return undefined;
        return value.flatMap((entry, index) => {
          const mapped = projection(entry, index);
          return Array.isArray(mapped) ? mapped : [mapped];
        });
      }
      case "reduce": {
        const initial = this.reduceInitialValue(args, scope);
        const closure = this.arrayMethodClosure(args);
        if (closure === null) return undefined;
        return value.reduce(
          (total, entry, index) =>
            this.evalValue(
              closure.body,
              this.scopeWithClosureValues(scope, closure.params, [total, entry, index]),
            ),
          initial,
        );
      }
      case "sorted": {
        const entries = value.slice();
        const comparator = this.arrayComparator(args, scope);
        return comparator === undefined
          ? entries.sort((left, right) => String(left).localeCompare(String(right)))
          : entries.sort(comparator);
      }
      case "min": {
        if (value.length === 0) return undefined;
        const comparator = this.arrayComparator(args, scope);
        if (comparator === undefined) {
          return value
            .slice()
            .sort((left, right) => this.compareProjectedValues(left, right))[0];
        }
        return value.slice().sort(comparator)[0];
      }
      case "max": {
        if (value.length === 0) return undefined;
        const comparator = this.arrayComparator(args, scope);
        if (comparator === undefined) {
          return value
            .slice()
            .sort((left, right) => this.compareProjectedValues(right, left))[0];
        }
        return value.slice().sort((left, right) => comparator(right, left))[0];
      }
      case "allSatisfy": {
        const projection = this.arrayProjection(args, scope);
        if (projection === undefined) return undefined;
        return value.every((entry, index) => this.truthy(projection(entry, index)));
      }
      case "prefix":
        return value.slice(0, Math.max(0, Math.floor(count ?? value.length)));
      case "suffix": {
        const length = Math.max(0, Math.floor(count ?? value.length));
        return length === 0 ? [] : value.slice(-length);
      }
      case "dropFirst":
        return value.slice(Math.max(0, Math.floor(count ?? 1)));
      case "dropLast": {
        const length = Math.max(0, Math.floor(count ?? 1));
        return length === 0 ? value.slice() : value.slice(0, -length);
      }
      case "reversed":
        return value.slice().reverse();
      case "enumerated":
        return value.map((element, offset) => ({ offset, element }));
      case "contains": {
        const needle = this.evalExpression(this.splitTopLevel(args, ",")[0] ?? "", scope);
        return value.some((entry) => String(entry) === needle);
      }
      default:
        return undefined;
    }
  }

  private reduceInitialValue(args: string, scope: SwiftParseScope): unknown {
    const initialArg =
      this.splitTopLevel(args, ",").find((arg) => !arg.trim().startsWith("{")) ?? "";
    const trimmed = initialArg.trim();
    if (trimmed.startsWith("into:")) {
      return this.evalValue(trimmed.slice("into:".length), scope);
    }
    return this.evalValue(trimmed, scope);
  }

  private arrayMethodClosure(
    args: string,
  ): { params: string[]; body: string } | null {
    const trimmed = args.trim();
    if (!trimmed) return null;
    const closureSource = trimmed.startsWith("{")
      ? trimmed
      : this.splitTopLevel(trimmed, ",").find((arg) => arg.trim().startsWith("{"));
    if (closureSource === undefined) return null;
    const open = closureSource.indexOf("{");
    const balanced = this.readBalanced(closureSource, open, "{", "}");
    if (balanced === null) return null;
    return this.splitClosureParameter(balanced.content);
  }

  private scopeWithClosureValues(
    scope: SwiftParseScope,
    params: string[],
    values: unknown[],
  ): SwiftParseScope {
    let next = { ...scope } as SwiftParseScope;
    values.forEach((value, index) => {
      next = { ...next, [`$${index}`]: value } as SwiftParseScope;
    });
    params.forEach((param, index) => {
      next = this.scopeWithLoopValue(next, param, values[index]);
    });
    return next;
  }

  private resolveStringMethod(
    value: string,
    method: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    const firstArg = this.splitTopLevel(args, ",")[0] ?? "";
    switch (method) {
      case "hasPrefix":
        return value.startsWith(this.evalExpression(firstArg, scope));
      case "hasSuffix":
        return value.endsWith(this.evalExpression(firstArg, scope));
      case "contains":
        return value.includes(this.evalExpression(firstArg, scope));
      case "uppercased":
        return value.toUpperCase();
      case "lowercased":
        return value.toLowerCase();
      case "split": {
        const separator =
          this.namedStringArg(args, "separator", scope) ??
          this.evalExpression(firstArg, scope);
        return value.split(separator);
      }
      default:
        return undefined;
    }
  }

  private formatSwiftValue(value: unknown, format: string, scope: SwiftParseScope): string {
    if (value === undefined || value === null) return "";
    if (Array.isArray(value) && format.includes("list")) {
      return this.formatSwiftList(value, format);
    }
    if (this.isSwiftDate(value)) {
      return this.formatSwiftDate(value, format);
    }
    if (this.isSwiftMeasurement(value)) {
      return this.formatSwiftMeasurement(value, format);
    }
    const numericValue = Number(value);
    if (Number.isFinite(numericValue)) {
      return this.formatSwiftNumber(numericValue, format, scope);
    }
    return String(value);
  }

  private formatSwiftList(value: unknown[], format: string): string {
    const type = format.includes(".or") || /type:\s*\.or/.test(format)
      ? "disjunction"
      : format.includes(".unit") || /type:\s*\.unit/.test(format)
        ? "unit"
        : "conjunction";
    const style = format.includes(".narrow") || /width:\s*\.narrow/.test(format)
      ? "narrow"
      : format.includes(".short") || /width:\s*\.short/.test(format)
        ? "short"
        : "long";
    return new Intl.ListFormat("en-US", { type, style }).format(
      value.map((entry) => (entry === undefined || entry === null ? "" : String(entry))),
    );
  }

  private formatSwiftNumber(value: number, format: string, scope: SwiftParseScope): string {
    const source = format.trim();
    if (source.includes("byteCount")) {
      return this.formatSwiftByteCount(value, source);
    }
    if (source.includes("currency")) {
      const call = this.readCall(source.replace(/^\./, ""));
      const currency =
        call?.name === "currency"
          ? (this.namedStringArg(call.args, "code", scope) ?? "USD")
          : "USD";
      return new Intl.NumberFormat("en-US", {
        style: "currency",
        currency,
      }).format(value);
    }
    if (source.includes("percent")) {
      return new Intl.NumberFormat("en-US", {
        style: "percent",
        maximumFractionDigits: 1,
      }).format(value);
    }
    if (source.includes("compactName") || source.includes("compact")) {
      return new Intl.NumberFormat("en-US", {
        notation: "compact",
        maximumFractionDigits: 1,
      }).format(value);
    }
    return new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 3,
    }).format(value);
  }

  private formatSwiftByteCount(value: number, format: string): string {
    const binary = /\.memory\b|\.binary\b|countStyle:\s*\.binary/.test(format);
    const base = binary ? 1024 : 1000;
    const units = binary
      ? ["bytes", "KiB", "MiB", "GiB", "TiB"]
      : ["bytes", "KB", "MB", "GB", "TB"];
    const sign = value < 0 ? "-" : "";
    let size = Math.abs(value);
    let unitIndex = 0;
    while (size >= base && unitIndex < units.length - 1) {
      size /= base;
      unitIndex += 1;
    }
    if (unitIndex === 0) {
      const bytes = Math.trunc(size);
      return `${sign}${bytes} ${bytes === 1 ? "byte" : "bytes"}`;
    }
    const formatted = new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 1,
    }).format(size);
    return `${sign}${formatted} ${units[unitIndex]}`;
  }

  private evalMeasurementInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftMeasurementValue | undefined {
    const value = this.toNumber(this.namedArg(args, "value"), scope);
    const unitArg = this.namedArg(args, "unit");
    if (value === undefined || unitArg === undefined) return undefined;
    return {
      __swiftMeasurement: true,
      value,
      unit: this.swiftMeasurementUnitToken(unitArg),
    };
  }

  private isSwiftMeasurement(value: unknown): value is SwiftMeasurementValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftMeasurementValue).__swiftMeasurement === true &&
      typeof (value as SwiftMeasurementValue).value === "number" &&
      typeof (value as SwiftMeasurementValue).unit === "string"
    );
  }

  private isSwiftAngle(value: unknown): value is SwiftAngleValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftAngleValue).__swiftAngle === true &&
      typeof (value as SwiftAngleValue).degrees === "number"
    );
  }

  private isSwiftUnitPoint(value: unknown): value is SwiftUnitPointValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftUnitPointValue).__swiftUnitPoint === true &&
      typeof (value as SwiftUnitPointValue).x === "number" &&
      typeof (value as SwiftUnitPointValue).y === "number"
    );
  }

  private isSwiftPoint(value: unknown): value is SwiftPointValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftPointValue).__swiftPoint === true &&
      typeof (value as SwiftPointValue).x === "number" &&
      typeof (value as SwiftPointValue).y === "number"
    );
  }

  private isSwiftSize(value: unknown): value is SwiftSizeValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftSizeValue).__swiftSize === true &&
      typeof (value as SwiftSizeValue).width === "number" &&
      typeof (value as SwiftSizeValue).height === "number"
    );
  }

  private isSwiftRect(value: unknown): value is SwiftRectValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftRectValue).__swiftRect === true &&
      typeof (value as SwiftRectValue).x === "number" &&
      typeof (value as SwiftRectValue).y === "number" &&
      typeof (value as SwiftRectValue).width === "number" &&
      typeof (value as SwiftRectValue).height === "number"
    );
  }

  private swiftGeometryDisplayValue(value: unknown): string | undefined {
    if (this.isSwiftAngle(value)) return String(value.degrees);
    if (this.isSwiftUnitPoint(value)) return value.token ?? `${value.x},${value.y}`;
    if (this.isSwiftPoint(value)) return `${value.x},${value.y}`;
    if (this.isSwiftSize(value)) return `${value.width}x${value.height}`;
    if (this.isSwiftRect(value)) return `${value.x},${value.y},${value.width}x${value.height}`;
    return undefined;
  }

  private swiftMeasurementUnitToken(expression: string): string {
    return expression
      .trim()
      .replace(/^\./, "")
      .replace(/^(?:UnitLength|UnitDuration|UnitInformationStorage|UnitMass|UnitTemperature)\./, "");
  }

  private formatSwiftMeasurement(measurement: SwiftMeasurementValue, format: string): string {
    const unit = this.swiftMeasurementUnitLabel(
      measurement.unit,
      format.includes(".wide") || /width:\s*\.wide/.test(format),
      measurement.value,
    );
    const value = new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 1,
    }).format(measurement.value);
    return `${value} ${unit}`;
  }

  private swiftMeasurementUnitLabel(unit: string, wide: boolean, value: number): string {
    const singular = Math.abs(value) === 1;
    const labels: Record<string, { abbreviated: string; singular: string; plural: string }> = {
      bytes: { abbreviated: "bytes", singular: "byte", plural: "bytes" },
      kilobytes: { abbreviated: "KB", singular: "kilobyte", plural: "kilobytes" },
      megabytes: { abbreviated: "MB", singular: "megabyte", plural: "megabytes" },
      gigabytes: { abbreviated: "GB", singular: "gigabyte", plural: "gigabytes" },
      seconds: { abbreviated: "s", singular: "second", plural: "seconds" },
      minutes: { abbreviated: "min", singular: "minute", plural: "minutes" },
      hours: { abbreviated: "hr", singular: "hour", plural: "hours" },
      meters: { abbreviated: "m", singular: "meter", plural: "meters" },
      kilometers: { abbreviated: "km", singular: "kilometer", plural: "kilometers" },
      miles: { abbreviated: "mi", singular: "mile", plural: "miles" },
      feet: { abbreviated: "ft", singular: "foot", plural: "feet" },
      grams: { abbreviated: "g", singular: "gram", plural: "grams" },
      kilograms: { abbreviated: "kg", singular: "kilogram", plural: "kilograms" },
      celsius: { abbreviated: "C", singular: "degree Celsius", plural: "degrees Celsius" },
      fahrenheit: { abbreviated: "F", singular: "degree Fahrenheit", plural: "degrees Fahrenheit" },
    };
    const label = labels[unit] ?? {
      abbreviated: unit,
      singular: unit,
      plural: unit,
    };
    return wide ? (singular ? label.singular : label.plural) : label.abbreviated;
  }

  private evalDateInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftDateValue | undefined {
    if (args.trim() === "") {
      return { __swiftDate: true, epochMs: Date.now() };
    }
    const unixSeconds = this.toNumber(this.namedArg(args, "timeIntervalSince1970"), scope);
    if (unixSeconds !== undefined) {
      return { __swiftDate: true, epochMs: unixSeconds * 1000 };
    }
    const referenceSeconds = this.toNumber(this.namedArg(args, "timeIntervalSinceReferenceDate"), scope);
    if (referenceSeconds !== undefined) {
      return { __swiftDate: true, epochMs: (978_307_200 + referenceSeconds) * 1000 };
    }
    return undefined;
  }

  private isSwiftDate(value: unknown): value is SwiftDateValue {
    return (
      typeof value === "object" &&
      value !== null &&
      (value as SwiftDateValue).__swiftDate === true &&
      typeof (value as SwiftDateValue).epochMs === "number"
    );
  }

  private evalDateInterval(
    expression: string,
    scope: SwiftParseScope,
  ): SwiftDateIntervalValue | undefined {
    const trimmed = expression.trim();
    const dateIntervalCall = this.readCall(trimmed);
    if (dateIntervalCall?.name === "DateInterval" && dateIntervalCall.end === trimmed.length) {
      const start = this.evalValue(this.namedArg(dateIntervalCall.args, "start") ?? "", scope);
      const end = this.evalValue(this.namedArg(dateIntervalCall.args, "end") ?? "", scope);
      return this.isSwiftDate(start) && this.isSwiftDate(end) ? { start, end } : undefined;
    }
    const delimiter =
      this.topLevelIndexOf(trimmed, "...") >= 0
        ? "..."
        : this.topLevelIndexOf(trimmed, "..<") >= 0
          ? "..<"
          : undefined;
    if (delimiter === undefined) return undefined;
    const [startExpression, endExpression] = this.splitTopLevel(trimmed, delimiter);
    const start = this.evalValue(startExpression ?? "", scope);
    const end = this.evalValue(endExpression ?? "", scope);
    return this.isSwiftDate(start) && this.isSwiftDate(end) ? { start, end } : undefined;
  }

  private formatSwiftTextDateStyle(value: SwiftDateValue, style: string): string {
    if (style === "time") return this.formatSwiftDate(value, ".time");
    if (style === "relative" || style === "offset") {
      return this.formatSwiftRelativeDuration(value.epochMs - Date.now());
    }
    if (style === "timer") {
      return this.formatSwiftDuration(value.epochMs - Date.now());
    }
    return this.formatSwiftDate(value, ".date");
  }

  private formatSwiftDate(value: SwiftDateValue, format: string): string {
    const source = format.trim();
    const date = new Date(value.epochMs);
    if (Number.isNaN(date.getTime())) return "";
    const wantsTime = /(?:\.time\b|hour|minute|second)/.test(source);
    const wantsDate =
      !wantsTime || /(?:\.date\b|year|month|day|weekday)/.test(source);
    if (wantsDate && wantsTime) {
      return `${this.formatSwiftDatePart(date)}, ${this.formatSwiftTimePart(date)}`;
    }
    if (wantsTime) return this.formatSwiftTimePart(date);
    return this.formatSwiftDatePart(date);
  }

  private formatSwiftDatePart(date: Date): string {
    return new Intl.DateTimeFormat("en-US", {
      timeZone: "UTC",
      year: "numeric",
      month: "short",
      day: "numeric",
    }).format(date);
  }

  private formatSwiftTimePart(date: Date): string {
    return new Intl.DateTimeFormat("en-US", {
      timeZone: "UTC",
      hour: "numeric",
      minute: "2-digit",
    }).format(date);
  }

  private formatSwiftTimerInterval(
    interval: SwiftDateIntervalValue,
    countsDown: boolean,
  ): string {
    const delta = countsDown
      ? interval.end.epochMs - interval.start.epochMs
      : interval.start.epochMs - interval.end.epochMs;
    return this.formatSwiftDuration(delta);
  }

  private formatSwiftDuration(deltaMs: number): string {
    const totalSeconds = Math.max(0, Math.round(Math.abs(deltaMs) / 1000));
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours > 0) {
      return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
    }
    return `${minutes}:${String(seconds).padStart(2, "0")}`;
  }

  private formatSwiftRelativeDuration(deltaMs: number): string {
    const future = deltaMs >= 0;
    const totalSeconds = Math.max(0, Math.round(Math.abs(deltaMs) / 1000));
    const units: Array<[label: string, seconds: number]> = [
      ["day", 86_400],
      ["hour", 3_600],
      ["minute", 60],
      ["second", 1],
    ];
    const [unit, unitSeconds] =
      units.find(([, seconds]) => totalSeconds >= seconds) ?? units.at(-1)!;
    const value = Math.max(1, Math.round(totalSeconds / unitSeconds));
    const label = `${value} ${unit}${value === 1 ? "" : "s"}`;
    return future ? `in ${label}` : `${label} ago`;
  }

  private evalBuiltinFunction(
    name: string,
    args: string,
    scope: SwiftParseScope,
  ): unknown {
    if (name === "String") {
      const formatted = this.evalStringFormatInitializer(args, scope);
      if (formatted !== undefined) return formatted;
    }
    if (name === "Dictionary") {
      return this.evalDictionaryInitializer(args, scope);
    }
    if (name === "Measurement") {
      return this.evalMeasurementInitializer(args, scope);
    }
    if (name === "Date") {
      return this.evalDateInitializer(args, scope);
    }
    if (name === "GridItem") {
      return this.evalGridItemInitializer(args, scope);
    }
    if (name === "Angle") {
      return this.evalAngleInitializer(args, scope);
    }
    if (name === "UnitPoint") {
      return this.evalUnitPointInitializer(args, scope);
    }
    if (name === "CGPoint") {
      return this.evalPointInitializer(args, scope);
    }
    if (name === "CGSize") {
      return this.evalSizeInitializer(args, scope);
    }
    if (name === "CGRect") {
      return this.evalRectInitializer(args, scope);
    }
    const values = this.splitTopLevel(args, ",").map((arg) => this.evalValue(arg, scope));
    switch (name) {
      case "min": {
        const numbers = values.map(Number).filter(Number.isFinite);
        return numbers.length === 0 ? undefined : Math.min(...numbers);
      }
      case "max": {
        const numbers = values.map(Number).filter(Number.isFinite);
        return numbers.length === 0 ? undefined : Math.max(...numbers);
      }
      case "abs": {
        const value = Number(values[0]);
        return Number.isFinite(value) ? Math.abs(value) : undefined;
      }
      case "Int": {
        const value = Number(values[0]);
        return Number.isFinite(value) ? Math.trunc(value) : undefined;
      }
      case "Double": {
        const value = Number(values[0]);
        return Number.isFinite(value) ? value : undefined;
      }
      case "String":
        return values[0] === undefined || values[0] === null ? "" : String(values[0]);
      default:
        return undefined;
    }
  }

  private evalDottedSwiftGeometryLiteral(
    expression: string,
    scope: SwiftParseScope,
  ): unknown {
    const match = expression.match(
      /^(Angle|UnitPoint|CGPoint|CGSize|CGRect)\.([A-Za-z_][A-Za-z0-9_]*)(?:\(([\s\S]*)\))?$/,
    );
    if (!match) return undefined;
    const typeName = match[1] ?? "";
    const member = match[2] ?? "";
    const args = match[3] ?? "";
    if (typeName === "Angle") {
      if (member === "degrees") return this.swiftAngleFromDegrees(this.toNumber(args, scope));
      if (member === "radians") return this.swiftAngleFromRadians(this.toNumber(args, scope));
    }
    if (typeName === "UnitPoint") {
      return this.swiftUnitPointFromToken(member);
    }
    if (member === "zero") {
      if (typeName === "CGPoint") return { __swiftPoint: true, x: 0, y: 0 };
      if (typeName === "CGSize") return { __swiftSize: true, width: 0, height: 0 };
      if (typeName === "CGRect") return { __swiftRect: true, x: 0, y: 0, width: 0, height: 0 };
    }
    return undefined;
  }

  private evalAngleInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftAngleValue | undefined {
    return (
      this.swiftAngleFromDegrees(this.toNumber(this.namedArg(args, "degrees"), scope)) ??
      this.swiftAngleFromRadians(this.toNumber(this.namedArg(args, "radians"), scope)) ??
      this.swiftAngleFromDegrees(this.toNumber(this.firstPositionalExpression(args), scope))
    );
  }

  private swiftAngleFromDegrees(value: number | undefined): SwiftAngleValue | undefined {
    return value === undefined ? undefined : { __swiftAngle: true, degrees: value };
  }

  private swiftAngleFromRadians(value: number | undefined): SwiftAngleValue | undefined {
    return value === undefined
      ? undefined
      : { __swiftAngle: true, degrees: value * (180 / Math.PI) };
  }

  private evalUnitPointInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftUnitPointValue | undefined {
    const x = this.toNumber(this.namedArg(args, "x"), scope);
    const y = this.toNumber(this.namedArg(args, "y"), scope);
    return x === undefined || y === undefined
      ? undefined
      : {
          __swiftUnitPoint: true,
          x,
          y,
          token: this.swiftUnitPointTokenFromCoordinates(x, y),
        };
  }

  private swiftUnitPointFromToken(token: string): SwiftUnitPointValue | undefined {
    const coordinates: Record<string, [number, number]> = {
      center: [0.5, 0.5],
      top: [0.5, 0],
      bottom: [0.5, 1],
      leading: [0, 0.5],
      trailing: [1, 0.5],
      topLeading: [0, 0],
      topTrailing: [1, 0],
      bottomLeading: [0, 1],
      bottomTrailing: [1, 1],
    };
    const point = coordinates[token];
    return point === undefined
      ? undefined
      : { __swiftUnitPoint: true, x: point[0], y: point[1], token };
  }

  private swiftUnitPointTokenFromCoordinates(x: number, y: number): string | undefined {
    const pairs: Array<[string, number, number]> = [
      ["center", 0.5, 0.5],
      ["top", 0.5, 0],
      ["bottom", 0.5, 1],
      ["leading", 0, 0.5],
      ["trailing", 1, 0.5],
      ["topLeading", 0, 0],
      ["topTrailing", 1, 0],
      ["bottomLeading", 0, 1],
      ["bottomTrailing", 1, 1],
    ];
    return pairs.find(([, pointX, pointY]) => pointX === x && pointY === y)?.[0];
  }

  private evalPointInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftPointValue | undefined {
    const x = this.toNumber(this.namedArg(args, "x"), scope) ?? 0;
    const y = this.toNumber(this.namedArg(args, "y"), scope) ?? 0;
    return { __swiftPoint: true, x, y };
  }

  private evalSizeInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftSizeValue | undefined {
    const width = this.toNumber(this.namedArg(args, "width"), scope) ?? 0;
    const height = this.toNumber(this.namedArg(args, "height"), scope) ?? 0;
    return { __swiftSize: true, width, height };
  }

  private evalRectInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftRectValue | undefined {
    const origin = this.evalValue(this.namedArg(args, "origin") ?? "", scope);
    const size = this.evalValue(this.namedArg(args, "size") ?? "", scope);
    const x = this.isSwiftPoint(origin)
      ? origin.x
      : (this.toNumber(this.namedArg(args, "x"), scope) ?? 0);
    const y = this.isSwiftPoint(origin)
      ? origin.y
      : (this.toNumber(this.namedArg(args, "y"), scope) ?? 0);
    const width = this.isSwiftSize(size)
      ? size.width
      : (this.toNumber(this.namedArg(args, "width"), scope) ?? 0);
    const height = this.isSwiftSize(size)
      ? size.height
      : (this.toNumber(this.namedArg(args, "height"), scope) ?? 0);
    return { __swiftRect: true, x, y, width, height };
  }

  private evalGridItemInitializer(
    args: string,
    scope: SwiftParseScope,
  ): SwiftGridItemValue | undefined {
    const sizeExpression = this.firstPositionalExpression(args) ?? ".flexible()";
    const size = this.swiftGridItemSize(sizeExpression, scope);
    if (size === undefined) return undefined;
    return {
      __swiftGridItem: true,
      ...size,
      spacing: this.toNumber(this.namedArg(args, "spacing"), scope),
      alignment: this.swiftAlignmentToken(this.namedArg(args, "alignment"), scope),
    };
  }

  private swiftGridItemSize(
    expression: string,
    scope: SwiftParseScope,
  ):
    | Pick<SwiftGridItemValue, "size" | "minimum" | "maximum">
    | undefined {
    const trimmed = expression.trim();
    const match = trimmed.match(
      /^(?:\.|GridItem\.Size\.)?(fixed|flexible|adaptive)\s*(?:\(([\s\S]*)\))?$/,
    );
    if (!match) return undefined;
    const size = match[1] as SwiftGridItemValue["size"];
    const args = match[2] ?? "";
    if (size === "fixed") {
      return {
        size,
        minimum: this.toNumber(this.firstPositionalExpression(args), scope),
      };
    }
    if (size === "adaptive") {
      return {
        size,
        minimum:
          this.toNumber(this.namedArg(args, "minimum"), scope) ??
          this.toNumber(this.firstPositionalExpression(args), scope),
        maximum: this.toNumber(this.namedArg(args, "maximum"), scope),
      };
    }
    return {
      size,
      minimum: this.toNumber(this.namedArg(args, "minimum"), scope),
      maximum: this.toNumber(this.namedArg(args, "maximum"), scope),
    };
  }

  private evalStringFormatInitializer(
    args: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const formatArg = this.namedArg(args, "format");
    if (formatArg === undefined) return undefined;
    const format = this.evalExpression(formatArg, scope);
    const positionalValues = this.splitTopLevel(args, ",")
      .filter((arg) => this.topLevelIndexOf(arg, ":") < 0)
      .map((arg) => this.evalValue(arg, scope));
    const argumentList = this.namedArg(args, "arguments");
    const listedValues =
      argumentList === undefined ? [] : this.evalValue(argumentList, scope);
    const values = Array.isArray(listedValues)
      ? [...positionalValues, ...listedValues]
      : [...positionalValues];
    return this.formatSwiftString(format, values);
  }

  private evalDictionaryInitializer(
    args: string,
    scope: SwiftParseScope,
  ): Record<string, unknown[]> | undefined {
    const groupingExpression =
      this.namedArg(args, "grouping") ?? this.firstPositionalExpression(args);
    if (groupingExpression === undefined) return undefined;
    const groupedValue = this.evalValue(groupingExpression, scope);
    if (!Array.isArray(groupedValue)) return undefined;
    const byExpression = this.namedArg(args, "by");
    if (byExpression === undefined) return undefined;
    const projection = this.arrayProjection(byExpression, scope);
    if (projection === undefined) return undefined;
    const record: Record<string, unknown[]> = {};
    groupedValue.forEach((entry, index) => {
      const key = projection(entry, index);
      const stringKey = key === undefined || key === null ? "" : String(key);
      record[stringKey] = [...(record[stringKey] ?? []), entry];
    });
    return record;
  }

  private formatSwiftString(format: string, values: unknown[]): string {
    let output = "";
    let formatIndex = 0;
    let valueIndex = 0;
    while (formatIndex < format.length) {
      const percentIndex = format.indexOf("%", formatIndex);
      if (percentIndex < 0) {
        output += format.slice(formatIndex);
        break;
      }
      output += format.slice(formatIndex, percentIndex);
      if (format[percentIndex + 1] === "%") {
        output += "%";
        formatIndex = percentIndex + 2;
        continue;
      }
      const specifier = format.slice(percentIndex).match(
        /^%([-+0 #]*)(\d+)?(?:\.(\d+))?([@sdifuoxXfFeEgG])/,
      );
      if (specifier === null) {
        output += "%";
        formatIndex = percentIndex + 1;
        continue;
      }
      output += this.formatSwiftSpecifier(
        values[valueIndex],
        specifier[4] ?? "@",
        specifier[1] ?? "",
        specifier[2],
        specifier[3],
      );
      valueIndex += 1;
      formatIndex = percentIndex + specifier[0].length;
    }
    return output;
  }

  private formatSwiftSpecifier(
    value: unknown,
    specifier: string,
    flags: string,
    width: string | undefined,
    precision: string | undefined,
  ): string {
    const numberValue = Number(value);
    const precisionValue = precision === undefined ? undefined : Number(precision);
    let formatted: string;
    switch (specifier) {
      case "d":
      case "i":
      case "u":
        formatted = Number.isFinite(numberValue) ? String(Math.trunc(numberValue)) : "0";
        break;
      case "o":
        formatted = Number.isFinite(numberValue) ? Math.trunc(numberValue).toString(8) : "0";
        break;
      case "x":
      case "X":
        formatted = Number.isFinite(numberValue) ? Math.trunc(numberValue).toString(16) : "0";
        formatted = specifier === "X" ? formatted.toUpperCase() : formatted;
        break;
      case "f":
      case "F":
        formatted = Number.isFinite(numberValue)
          ? numberValue.toFixed(Number.isFinite(precisionValue) ? precisionValue : 6)
          : "nan";
        formatted = specifier === "F" ? formatted.toUpperCase() : formatted;
        break;
      case "e":
      case "E":
        formatted = Number.isFinite(numberValue)
          ? numberValue.toExponential(Number.isFinite(precisionValue) ? precisionValue : 6)
          : "nan";
        formatted = specifier === "E" ? formatted.toUpperCase() : formatted;
        break;
      case "g":
      case "G":
        formatted = Number.isFinite(numberValue)
          ? numberValue.toPrecision(Number.isFinite(precisionValue) ? precisionValue : 6)
          : "nan";
        formatted = specifier === "G" ? formatted.toUpperCase() : formatted;
        break;
      case "@":
      case "s":
      default:
        formatted = value === undefined || value === null ? "" : String(value);
        break;
    }
    const widthValue = width === undefined ? undefined : Number(width);
    if (widthValue === undefined || !Number.isFinite(widthValue) || formatted.length >= widthValue) {
      return formatted;
    }
    const padChar = flags.includes("0") && !flags.includes("-") ? "0" : " ";
    const padding = padChar.repeat(widthValue - formatted.length);
    return flags.includes("-") ? `${formatted}${padding}` : `${padding}${formatted}`;
  }

  private interpolateSwiftString(value: string, scope: SwiftParseScope): string {
    let output = "";
    let index = 0;
    while (index < value.length) {
      if (value[index] === "\\" && value[index + 1] === "(") {
        const balanced = this.readBalanced(value, index + 1, "(", ")");
        if (balanced !== null) {
          output += this.evalExpression(balanced.content, scope);
          index = balanced.end;
          continue;
        }
      }
      output += value[index] ?? "";
      index += 1;
    }
    return output;
  }

  private evalSequence(sequence: string, scope: SwiftParseScope): unknown[] | null {
    const trimmed = sequence.trim();
    const halfOpen = trimmed.match(/^(.+?)\s*\.\.<\s*(.+)$/);
    const closed = trimmed.match(/^(.+?)\s*\.\.\.\s*(.+)$/);
    const range = halfOpen ?? closed;
    if (range) {
      const start = this.toNumber(range[1], scope);
      const end = this.toNumber(range[2], scope);
      if (start === undefined || end === undefined) return null;
      const limit = closed ? end + 1 : end;
      const values: number[] = [];
      for (let value = start; value < limit && values.length < 250; value += 1) {
        values.push(value);
      }
      return values;
    }
    const resolved = this.resolvePath(trimmed, scope);
    if (Array.isArray(resolved)) return resolved;
    return null;
  }

  private scopeWithLoopValue(
    scope: SwiftParseScope,
    param: string,
    value: unknown,
  ): SwiftParseScope {
    const next = { ...scope, [param]: value } as SwiftParseScope;
    if (this.isWorkspacePreview(value)) {
      return { ...next, workspace: value };
    }
    return next;
  }

  private scopeWithLoopValues(
    scope: SwiftParseScope,
    params: string[],
    value: unknown,
  ): SwiftParseScope {
    if (params.length <= 1) {
      return this.scopeWithLoopValue(scope, params[0] ?? "item", value);
    }
    const pair =
      typeof value === "object" && value !== null
        ? (value as { offset?: unknown; element?: unknown })
        : {};
    let next = { ...scope } as SwiftParseScope;
    next = this.scopeWithLoopValue(next, params[0] ?? "offset", pair.offset);
    next = this.scopeWithLoopValue(next, params[1] ?? "element", pair.element);
    return next;
  }

  private scopeWithForEachValues(
    scope: SwiftParseScope,
    params: string[],
    value: unknown,
    bindingCollectionKey: string | undefined,
    valueIndex: number,
  ): SwiftParseScope {
    const next = this.scopeWithLoopValues(scope, params, value);
    if (bindingCollectionKey === undefined || params.length > 1) return next;
    const param = params[0] ?? "item";
    return {
      ...next,
      __bindingKeys: {
        ...(next.__bindingKeys ?? {}),
        [param]: `${bindingCollectionKey}[${valueIndex}]`,
      },
    } as SwiftParseScope;
  }

  private isWorkspacePreview(value: unknown): value is WorkspacePreview {
    return (
      typeof value === "object" &&
      value !== null &&
      typeof (value as WorkspacePreview).id === "string" &&
      typeof (value as WorkspacePreview).title === "string"
    );
  }

  private firstPositionalArg(args: string, scope: SwiftParseScope): string | undefined {
    const first = this.splitTopLevel(args, ",").find(
      (arg) => this.topLevelIndexOf(arg, ":") < 0,
    );
    return first === undefined ? undefined : this.evalExpression(first, scope);
  }

  private firstPositionalExpression(args: string): string | undefined {
    return this.splitTopLevel(args, ",").find((arg) => this.topLevelIndexOf(arg, ":") < 0)?.trim();
  }

  private swiftListIdToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^\\?\./, "")
      .trim();
    return token === "" ? undefined : token;
  }

  private presentationDetentsValue(args: string, scope: SwiftParseScope): string | undefined {
    const trimmed = args.trim();
    if (!trimmed) return undefined;
    const list = trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1) : trimmed;
    const detents = this.splitTopLevel(list, ",")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) =>
        String(this.evalExpression(part, scope) ?? part)
          .replace(/^(?:\.|PresentationDetent\.)/, "")
          .trim(),
      )
      .filter(Boolean);
    return detents.length === 0 ? undefined : detents.join(",");
  }

  private accessibilityTokenValue(args: string, scope: SwiftParseScope): string | undefined {
    const trimmed = args.trim();
    if (!trimmed) return undefined;
    const list = trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1) : trimmed;
    const tokens = this.splitTopLevel(list, ",")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) =>
        String(this.evalExpression(part, scope) ?? part)
          .replace(/^(?:\.|AccessibilityTraits\.)/, "")
          .trim(),
      )
      .filter(Boolean);
    return tokens.length === 0 ? undefined : tokens.join(",");
  }

  private accessibilityElementChildrenValue(
    args: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const children = this.namedArg(args, "children") ?? this.firstPositionalArg(args, scope);
    return children
      ?.replace(/^(?:\.|AccessibilityChildBehavior\.)/, "")
      .trim();
  }

  private accessibilityActionValue(args: string, scope: SwiftParseScope): string {
    const expression = this.namedArg(args, "named") ?? this.firstPositionalExpression(args);
    if (expression === undefined || expression.trim() === "") return "default";
    const trimmed = expression.trim();
    const textCall = this.readCall(trimmed);
    if (textCall?.name === "Text" && textCall.end === trimmed.length) {
      return this.firstPositionalArg(textCall.args, scope) ?? "named";
    }
    const value = this.evalExpression(trimmed, scope).replace(
      /^(?:\.|AccessibilityActionKind\.)/,
      "",
    );
    return value.trim() === "" ? "default" : value.trim();
  }

  private swiftKeyboardShortcutModifiers(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("[") && trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed;
    const modifiers = new Set<string>();
    for (const part of this.splitTopLevel(source, ",")) {
      const token = this.evalExpression(part, scope)
        .replace(/^(?:\.|EventModifiers\.)/, "")
        .trim();
      switch (token) {
        case "command":
        case "cmd":
          modifiers.add("command");
          break;
        case "shift":
          modifiers.add("shift");
          break;
        case "option":
        case "alt":
          modifiers.add("option");
          break;
        case "control":
        case "ctrl":
          modifiers.add("control");
          break;
        default:
          break;
      }
    }
    return modifiers.size === 0 ? undefined : [...modifiers].join(",");
  }

  private namedArg(args: string | undefined, label: string): string | undefined {
    if (args === undefined) return undefined;
    for (const arg of this.splitTopLevel(args, ",")) {
      const colon = this.topLevelIndexOf(arg, ":");
      if (colon < 0) continue;
      if (arg.slice(0, colon).trim() === label) {
        return arg.slice(colon + 1).trim();
      }
    }
    return undefined;
  }

  private namedStringArg(
    args: string,
    label: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const value = this.namedArg(args, label);
    return value === undefined ? undefined : this.evalExpression(value, scope);
  }

  private toNumber(expression: string | undefined, scope: SwiftParseScope): number | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const resolved = this.evalValue(expression.trim(), scope);
    const value = this.isSwiftAngle(resolved) ? resolved.degrees : Number(resolved);
    return Number.isFinite(value) ? value : undefined;
  }

  private swiftDegrees(args: string, scope: SwiftParseScope): number | undefined {
    const expression = this.firstPositionalExpression(args);
    if (expression !== undefined) {
      const value = this.evalValue(expression, scope);
      if (this.isSwiftAngle(value)) return value.degrees;
    }
    const degrees = args.match(/\.degrees\(([^)]+)\)/);
    if (degrees) return this.toNumber(degrees[1], scope);
    const radians = args.match(/\.radians\(([^)]+)\)/);
    if (radians) return this.swiftAngleFromRadians(this.toNumber(radians[1], scope))?.degrees;
    return this.toNumber(this.firstPositionalArg(args, scope), scope);
  }

  private swiftAxisTuple(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): { x: number; y: number; z: number } | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("(") && trimmed.endsWith(")")
      ? trimmed.slice(1, -1)
      : trimmed;
    const parts = this.splitTopLevel(source, ",");
    return {
      x: this.toNumber(this.namedArg(source, "x") ?? parts[0], scope) ?? 0,
      y: this.toNumber(this.namedArg(source, "y") ?? parts[1], scope) ?? 0,
      z: this.toNumber(this.namedArg(source, "z") ?? parts[2], scope) ?? 0,
    };
  }

  private swiftUnitPointToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const value = this.evalValue(expression, scope);
    if (this.isSwiftUnitPoint(value)) return value.token;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|UnitPoint\.)/, "")
      .trim();
    switch (token) {
      case "center":
      case "top":
      case "bottom":
      case "leading":
      case "trailing":
      case "topLeading":
      case "topTrailing":
      case "bottomLeading":
      case "bottomTrailing":
        return token;
      default:
        return undefined;
    }
  }

  private swiftPointValue(
    expression: string | undefined,
    args: string,
    scope: SwiftParseScope,
  ): { x?: number; y?: number } {
    const source = expression?.trim();
    const value = source === undefined ? undefined : this.evalValue(source, scope);
    if (this.isSwiftPoint(value) || this.isSwiftUnitPoint(value)) {
      return { x: value.x, y: value.y };
    }
    const call = source === undefined ? null : this.readCall(source);
    const callArgs =
      call !== null && (call.name === "CGPoint" || call.name === "init") && call.end === source?.length
        ? call.args
        : undefined;
    const x = this.toNumber(
      this.namedArg(callArgs, "x") ?? this.namedArg(args, "x"),
      scope,
    );
    const y = this.toNumber(
      this.namedArg(callArgs, "y") ?? this.namedArg(args, "y"),
      scope,
    );
    return {
      ...(x !== undefined ? { x } : {}),
      ...(y !== undefined ? { y } : {}),
    };
  }

  private swiftRectValue(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): SwiftRectValue | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const value = this.evalValue(expression, scope);
    return this.isSwiftRect(value) ? value : undefined;
  }

  private swiftCoordinateSpaceToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith(".") ? trimmed.slice(1) : trimmed;
    const namedCall = this.readCall(source);
    if (namedCall?.name === "named" && namedCall.end === source.length) {
      return this.firstPositionalArg(namedCall.args, scope);
    }
    const token = this.evalExpression(trimmed, scope)
      .replace(/^(?:\.|CoordinateSpace\.)/, "")
      .trim();
    return token === "" ? undefined : token;
  }

  private swiftAlignmentGuideToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|HorizontalAlignment\.|VerticalAlignment\.)/, "")
      .trim();
    switch (token) {
      case "leading":
      case "center":
      case "trailing":
      case "top":
      case "bottom":
      case "firstTextBaseline":
      case "lastTextBaseline":
        return token;
      default:
        return token === "" ? undefined : token;
    }
  }

  private alignmentGuideOffsetValue(
    trailingBody: string,
    scope: SwiftParseScope,
  ): string | undefined {
    const body = this.splitClosureParameter(trailingBody).body.trim();
    if (body === "") return undefined;
    const returnMatch = body.match(/^return\s+([\s\S]+)$/);
    const expression = returnMatch?.[1] ?? body;
    const value = this.toNumber(expression, scope);
    return value === undefined ? undefined : String(value);
  }

  private swiftEdgeInsetsValue(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): Pick<
    CustomSidebarSwiftModifier,
    "capInsetTop" | "capInsetLeading" | "capInsetBottom" | "capInsetTrailing"
  > {
    if (expression === undefined || expression.trim() === "") return {};
    const trimmed = expression.trim();
    const call = this.readCall(trimmed);
    const source = call?.name === "EdgeInsets" || call?.name === ".init" ? call.args : trimmed;
    const top = this.toNumber(this.namedArg(source, "top"), scope);
    const leading = this.toNumber(this.namedArg(source, "leading"), scope);
    const bottom = this.toNumber(this.namedArg(source, "bottom"), scope);
    const trailing = this.toNumber(this.namedArg(source, "trailing"), scope);
    return {
      ...(top !== undefined ? { capInsetTop: top } : {}),
      ...(leading !== undefined ? { capInsetLeading: leading } : {}),
      ...(bottom !== undefined ? { capInsetBottom: bottom } : {}),
      ...(trailing !== undefined ? { capInsetTrailing: trailing } : {}),
    };
  }

  private swiftPaddingInsetsValue(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): Pick<
    CustomSidebarSwiftModifier,
    "paddingTop" | "paddingLeading" | "paddingBottom" | "paddingTrailing"
  > {
    if (expression === undefined || expression.trim() === "") return {};
    const trimmed = expression.trim();
    const call = this.readCall(trimmed);
    if (call?.name !== "EdgeInsets" && call?.name !== ".init") return {};
    const top = this.toNumber(this.namedArg(call.args, "top"), scope);
    const leading = this.toNumber(this.namedArg(call.args, "leading"), scope);
    const bottom = this.toNumber(this.namedArg(call.args, "bottom"), scope);
    const trailing = this.toNumber(this.namedArg(call.args, "trailing"), scope);
    return {
      ...(top !== undefined ? { paddingTop: top } : {}),
      ...(leading !== undefined ? { paddingLeading: leading } : {}),
      ...(bottom !== undefined ? { paddingBottom: bottom } : {}),
      ...(trailing !== undefined ? { paddingTrailing: trailing } : {}),
    };
  }

  private swiftContentMarginPlacementToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|ContentMarginPlacement\.)/, "")
      .trim();
    switch (token) {
      case "automatic":
      case "scrollContent":
      case "scrollIndicators":
        return token;
      default:
        return undefined;
    }
  }

  private swiftResizableModeToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|Image\.ResizingMode\.)/, "")
      .trim();
    switch (token) {
      case "stretch":
      case "tile":
        return token;
      default:
        return undefined;
    }
  }

  private swiftColorSchemeToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|ColorScheme\.)/, "")
      .trim();
    return token === "dark" || token === "light" ? token : undefined;
  }

  private swiftLayoutDirectionToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|LayoutDirection\.)/, "")
      .trim();
    switch (token) {
      case "rightToLeft":
      case "rtl":
        return "rightToLeft";
      case "leftToRight":
      case "ltr":
        return "leftToRight";
      default:
        return undefined;
    }
  }

  private swiftEnvironmentKeyToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): "colorScheme" | "layoutDirection" | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\\?\.|EnvironmentValues\.)/, "")
      .trim();
    switch (token) {
      case "colorScheme":
      case "layoutDirection":
        return token;
      default:
        return undefined;
    }
  }

  private swiftEnvironmentReadKeyToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): "colorScheme" | "layoutDirection" | "locale" | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\\?\.|EnvironmentValues\.)/, "")
      .trim();
    switch (token) {
      case "colorScheme":
      case "layoutDirection":
      case "locale":
        return token;
      default:
        return undefined;
    }
  }

  private swiftEnvironmentReadValue(
    key: "colorScheme" | "layoutDirection" | "locale",
  ): unknown {
    switch (key) {
      case "colorScheme":
        return "light";
      case "layoutDirection":
        return "leftToRight";
      case "locale":
        return {
          identifier: "en-US",
          languageCode: "en",
        };
      default:
        return undefined;
    }
  }

  private swiftShapeToken(args: string): string | undefined {
    const token = args.match(/\b(Circle|Capsule|Rectangle|RoundedRectangle)\s*\(/);
    return token?.[1]?.replace(/^./, (char) => char.toLowerCase());
  }

  private swiftRoundedCornerStyleToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|RoundedCornerStyle\.)/, "")
      .trim();
    return token === "continuous" || token === "circular" ? token : undefined;
  }

  private swiftFillStyleMetadata(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): Pick<CustomSidebarSwiftModifier, "fillStyle" | "antialiased"> {
    if (expression === undefined || expression.trim() === "") return {};
    const trimmed = expression.trim();
    const call = this.readCall(trimmed);
    const source = call?.name === "FillStyle" || call?.name === ".init" ? call.args : trimmed;
    const eoFill = this.namedArg(source, "eoFill");
    const antialiased = this.namedArg(source, "antialiased");
    return {
      ...(eoFill !== undefined && this.evalCondition(eoFill, scope) === true
        ? { fillStyle: "eoFill" }
        : {}),
      ...(antialiased !== undefined
        ? { antialiased: this.evalCondition(antialiased, scope) }
        : {}),
    };
  }

  private swiftEdgeSet(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string[] | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("[") && trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed;
    const edges = new Set<string>();
    for (const part of this.splitTopLevel(source, ",")) {
      const token = this.evalExpression(part, scope)
        .replace(/^(?:\.|Edge\.Set\.)/, "")
        .trim();
      switch (token) {
        case "all":
          edges.add("top");
          edges.add("trailing");
          edges.add("bottom");
          edges.add("leading");
          break;
        case "horizontal":
          edges.add("leading");
          edges.add("trailing");
          break;
        case "vertical":
          edges.add("top");
          edges.add("bottom");
          break;
        case "top":
        case "bottom":
        case "leading":
        case "trailing":
          edges.add(token);
          break;
        default:
          break;
      }
    }
    return edges.size === 0 ? undefined : [...edges];
  }

  private swiftAlignmentToken(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const token = this.evalExpression(expression, scope)
      .replace(/^(?:\.|Alignment\.)/, "")
      .trim();
    switch (token) {
      case "leading":
      case "trailing":
      case "center":
      case "top":
      case "bottom":
      case "topLeading":
      case "topTrailing":
      case "bottomLeading":
      case "bottomTrailing":
        return token;
      default:
        return undefined;
    }
  }

  private swiftPinnedViews(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): string | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const trimmed = expression.trim();
    const source = trimmed.startsWith("[") && trimmed.endsWith("]")
      ? trimmed.slice(1, -1)
      : trimmed;
    const pinned = new Set<string>();
    for (const part of this.splitTopLevel(source, ",")) {
      const token = this.evalExpression(part, scope)
        .replace(/^(?:\.|PinnedScrollableViews\.)/, "")
        .trim();
      switch (token) {
        case "sectionHeaders":
        case "sectionFooters":
          pinned.add(token);
          break;
        default:
          break;
      }
    }
    return pinned.size === 0 ? undefined : [...pinned].join(",");
  }

  private swiftGridItems(
    expression: string | undefined,
    scope: SwiftParseScope,
  ): SwiftGridItemValue[] | undefined {
    if (expression === undefined || expression.trim() === "") return undefined;
    const value = this.evalValue(expression, scope);
    if (!Array.isArray(value)) return undefined;
    const items = value.filter(isSwiftGridItem);
    return items.length === 0 ? undefined : items;
  }

  private readViewStatement(
    body: string,
    index: number,
  ): { text: string; end: number } | null {
    const call = this.readViewCall(body.slice(index));
    if (call === null) return null;
    let end = index + call.end;
    if (
      call.name === "AsyncImage" ||
      call.name === "GroupBox" ||
      call.name === "DisclosureGroup" ||
      call.name === "ContentUnavailableView"
    ) {
      const sequence = this.readTrailingClosureSequence(body.slice(end));
      if (sequence.end > 0) {
        end += sequence.end;
      }
    } else {
      const trailing = this.readTrailingClosure(body.slice(end));
      if (trailing !== null) end += trailing.end;
    }
    while (true) {
      const next = this.skipWhitespaceAndSeparators(body, end);
      if (body[next] !== ".") break;
      const modifier = this.readModifierCall(body.slice(next + 1));
      if (modifier === null) break;
      end = next + 1 + modifier.end;
      const modifierTrailing = this.readTrailingClosure(body.slice(end));
      if (modifierTrailing !== null) end += modifierTrailing.end;
    }
    const next = this.skipWhitespaceAndSeparators(body, end);
    if (body[next] === "+") {
      const rhs = this.readViewStatement(body, next + 1);
      if (rhs !== null) end = rhs.end;
    }
    return { text: body.slice(index, end), end };
  }

  private readSimpleStatement(
    body: string,
    index: number,
  ): { text: string; end: number } {
    let depth = 0;
    let inString = false;
    for (let i = index; i < body.length; i += 1) {
      const char = body[i];
      if (char === '"' && body[i - 1] !== "\\") inString = !inString;
      if (inString) continue;
      if (char === "(" || char === "[" || char === "{") depth += 1;
      if (char === ")" || char === "]" || char === "}") depth -= 1;
      if (depth === 0 && (char === "\n" || char === ";")) {
        return { text: body.slice(index, i).trim(), end: i + 1 };
      }
    }
    return { text: body.slice(index).trim(), end: body.length };
  }

  private readCall(text: string): { name: string; args: string; end: number } | null {
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(/);
    if (!match) return null;
    const open = text.indexOf("(", match[0].length - 1);
    const balanced = this.readBalanced(text, open, "(", ")");
    if (balanced === null) return null;
    return { name: match[1] ?? "", args: balanced.content, end: balanced.end };
  }

  private readModifierCall(text: string): { name: string; args: string; end: number } | null {
    const call = this.readCall(text);
    if (call !== null) return call;
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (!match) return null;
    const end = match[0].length;
    const next = this.skipWhitespaceAndSeparators(text, end);
    return text[next] === "{" ? { name: match[1] ?? "", args: "", end } : null;
  }

  private readViewCall(text: string): { name: string; args: string; end: number } | null {
    const call = this.readCall(text);
    if (call !== null) return call;
    const match = text.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\b/);
    if (!match) return null;
    const end = match[0].length;
    const next = this.skipWhitespaceAndSeparators(text, end);
    return text[next] === "{" ? { name: match[1] ?? "", args: "", end } : null;
  }

  private readTrailingClosure(text: string): { body: string; end: number } | null {
    const start = this.skipWhitespaceAndSeparators(text, 0);
    if (text[start] !== "{") return null;
    const balanced = this.readBalanced(text, start, "{", "}");
    return balanced === null ? null : { body: balanced.content, end: balanced.end };
  }

  private readTrailingClosureSequence(
    text: string,
  ): { first?: string; labeled: Record<string, string>; end: number } {
    const first = this.readTrailingClosure(text);
    if (first === null) return { labeled: {}, end: 0 };
    const labeled: Record<string, string> = {};
    let end = first.end;
    while (true) {
      const labelStart = this.skipWhitespaceAndSeparators(text, end);
      const label = text.slice(labelStart).match(/^([A-Za-z_][A-Za-z0-9_]*)\s*:/);
      if (label === null) break;
      const labelName = label[1] ?? "";
      if (
        labelName !== "content" &&
        labelName !== "placeholder" &&
        labelName !== "label" &&
        labelName !== "description" &&
        labelName !== "actions"
      ) {
        break;
      }
      const closure = this.readTrailingClosure(text.slice(labelStart + label[0].length));
      if (closure === null) break;
      labeled[labelName] = closure.body;
      end = labelStart + label[0].length + closure.end;
    }
    return { first: first.body, labeled, end };
  }

  private readBalanced(
    text: string,
    open: number,
    openChar: string,
    closeChar: string,
  ): { content: string; end: number } | null {
    let depth = 0;
    let inString = false;
    for (let i = open; i < text.length; i += 1) {
      const char = text[i];
      const prev = text[i - 1];
      if (char === '"' && prev !== "\\") inString = !inString;
      if (inString) continue;
      if (char === openChar) depth += 1;
      if (char === closeChar) {
        depth -= 1;
        if (depth === 0) return { content: text.slice(open + 1, i), end: i + 1 };
      }
    }
    return null;
  }

  private splitClosureParameter(body: string): { params: string[]; body: string } {
    const match = body.match(
      /^\s*(\(?\s*\$?[A-Za-z_][A-Za-z0-9_]*(?:\s*,\s*\$?[A-Za-z_][A-Za-z0-9_]*)*\s*\)?)\s+in\b/,
    );
    if (!match) return { params: [], body };
    const params = body
      .slice(0, match[0].lastIndexOf(" in"))
      .trim()
      .replace(/[()]/g, "")
      .split(",")
      .map((param) => param.trim().replace(/^\$/, ""))
      .filter(Boolean);
    return {
      params,
      body: body.slice(match[0].length),
    };
  }

  private escapeRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  private extractNamedClosure(args: string, label: string): string | null {
    const index = args.indexOf(`${label}:`);
    if (index < 0) return null;
    const open = args.indexOf("{", index);
    const block = open >= 0 ? this.readBalanced(args, open, "{", "}") : null;
    return block?.content ?? null;
  }

  private splitTernary(text: string): { condition: string; truthy: string; falsy: string } | null {
    const question = this.topLevelIndexOf(text, "?");
    if (question < 0) return null;
    const colon = this.topLevelIndexOf(text.slice(question + 1), ":");
    if (colon < 0) return null;
    return {
      condition: text.slice(0, question),
      truthy: text.slice(question + 1, question + 1 + colon),
      falsy: text.slice(question + 2 + colon),
    };
  }

  private splitTopLevel(text: string, separator: string): string[] {
    const parts: string[] = [];
    let start = 0;
    let depth = 0;
    let inString = false;
    for (let i = 0; i < text.length; i += 1) {
      const char = text[i];
      if (char === '"' && text[i - 1] !== "\\") inString = !inString;
      if (!inString && "({[".includes(char ?? "")) depth += 1;
      if (!inString && ")}]".includes(char ?? "")) depth -= 1;
      if (!inString && depth === 0 && text.startsWith(separator, i)) {
        parts.push(text.slice(start, i).trim());
        start = i + separator.length;
      }
    }
    parts.push(text.slice(start).trim());
    return parts.filter(Boolean);
  }

  private topLevelIndexOf(text: string, needle: string): number {
    let depth = 0;
    let inString = false;
    for (let i = 0; i < text.length; i += 1) {
      const char = text[i];
      if (char === '"' && text[i - 1] !== "\\") inString = !inString;
      if (!inString && "({[".includes(char ?? "")) depth += 1;
      if (!inString && ")}]".includes(char ?? "")) depth -= 1;
      if (!inString && depth === 0 && text.startsWith(needle, i)) return i;
    }
    return -1;
  }

  private topLevelOperatorIndex(
    text: string,
    operators: string[],
    fromRight = false,
  ): number {
    let depth = 0;
    let inString = false;
    const start = fromRight ? text.length - 1 : 0;
    const end = fromRight ? -1 : text.length;
    const step = fromRight ? -1 : 1;
    for (let i = start; i !== end; i += step) {
      const char = text[i];
      if (char === '"' && text[i - 1] !== "\\") inString = !inString;
      if (inString) continue;
      if (fromRight) {
        if (")}]".includes(char ?? "")) depth += 1;
        if ("({[".includes(char ?? "")) depth -= 1;
      } else {
        if ("({[".includes(char ?? "")) depth += 1;
        if (")}]".includes(char ?? "")) depth -= 1;
      }
      if (depth !== 0) continue;
      for (const operator of operators) {
        const operatorIndex = fromRight ? i - operator.length + 1 : i;
        if (operatorIndex < 0 || !text.startsWith(operator, operatorIndex)) continue;
        if ((operator === "+" || operator === "-") && this.isUnarySign(text, operatorIndex)) {
          continue;
        }
        return operatorIndex;
      }
    }
    return -1;
  }

  private isUnarySign(text: string, index: number): boolean {
    let previous = index - 1;
    while (previous >= 0 && /\s/.test(text[previous] ?? "")) previous -= 1;
    if (previous < 0) return true;
    return "([{:,+-*/%<>!=&|?".includes(text[previous] ?? "");
  }

  private skipWhitespaceAndSeparators(text: string, index: number): number {
    let next = index;
    while (next < text.length && /[\s;]/.test(text[next] ?? "")) next += 1;
    return next;
  }

  private unquote(text: string): string {
    return text.slice(1, -1).replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
}

export function parseCustomSidebarSwift(
  source: string,
  context: JsonTemplateContext,
  previews: WorkspacePreview[],
  stateValues: Record<string, SwiftSidebarStateValue> = {},
): CustomSidebarSwiftDocument {
  const time = new Date().toLocaleTimeString([], { hour12: false });
  const events = context.events ?? emptyCustomSidebarEventsContext();
  return new SwiftSidebarParser(
    source,
    {
      root: { ...context, events, clock: { time }, workspaces: previews },
      __stateValues: stateValues,
    },
  ).parse();
}

export function interpolateCustomSidebarTemplate(
  template: string | undefined,
  context: TemplateContext,
): string {
  if (template === undefined) return "";
  return template.replace(
    /\{([A-Za-z][A-Za-z0-9_]*)(?:\.([A-Za-z][A-Za-z0-9_]*))?\}/g,
    (match, key, field) => {
      const root = context[key as keyof TemplateContext];
      const value =
        field === undefined
          ? root
          : typeof root === "object" && root !== null
            ? (root as unknown as Record<string, unknown>)[field]
            : undefined;
      return value === undefined ? match : String(value);
    },
  );
}

export function resolveCustomSidebarActionParams(
  value: unknown,
  context: TemplateContext,
): unknown {
  if (typeof value === "string") {
    return interpolateCustomSidebarTemplate(value, context);
  }
  if (Array.isArray(value)) {
    return value.map((entry) => resolveCustomSidebarActionParams(entry, context));
  }
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [
        key,
        resolveCustomSidebarActionParams(entry, context),
      ]),
    );
  }
  return value;
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

function swiftToken(value: string | undefined, fallback: string): string {
  return (value ?? fallback)
    .replace(/^(?:\.|SwiftUI\.|Color\.|Text\.|Image\.|SymbolVariants\.)/, "")
    .replace(/[^a-zA-Z0-9_-]/g, "")
    .trim();
}

function swiftRedactionReasons(value: string | undefined): string | undefined {
  const tokens = (value ?? ".placeholder")
    .split(/[^a-zA-Z0-9_]+/)
    .map((part) => swiftToken(part, ""))
    .filter((part) => part.length > 0 && part !== "RedactionReasons");
  return tokens.length > 0 ? [...new Set(tokens)].join(",") : undefined;
}

function swiftTabViewStyleToken(value: string | undefined): string {
  const token = swiftToken(value, "automatic");
  if (token === "page" || token === "PageTabViewStyle") return "page";
  return token || "automatic";
}

function swiftTransformOrigin(anchor: string): string {
  switch (anchor) {
    case "top":
      return "center top";
    case "bottom":
      return "center bottom";
    case "leading":
      return "left center";
    case "trailing":
      return "right center";
    case "topLeading":
      return "left top";
    case "topTrailing":
      return "right top";
    case "bottomLeading":
      return "left bottom";
    case "bottomTrailing":
      return "right bottom";
    default:
      return "center center";
  }
}

function swiftKeyboardShortcutAria(
  key: string | undefined,
  modifiers: string | undefined,
): string | undefined {
  if (key === undefined || key.trim() === "") return undefined;
  const parts: string[] = [];
  for (const modifier of (modifiers ?? "").split(",").filter(Boolean)) {
    switch (modifier) {
      case "command":
        parts.push("Meta");
        break;
      case "option":
        parts.push("Alt");
        break;
      case "control":
        parts.push("Control");
        break;
      case "shift":
        parts.push("Shift");
        break;
      default:
        break;
    }
  }
  parts.push(key);
  return parts.join("+");
}

function swiftStackAlignmentClass(value: string | undefined): string {
  return value === undefined
    ? ""
    : ` cmux-custom-sidebar-swift-stack-alignment-${swiftToken(value, "center")}`;
}

function swiftLazyStackClass(value: boolean | undefined): string {
  return value === true ? " cmux-custom-sidebar-swift-stack-lazy" : "";
}

function swiftPinnedViewsClass(value: string | undefined): string {
  return value === undefined
    ? ""
    : value
        .split(",")
        .map((token) => ` cmux-custom-sidebar-swift-pinned-${swiftToken(token, "view")}`)
        .join("");
}

function swiftHorizontalAlignmentToken(
  value: string | undefined,
): "leading" | "center" | "trailing" | undefined {
  const token = swiftToken(value, "");
  if (token === "leading" || token === "center" || token === "trailing") return token;
  return undefined;
}

function swiftGridSelfAlignment(
  value: "leading" | "center" | "trailing",
): CSSProperties["justifySelf"] {
  if (value === "leading") return "start";
  if (value === "trailing") return "end";
  return "center";
}

function swiftGridCellAnchorToken(value: string | undefined): string | undefined {
  const token = swiftToken(value, "");
  switch (token) {
    case "center":
    case "top":
    case "bottom":
    case "leading":
    case "trailing":
    case "topLeading":
    case "topTrailing":
    case "bottomLeading":
    case "bottomTrailing":
      return token;
    default:
      return undefined;
  }
}

function swiftGridCellAnchorPlaceSelf(value: string): CSSProperties["placeSelf"] {
  const vertical = swiftVerticalAlignment(value).grid;
  const horizontal = swiftHorizontalAlignment(value).grid;
  return `${vertical} ${horizontal}`;
}

function swiftAlignmentGuideIsVertical(value: string | undefined): boolean {
  const token = swiftToken(value, "center");
  return (
    token === "top" ||
    token === "bottom" ||
    token === "firstTextBaseline" ||
    token === "lastTextBaseline"
  );
}

function swiftStackAlignmentStyle(
  axis: "vertical" | "horizontal" | "zstack",
  value: string | undefined,
): CSSProperties {
  if (value === undefined) return {};
  const horizontal = swiftHorizontalAlignment(value);
  const vertical = swiftVerticalAlignment(value);
  if (axis === "zstack") {
    return {
      placeItems: `${vertical.grid} ${horizontal.grid}`,
    };
  }
  if (axis === "vertical") {
    return {
      alignItems: horizontal.flex,
      ...(vertical.explicit ? { justifyContent: vertical.flex } : {}),
    };
  }
  return {
    alignItems: vertical.flex,
    ...(horizontal.explicit ? { justifyContent: horizontal.flex } : {}),
  };
}

function swiftHorizontalAlignment(value: string): {
  flex: CSSProperties["alignItems"];
  grid: "start" | "center" | "end";
  explicit: boolean;
} {
  const token = swiftToken(value, "center");
  if (token === "leading" || token.endsWith("Leading")) {
    return { flex: "flex-start", grid: "start", explicit: true };
  }
  if (token === "trailing" || token.endsWith("Trailing")) {
    return { flex: "flex-end", grid: "end", explicit: true };
  }
  return { flex: "center", grid: "center", explicit: token === "center" };
}

function swiftVerticalAlignment(value: string): {
  flex: CSSProperties["alignItems"];
  grid: "start" | "center" | "end";
  explicit: boolean;
} {
  const token = swiftToken(value, "center");
  if (token === "top" || token.startsWith("top")) {
    return { flex: "flex-start", grid: "start", explicit: true };
  }
  if (token === "bottom" || token.startsWith("bottom")) {
    return { flex: "flex-end", grid: "end", explicit: true };
  }
  return { flex: "center", grid: "center", explicit: token === "center" };
}

function swiftFontStyle(value: string | undefined): CSSProperties {
  switch (value?.replace(/^\./, "")) {
    case "largeTitle":
      return { fontSize: "22px", fontWeight: 780, letterSpacing: "-0.02em" };
    case "title":
      return { fontSize: "20px", fontWeight: 760, letterSpacing: "-0.02em" };
    case "title2":
      return { fontSize: "18px", fontWeight: 740, letterSpacing: "-0.01em" };
    case "title3":
      return { fontSize: "16px", fontWeight: 720 };
    case "headline":
      return { fontSize: "14px", fontWeight: 760 };
    case "subheadline":
      return { fontSize: "12px", fontWeight: 650 };
    case "caption":
      return { fontSize: "11px", lineHeight: 1.35 };
    case "caption2":
      return { fontSize: "10px", lineHeight: 1.3 };
    case "body":
    default:
      return {};
  }
}

function swiftFontWeight(value: string | undefined): CSSProperties["fontWeight"] {
  switch (swiftToken(value, "regular")) {
    case "ultraLight":
      return 200;
    case "thin":
      return 250;
    case "light":
      return 300;
    case "regular":
      return 400;
    case "medium":
      return 520;
    case "semibold":
      return 650;
    case "bold":
      return 760;
    case "heavy":
      return 820;
    case "black":
      return 880;
    default: {
      const numeric = Number(value);
      return Number.isFinite(numeric) ? Math.max(1, Math.min(1000, numeric)) : 400;
    }
  }
}

function swiftColor(value: string | undefined): string {
  switch (value?.replace(/^(?:\.|Color\.)/, "")) {
    case "primary":
      return "#f4fffb";
    case "secondary":
      return "#8ea8a2";
    case "tertiary":
      return "#6f8580";
    case "quaternary":
      return "#536661";
    case "quinary":
      return "#40514d";
    case "accent":
    case "accentColor":
      return "#2dd4bf";
    case "red":
      return "#fda4af";
    case "orange":
      return "#fdba74";
    case "yellow":
      return "#fde68a";
    case "green":
      return "#86efac";
    case "blue":
      return "#93c5fd";
    case "mint":
      return "#6ee7b7";
    case "indigo":
      return "#a5b4fc";
    case "purple":
      return "#c4b5fd";
    case "pink":
      return "#f9a8d4";
    case "brown":
      return "#d6a77a";
    case "teal":
    case "cyan":
      return "#67e8f9";
    case "gray":
      return "#94a3b8";
    case "white":
      return "#ffffff";
    case "black":
      return "#020617";
    default:
      return "#dceee9";
  }
}

function applySwiftForegroundStyle(
  style: CSSProperties,
  classes: string[],
  value: string | undefined,
): void {
  const gradient = swiftGradientStyle(value);
  if (gradient !== undefined) {
    style.background = gradient;
    style.color = "transparent";
    (style as CSSProperties & Record<string, string>).WebkitBackgroundClip = "text";
    (style as CSSProperties & Record<string, string>).backgroundClip = "text";
    (style as CSSProperties & Record<string, string>).WebkitTextFillColor = "transparent";
    classes.push("cmux-custom-sidebar-swift-gradient-foreground");
    return;
  }
  const hierarchy = swiftHierarchyToken(value);
  if (hierarchy !== undefined) {
    classes.push(`cmux-custom-sidebar-swift-foreground-${hierarchy}`);
  }
  style.color = swiftColor(value);
}

function swiftHierarchyToken(value: string | undefined): string | undefined {
  const token = value?.replace(/^(?:\.|Color\.|ShapeStyle\.)/, "");
  switch (token) {
    case "primary":
    case "secondary":
    case "tertiary":
    case "quaternary":
    case "quinary":
      return token;
    default:
      return undefined;
  }
}

function swiftShadowColor(value: string | undefined): string {
  if (value === undefined) return "rgba(0, 0, 0, 0.32)";
  const token = value.replace(/^(?:\.|Color\.)/, "");
  if (token === "black") return "rgba(0, 0, 0, 0.42)";
  if (token === "white") return "rgba(255, 255, 255, 0.2)";
  return swiftColor(value);
}

function swiftTextAlign(value: string | undefined): CSSProperties["textAlign"] {
  switch (value?.replace(/^\./, "")) {
    case "center":
      return "center";
    case "trailing":
    case "right":
      return "right";
    case "leading":
    case "left":
    default:
      return "left";
  }
}

function swiftTextDecorationStyle(
  value: string | undefined,
): CSSProperties["textDecorationStyle"] {
  switch (swiftToken(value, "solid")) {
    case "dot":
      return "dotted";
    case "dash":
    case "dashDot":
    case "dashDotDot":
      return "dashed";
    case "solid":
    default:
      return "solid";
  }
}

function swiftBlendMode(value: string | undefined): CSSProperties["mixBlendMode"] | undefined {
  switch (swiftToken(value, "normal")) {
    case "normal":
      return "normal";
    case "multiply":
      return "multiply";
    case "screen":
      return "screen";
    case "overlay":
      return "overlay";
    case "darken":
      return "darken";
    case "lighten":
      return "lighten";
    case "colorDodge":
      return "color-dodge";
    case "colorBurn":
      return "color-burn";
    case "softLight":
      return "soft-light";
    case "hardLight":
      return "hard-light";
    case "difference":
      return "difference";
    case "exclusion":
      return "exclusion";
    case "hue":
      return "hue";
    case "saturation":
      return "saturation";
    case "color":
      return "color";
    case "luminosity":
      return "luminosity";
    default:
      return undefined;
  }
}

function swiftBackgroundStyle(value: string | undefined): string {
  return swiftGradientStyle(value) ?? swiftMaterialBackgroundStyle(value) ?? swiftBackgroundColor(value);
}

function swiftBackgroundColor(value: string | undefined): string {
  switch (value?.replace(/^(?:\.|Color\.)/, "")) {
    case "secondary":
    case "gray":
      return "rgba(148, 163, 184, 0.13)";
    case "tertiary":
      return "rgba(111, 133, 128, 0.14)";
    case "quaternary":
    case "quinary":
      return "rgba(83, 102, 97, 0.14)";
    case "accent":
    case "accentColor":
      return "rgba(45, 212, 191, 0.15)";
    case "red":
      return "rgba(244, 63, 94, 0.14)";
    case "orange":
      return "rgba(249, 115, 22, 0.14)";
    case "yellow":
      return "rgba(234, 179, 8, 0.13)";
    case "green":
      return "rgba(34, 197, 94, 0.14)";
    case "blue":
      return "rgba(59, 130, 246, 0.14)";
    case "mint":
      return "rgba(52, 211, 153, 0.14)";
    case "indigo":
      return "rgba(99, 102, 241, 0.15)";
    case "purple":
      return "rgba(139, 92, 246, 0.15)";
    case "brown":
      return "rgba(146, 64, 14, 0.16)";
    case "teal":
    case "cyan":
      return "rgba(20, 184, 166, 0.15)";
    default:
      return "rgba(15, 23, 42, 0.45)";
  }
}

function swiftMaterialToken(value: string | undefined): string | undefined {
  const token = value?.replace(/^(?:\.|Material\.)/, "");
  switch (token) {
    case "ultraThinMaterial":
    case "thinMaterial":
    case "regularMaterial":
    case "thickMaterial":
    case "ultraThickMaterial":
    case "bar":
      return token;
    default:
      return undefined;
  }
}

function swiftMaterialBackgroundStyle(value: string | undefined): string | undefined {
  switch (swiftMaterialToken(value)) {
    case "ultraThinMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.08), rgba(15, 23, 42, 0.22))";
    case "thinMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.1), rgba(15, 23, 42, 0.3))";
    case "regularMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.13), rgba(15, 23, 42, 0.42))";
    case "thickMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.16), rgba(15, 23, 42, 0.56))";
    case "ultraThickMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.2), rgba(15, 23, 42, 0.68))";
    case "bar":
      return "linear-gradient(180deg, rgba(15, 23, 42, 0.78), rgba(2, 6, 23, 0.66))";
    default:
      return undefined;
  }
}

function swiftMaterialBackdropFilter(value: string | undefined): string {
  switch (swiftMaterialToken(value)) {
    case "ultraThinMaterial":
      return "blur(18px) saturate(1.12)";
    case "thinMaterial":
      return "blur(16px) saturate(1.16)";
    case "thickMaterial":
      return "blur(12px) saturate(1.28)";
    case "ultraThickMaterial":
      return "blur(10px) saturate(1.36)";
    case "bar":
      return "blur(14px) saturate(1.22)";
    case "regularMaterial":
    default:
      return "blur(14px) saturate(1.2)";
  }
}

function swiftContainerRelativeFrameSize(
  count: number | undefined,
  span: number | undefined,
  spacing: number | undefined,
): string {
  if (count === undefined) return "100%";
  const safeSpan = Math.min(span ?? 1, count);
  const percentage = (safeSpan / count) * 100;
  if (spacing === undefined || spacing === 0 || count <= 1) {
    return `${percentage}%`;
  }
  return `calc(${percentage}% - ${spacing}px)`;
}

function swiftFontWidth(value: string | undefined): CSSProperties["fontStretch"] | undefined {
  switch (swiftToken(value, "standard")) {
    case "compressed":
      return "75%";
    case "condensed":
      return "87.5%";
    case "standard":
      return "normal";
    case "expanded":
      return "112.5%";
    default:
      return undefined;
  }
}

function swiftDynamicTypeFontSize(token: string): CSSProperties["fontSize"] | undefined {
  switch (token) {
    case "xSmall":
      return "0.82rem";
    case "small":
      return "0.88rem";
    case "medium":
      return "1rem";
    case "large":
      return "1.06rem";
    case "xLarge":
      return "1.18rem";
    case "xxLarge":
      return "1.3rem";
    case "xxxLarge":
      return "1.44rem";
    case "accessibility1":
      return "1.6rem";
    case "accessibility2":
      return "1.78rem";
    case "accessibility3":
      return "1.98rem";
    case "accessibility4":
      return "2.2rem";
    case "accessibility5":
      return "2.44rem";
    default:
      return undefined;
  }
}

function swiftGradientStyle(value: string | undefined): string | undefined {
  const expression = value?.trim();
  if (!expression) return undefined;
  const colorGradient = expression.match(/^(?:\.|Color\.)?([A-Za-z]+)\.gradient$/);
  if (colorGradient) {
    const color = swiftColor(colorGradient[1]);
    return `linear-gradient(135deg, ${color}, ${swiftTransparentColor(color)})`;
  }

  const call = expression.match(/^([A-Za-z]+Gradient)\(([\s\S]*)\)$/);
  if (!call) return undefined;
  const name = call[1];
  const args = call[2] ?? "";
  const colors = swiftGradientColors(args);
  if (colors.length === 0) return undefined;

  if (name === "LinearGradient") {
    const endPoint = swiftNamedArg(args, "endPoint") ?? ".bottom";
    return `linear-gradient(${swiftLinearGradientDirection(endPoint)}, ${colors.join(", ")})`;
  }
  if (name === "RadialGradient") {
    return `radial-gradient(circle, ${colors.join(", ")})`;
  }
  if (name === "AngularGradient") {
    return `conic-gradient(${colors.join(", ")})`;
  }
  return undefined;
}

function swiftGradientColors(args: string): string[] {
  const directColors = swiftNamedArg(args, "colors");
  const colorsSource =
    directColors ??
    (() => {
      const gradientArg = swiftNamedArg(args, "gradient");
      if (gradientArg === undefined) return undefined;
      const match = gradientArg.trim().match(/^Gradient\(([\s\S]*)\)$/);
      return match ? swiftNamedArg(match[1] ?? "", "colors") : undefined;
    })();
  if (colorsSource === undefined) return [];
  const trimmed = colorsSource.trim();
  if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) return [];
  return swiftSplitTopLevel(trimmed.slice(1, -1), ",")
    .map((color) => swiftColor(color.trim()))
    .slice(0, 8);
}

function swiftLinearGradientDirection(endPoint: string): string {
  const token = endPoint.replace(/^(?:\.|UnitPoint\.)/, "");
  switch (token) {
    case "top":
      return "to top";
    case "topLeading":
    case "topLeft":
      return "to top left";
    case "topTrailing":
    case "topRight":
      return "to top right";
    case "leading":
    case "left":
      return "to left";
    case "trailing":
    case "right":
      return "to right";
    case "bottomLeading":
    case "bottomLeft":
      return "to bottom left";
    case "bottomTrailing":
    case "bottomRight":
      return "to bottom right";
    case "bottom":
    default:
      return "to bottom";
  }
}

function swiftTransparentColor(color: string): string {
  if (color.startsWith("#") && color.length === 7) {
    return `${color}33`;
  }
  return "rgba(15, 23, 42, 0.2)";
}

function swiftNamedArg(args: string, label: string): string | undefined {
  for (const arg of swiftSplitTopLevel(args, ",")) {
    const colon = swiftTopLevelIndexOf(arg, ":");
    if (colon < 0) continue;
    if (arg.slice(0, colon).trim() === label) {
      return arg.slice(colon + 1).trim();
    }
  }
  return undefined;
}

function swiftSplitTopLevel(text: string, separator: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote = false;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index] ?? "";
    if (char === '"' && text[index - 1] !== "\\") quote = !quote;
    if (quote) continue;
    if (char === "(" || char === "[" || char === "{") depth += 1;
    if (char === ")" || char === "]" || char === "}") depth -= 1;
    if (depth === 0 && text.startsWith(separator, index)) {
      parts.push(text.slice(start, index).trim());
      index += separator.length - 1;
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts.filter((part) => part !== "");
}

function swiftTopLevelIndexOf(text: string, needle: string): number {
  let depth = 0;
  let quote = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index] ?? "";
    if (char === '"' && text[index - 1] !== "\\") quote = !quote;
    if (quote) continue;
    if (char === "(" || char === "[" || char === "{") depth += 1;
    if (char === ")" || char === "]" || char === "}") depth -= 1;
    if (depth === 0 && text.startsWith(needle, index)) return index;
  }
  return -1;
}

function swiftSystemImageGlyph(systemName: string): string {
  const normalized = systemName.toLowerCase();
  if (normalized.includes("folder")) return "folder";
  if (normalized.includes("terminal")) return "term";
  if (normalized.includes("bolt")) return "bolt";
  if (normalized.includes("checkmark")) return "check";
  if (normalized.includes("circle")) return "dot";
  if (normalized.includes("exclamation")) return "!";
  if (normalized.includes("clock")) return "time";
  if (normalized.includes("gear")) return "gear";
  return systemName.replace(/\..*$/, "") || "icon";
}

function JsonSidebarBody({
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
