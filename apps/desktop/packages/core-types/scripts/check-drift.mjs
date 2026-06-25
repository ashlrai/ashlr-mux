#!/usr/bin/env bun
// Drift check (enforces cross-cutting rule #1: the web chrome and Rust never
// drift). Regenerates the bindings into a temp dir and compares them byte-for-
// byte against the committed src/generated. Exits non-zero on any difference.
import {
  GENERATED_DIR,
  generateTemp,
  readFileSync,
  readdirSync,
  rmSync,
} from "./lib.mjs";
import { join } from "node:path";

function snapshot(dir) {
  const out = new Map();
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".ts")).sort()) {
    out.set(f, readFileSync(join(dir, f), "utf8"));
  }
  return out;
}

const { dir: tmp } = generateTemp();
try {
  const fresh = snapshot(tmp);
  let committed;
  try {
    committed = snapshot(GENERATED_DIR);
  } catch {
    committed = new Map();
  }

  const problems = [];
  for (const [name, body] of fresh) {
    if (!committed.has(name)) {
      problems.push(`missing committed file: ${name}`);
    } else if (committed.get(name) !== body) {
      problems.push(`content drift: ${name}`);
    }
  }
  for (const name of committed.keys()) {
    if (!fresh.has(name)) problems.push(`stale committed file: ${name}`);
  }

  if (problems.length > 0) {
    console.error(
      "core-types drift detected. Run `bun run generate` in " +
        "apps/desktop/packages/core-types and commit the result.\n  - " +
        problems.join("\n  - "),
    );
    process.exit(1);
  }
  console.log("core-types bindings are up to date (no drift).");
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
