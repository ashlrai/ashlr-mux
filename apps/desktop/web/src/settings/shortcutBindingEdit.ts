import type { ShortcutBinding } from "@cmux/core-types";

export interface ShortcutCaptureEvent {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
}

export function shortcutBindingDraftValue(
  binding: ShortcutBinding | null | undefined,
): string {
  if (binding == null) {
    return "";
  }
  return Array.isArray(binding) ? binding.join(", ") : binding;
}

export function shortcutBindingFromDraft(draft: string): ShortcutBinding | null {
  const trimmed = draft.trim();
  if (trimmed === "") {
    return null;
  }
  if (!trimmed.includes(",")) {
    return trimmed;
  }
  const strokes = trimmed
    .split(",")
    .map((stroke) => stroke.trim())
    .filter((stroke) => stroke !== "");
  return strokes.length === 0 ? null : strokes;
}

export function shortcutBindingFromKeyboardEvent(
  event: ShortcutCaptureEvent,
): ShortcutBinding | null | undefined {
  if (event.key === "Backspace" || event.key === "Delete") {
    return null;
  }
  if (event.key === "Escape" || isModifierKey(event.key)) {
    return undefined;
  }

  const key = configKeyTokenFromKeyboardKey(event.key);
  if (key === null) {
    return undefined;
  }

  const hasModifier =
    event.ctrlKey === true ||
    event.metaKey === true ||
    event.altKey === true ||
    event.shiftKey === true;
  if (!hasModifier && key.length === 1) {
    return undefined;
  }

  const parts: string[] = [];
  if (event.ctrlKey) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");
  if (event.metaKey) parts.push("cmd");
  parts.push(key);
  return parts.join("+");
}

function isModifierKey(key: string): boolean {
  return key === "Control" || key === "Alt" || key === "Shift" || key === "Meta";
}

function configKeyTokenFromKeyboardKey(key: string): string | null {
  if (key === " ") return "space";
  if (key.length === 1) return key.toLowerCase();
  if (/^F([1-9]|1[0-9]|20)$/i.test(key)) return key.toLowerCase();
  switch (key) {
    case "Enter":
      return "enter";
    case "Tab":
      return "tab";
    case "ArrowLeft":
      return "arrowleft";
    case "ArrowRight":
      return "arrowright";
    case "ArrowUp":
      return "arrowup";
    case "ArrowDown":
      return "arrowdown";
    default:
      return null;
  }
}
