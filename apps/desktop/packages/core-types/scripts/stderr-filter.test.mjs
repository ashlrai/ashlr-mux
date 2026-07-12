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
