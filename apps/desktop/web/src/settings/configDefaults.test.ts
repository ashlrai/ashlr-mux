// Drift-pin: the transcribed defaults must equal the canonical `default`
// fields in `web/data/cmux.schema.json` — the schema is the source of truth,
// so any upstream default change fails here instead of silently diverging.

import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  DEFAULT_APP,
  DEFAULT_NOTIFICATIONS,
  DEFAULT_SIDEBAR,
  withSettingsDefaults,
} from "./configDefaults";

// apps/desktop/web/src/settings → repo root is five levels up.
const SCHEMA_PATH = join(
  import.meta.dir,
  "..", "..", "..", "..", "..",
  "web", "data", "cmux.schema.json",
);

type SchemaProps = Record<string, { default?: unknown }>;

function schemaSectionDefaults(section: string): Record<string, unknown> {
  const schema = JSON.parse(readFileSync(SCHEMA_PATH, "utf-8")) as {
    properties: Record<string, { properties: SchemaProps }>;
  };
  const props = schema.properties[section]?.properties ?? {};
  const defaults: Record<string, unknown> = {};
  for (const [key, spec] of Object.entries(props)) {
    if (spec.default !== undefined && spec.default !== null) {
      defaults[key] = spec.default;
    }
  }
  return defaults;
}

describe("configDefaults drift-pin vs cmux.schema.json", () => {
  test("sidebar defaults match the schema", () => {
    // TS `right_max_width` is schema `rightMaxWidth` with default null →
    // deliberately absent from DEFAULT_SIDEBAR; the walk skips null defaults.
    expect({ ...DEFAULT_SIDEBAR } as Record<string, unknown>).toEqual(
      schemaSectionDefaults("sidebar"),
    );
  });

  test("notifications defaults match the schema", () => {
    expect({ ...DEFAULT_NOTIFICATIONS } as Record<string, unknown>).toEqual(
      schemaSectionDefaults("notifications"),
    );
  });

  test("app defaults match the schema", () => {
    expect({ ...DEFAULT_APP } as Record<string, unknown>).toEqual(
      schemaSectionDefaults("app"),
    );
  });
});

describe("withSettingsDefaults", () => {
  test("fills absent sections and merges partial ones over defaults", () => {
    const effective = withSettingsDefaults({
      sidebar: { ...DEFAULT_SIDEBAR, showSSH: false },
    });
    expect(effective.sidebar?.showSSH).toBe(false); // override wins
    expect(effective.sidebar?.showPorts).toBe(true); // default fills
    expect(effective.notifications).toEqual(DEFAULT_NOTIFICATIONS);
    expect(effective.app?.appearance).toBe("system");
  });

  test("preserves unrelated sections untouched", () => {
    const config = { terminal: { fontSize: 13 } } as never;
    const effective = withSettingsDefaults(config);
    expect((effective as { terminal?: unknown }).terminal).toEqual({
      fontSize: 13,
    });
  });
});
