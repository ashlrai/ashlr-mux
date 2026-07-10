export interface CommandPaletteUsageEntry {
  useCount: number;
  lastUsedAt: number;
}

export type CommandPaletteUsageHistory = Record<string, CommandPaletteUsageEntry>;

export interface UsageHistoryStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const COMMAND_PALETTE_USAGE_HISTORY_KEY = "cmux.commandPalette.usageHistory.v1";

function validEntry(value: unknown): value is CommandPaletteUsageEntry {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<CommandPaletteUsageEntry>;
  return (
    Number.isFinite(candidate.useCount) &&
    Number.isFinite(candidate.lastUsedAt) &&
    (candidate.useCount ?? 0) >= 0 &&
    (candidate.lastUsedAt ?? 0) >= 0
  );
}

export function normalizeUsageHistory(value: unknown): CommandPaletteUsageHistory {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return {};
  }
  const normalized: CommandPaletteUsageHistory = {};
  for (const [commandId, entry] of Object.entries(value)) {
    if (commandId.trim() === "" || !validEntry(entry)) {
      continue;
    }
    normalized[commandId] = {
      useCount: Math.floor(entry.useCount),
      lastUsedAt: entry.lastUsedAt,
    };
  }
  return normalized;
}

export function readCommandPaletteUsageHistory(
  storage: UsageHistoryStorage | undefined,
): CommandPaletteUsageHistory {
  if (storage === undefined) {
    return {};
  }
  try {
    const raw = storage.getItem(COMMAND_PALETTE_USAGE_HISTORY_KEY);
    return raw === null ? {} : normalizeUsageHistory(JSON.parse(raw));
  } catch {
    return {};
  }
}

export function writeCommandPaletteUsageHistory(
  storage: UsageHistoryStorage | undefined,
  history: CommandPaletteUsageHistory,
): void {
  if (storage === undefined) {
    return;
  }
  try {
    storage.setItem(COMMAND_PALETTE_USAGE_HISTORY_KEY, JSON.stringify(history));
  } catch {
    // Storage is best-effort; ranking still works for the current in-memory run.
  }
}

export function recordCommandPaletteUsage(
  history: CommandPaletteUsageHistory,
  commandId: string,
  timestampSeconds: number,
): CommandPaletteUsageHistory {
  const trimmedCommandId = commandId.trim();
  if (trimmedCommandId === "" || !Number.isFinite(timestampSeconds)) {
    return history;
  }
  const existing = history[trimmedCommandId];
  return {
    ...history,
    [trimmedCommandId]: {
      useCount: (existing?.useCount ?? 0) + 1,
      lastUsedAt: Math.max(0, timestampSeconds),
    },
  };
}

export function browserUsageHistoryStorage(): UsageHistoryStorage | undefined {
  if (typeof window === "undefined") {
    return undefined;
  }
  return window.localStorage;
}
