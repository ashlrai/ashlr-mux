import { describe, expect, test } from "bun:test";

import {
  COMMAND_PALETTE_USAGE_HISTORY_KEY,
  normalizeUsageHistory,
  readCommandPaletteUsageHistory,
  recordCommandPaletteUsage,
  writeCommandPaletteUsageHistory,
  type UsageHistoryStorage,
} from "./usageHistory";

function memoryStorage(initial?: string): UsageHistoryStorage & { value: string | null } {
  return {
    value: initial ?? null,
    getItem(key: string): string | null {
      return key === COMMAND_PALETTE_USAGE_HISTORY_KEY ? this.value : null;
    },
    setItem(key: string, value: string): void {
      if (key === COMMAND_PALETTE_USAGE_HISTORY_KEY) {
        this.value = value;
      }
    },
  };
}

describe("command palette usage history", () => {
  test("normalizeUsageHistory keeps only valid command entries", () => {
    expect(
      normalizeUsageHistory({
        "palette.newWorkspace": { useCount: 2.8, lastUsedAt: 100 },
        "": { useCount: 1, lastUsedAt: 50 },
        badCount: { useCount: -1, lastUsedAt: 50 },
        badTimestamp: { useCount: 1, lastUsedAt: Number.NaN },
        array: [],
      }),
    ).toEqual({
      "palette.newWorkspace": { useCount: 2, lastUsedAt: 100 },
    });
  });

  test("readCommandPaletteUsageHistory tolerates missing and corrupt storage", () => {
    expect(readCommandPaletteUsageHistory(undefined)).toEqual({});
    expect(readCommandPaletteUsageHistory(memoryStorage("{"))).toEqual({});
    expect(readCommandPaletteUsageHistory(memoryStorage())).toEqual({});
  });

  test("recordCommandPaletteUsage increments count and stamps recency", () => {
    expect(
      recordCommandPaletteUsage(
        { "palette.openSettings": { useCount: 2, lastUsedAt: 10 } },
        "palette.openSettings",
        42,
      ),
    ).toEqual({
      "palette.openSettings": { useCount: 3, lastUsedAt: 42 },
    });
  });

  test("writeCommandPaletteUsageHistory stores the backend wire shape", () => {
    const storage = memoryStorage();
    writeCommandPaletteUsageHistory(storage, {
      "palette.openSettings": { useCount: 1, lastUsedAt: 42.5 },
    });
    expect(storage.value).toBe(
      '{"palette.openSettings":{"useCount":1,"lastUsedAt":42.5}}',
    );
  });
});
