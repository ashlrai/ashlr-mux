import { expect, test } from "bun:test";
import { filterTsRsDiagnostics } from "./stderr-filter.mjs";

test("filters only the exact known ts-rs serde diagnostic", () => {
  const known = `warning: failed to parse serde attribute
  |
  | #[serde(default, skip_serializing_if = "Option::is_none")]
  |
  = note: ts-rs failed to parse this attribute. It will be ignored.
`;
  expect(filterTsRsDiagnostics(known)).toBe("");
});

test("surfaces unexpected compiler stderr byte-for-byte", () => {
  const unexpected = "warning: unused variable: lifecycle\nerror: build failed\n";
  expect(filterTsRsDiagnostics(unexpected)).toBe(unexpected);
});

test("preserves injected diagnostics inside known sentinels", () => {
  const injected = `warning: failed to parse serde attribute
  |
  | #[serde(default, skip_serializing_if = "Option::is_none")]
error: injected build failure
  |
  = note: ts-rs failed to parse this attribute. It will be ignored.
`;
  expect(filterTsRsDiagnostics(injected)).toBe(injected);
});

test("preserves malformed known-looking blocks", () => {
  const malformed = `warning: failed to parse serde attribute
  | unexpected marker
  | #[serde(default, skip_serializing_if = "Option::is_none")]
  |
  = note: ts-rs failed to parse this attribute. It will be ignored.
`;
  expect(filterTsRsDiagnostics(malformed)).toBe(malformed);
});

test("preserves interleaved warning blocks byte-for-byte", () => {
  const interleaved = `warning: failed to parse serde attribute
  |
  | #[serde(default, skip_serializing_if = "Option::is_none")]
warning: unused variable: lifecycle
  |
  = note: ts-rs failed to parse this attribute. It will be ignored.
`;
  expect(filterTsRsDiagnostics(interleaved)).toBe(interleaved);
});
