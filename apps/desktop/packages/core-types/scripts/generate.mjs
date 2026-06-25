#!/usr/bin/env bun
// Regenerate the committed TypeScript bindings into src/generated.
import { generateCommitted, GENERATED_DIR } from "./lib.mjs";

const files = generateCommitted();
console.log(
  `Generated ${files.length} binding file(s) into ${GENERATED_DIR}:\n  ` +
    files.join("\n  "),
);
