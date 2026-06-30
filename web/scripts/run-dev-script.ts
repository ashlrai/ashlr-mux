#!/usr/bin/env bun
// Cross-platform dispatcher for the web/ dev shell scripts.
//
// On win32 it runs the PowerShell port (`pwsh -NoProfile -File scripts/<name>.ps1`);
// everywhere else (and on Windows with CMUX_DEV_SHELL=gitbash) it runs the bash
// original (`bash scripts/<name>.sh`) so macOS/Linux stay byte-identical. The
// single optional second argument is the db-local subcommand (up/down/...).
import { spawnSync } from "node:child_process";

const [name, sub] = process.argv.slice(2);
if (!name) {
  console.error("Usage: run-dev-script.ts <script-name> [subcommand]");
  process.exit(2);
}

const useGitBash = process.env.CMUX_DEV_SHELL === "gitbash";
const usePwsh = process.platform === "win32" && !useGitBash;

const command = usePwsh ? "pwsh" : "bash";
const args = usePwsh
  ? ["-NoProfile", "-File", `scripts/${name}.ps1`]
  : [`scripts/${name}.sh`];
if (sub) {
  args.push(sub);
}

const result = spawnSync(command, args, { stdio: "inherit" });
if (result.error) {
  console.error(`Failed to run ${command}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
