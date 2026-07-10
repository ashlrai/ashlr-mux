import type { ShortcutBinding } from "@cmux/core-types";

import { parseShortcutBinding } from "./shortcutBinding";
import { isUnbound, type ShortcutStroke } from "./shortcutFormat";

export const WARM_CLAUDE_CODE_SHORTCUT_ACTION = "agent.warmClaudeCode";

export interface ShortcutRuntimeEvent {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}

export type ShortcutBindingMap = Record<string, ShortcutBinding | null | undefined>;

export function shortcutActionForEvent(
  bindings: ShortcutBindingMap | null | undefined,
  event: ShortcutRuntimeEvent,
): string | null {
  if (bindings == null) {
    return null;
  }
  for (const [actionId, binding] of Object.entries(bindings)) {
    if (shortcutBindingMatchesEvent(binding, event)) {
      return actionId;
    }
  }
  return null;
}

export function shortcutBindingMatchesEvent(
  binding: ShortcutBinding | null | undefined,
  event: ShortcutRuntimeEvent,
): boolean {
  const parsed = parseShortcutBinding(binding);
  if (parsed === null || isUnbound(parsed) || parsed.second != null) {
    return false;
  }
  const eventKey = eventKeyToken(event.key);
  if (eventKey === null) {
    return false;
  }
  return strokeMatchesEvent(parsed.first, event, eventKey);
}

function strokeMatchesEvent(
  stroke: ShortcutStroke,
  event: ShortcutRuntimeEvent,
  eventKey: string,
): boolean {
  return (
    stroke.key === eventKey &&
    stroke.control === (event.ctrlKey === true) &&
    stroke.command === (event.metaKey === true) &&
    stroke.option === (event.altKey === true) &&
    stroke.shift === (event.shiftKey === true)
  );
}

function eventKeyToken(key: string): string | null {
  if (key === " ") return "space";
  if (key.length === 1) return key.toLowerCase();
  if (/^F([1-9]|1[0-9]|20)$/i.test(key)) return key.toLowerCase();
  switch (key) {
    case "Enter":
      return "\r";
    case "Tab":
      return "\t";
    case "ArrowLeft":
      return "←";
    case "ArrowRight":
      return "→";
    case "ArrowUp":
      return "↑";
    case "ArrowDown":
      return "↓";
    default:
      return null;
  }
}
