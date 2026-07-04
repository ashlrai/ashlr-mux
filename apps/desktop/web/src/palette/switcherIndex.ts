// Switcher search keyword indexer — port of
// `Packages/macOS/CmuxCommandPalette/Sources/CmuxCommandPalette/Search/CommandPaletteSwitcherSearchIndexer.swift:8-141`
// (whole algorithm) with the metadata value type from
// `CommandPaletteSwitcherSearchMetadata.swift:5-45` (minus `combine(into:)`,
// which is SipHash change-detection the web shell does not need).
//
// Derives the normalized, de-duplicated search-keyword list for one switcher
// entry (workspace or surface) from base keywords + metadata (directories,
// git branches, ports, description). Production callers:
// `Sources/ContentView.swift:5275-5285` (workspace entries, detail
// "workspace") and 5319-5331 (surface entries, detail "surface").
//
// SANCTIONED DIVERGENCE #1 (dedupe folding): Swift dedupe keys use
// `.folding(options: [.diacriticInsensitive, .caseInsensitive], locale:
// .current).lowercased()` — ICU folding with the CURRENT locale (e.g.
// Turkish dotless-i). `foldKey` approximates with locale-independent
// NFD + strip-combining-marks + toLowerCase(); only dedupe collisions on
// exotic scripts can differ.
//
// SANCTIONED DIVERGENCE #2 (path standardization):
// `NSString.standardizingPath` is partially filesystem-dependent (its
// `/private`-prefix stripping consults the disk) and expands "~" from the
// process environment. `standardizePathLexically` is a PURE LEXICAL port
// taking `homeDir` from the caller: no `/private` resolution, no symlink or
// existence checks; ".." resolves lexically and only in absolute paths
// (matching the documented NSString behavior). Windows-shaped inputs
// ("C:\\...") get no special handling — they contain no "/" so they pass
// through unchanged, and the delimiter split later tokenizes them fine;
// web-shell cwds are Windows paths.

import { trimWhitespaceAndNewlines } from "./listScope";

/**
 * Searchable workspace/surface metadata feeding the switcher search corpus.
 * Mirrors `CommandPaletteSwitcherSearchMetadata` (Metadata.swift:5-26); all
 * fields default to empty.
 */
export interface SwitcherSearchMetadata {
  directories?: string[];
  branches?: string[];
  ports?: number[];
  description?: string | null;
}

/**
 * How much metadata detail to tokenize: workspaces index whole paths,
 * surfaces additionally index path/branch components. Mirrors
 * `CommandPaletteSwitcherSearchIndexer.MetadataDetail`.
 */
export type MetadataDetail = "workspace" | "surface";

/** Injected environment for the pure path helpers. */
export interface SwitcherIndexEnv {
  homeDir?: string;
}

/**
 * The 7 metadata delimiter characters, mirroring
 * `CharacterSet(charactersIn: "/\\.:_- ")` (Indexer.swift:18): slash,
 * backslash, dot, colon, underscore, hyphen, space.
 */
const METADATA_DELIMITERS = /[/\\.:_\- ]/;

/**
 * Splits on any metadata delimiter and drops empty parts, mirroring
 * `components(separatedBy: metadataDelimiters).filter { !$0.isEmpty }`.
 */
export function splitOnMetadataDelimiters(s: string): string[] {
  return s.split(METADATA_DELIMITERS).filter((part) => part !== "");
}

/**
 * Dedupe key mirroring Swift's diacritic- and case-insensitive fold
 * (Indexer.swift:133-135). See SANCTIONED DIVERGENCE #1 above.
 */
export function foldKey(s: string): string {
  return s
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .toLowerCase();
}

/**
 * Trim-skip-dedupe pass mirroring `uniqueNormalizedPreservingOrder`
 * (Indexer.swift:125-140): each value is trimmed with Swift's
 * `whitespacesAndNewlines` set, empties are skipped, and the FIRST
 * occurrence's original trimmed casing wins (the folded key is only the
 * dedupe key, never the output).
 */
function uniqueNormalizedPreservingOrder(values: string[]): string[] {
  const result: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    const trimmed = trimWhitespaceAndNewlines(value);
    if (trimmed === "") {
      continue;
    }
    const normalizedKey = foldKey(trimmed);
    if (seen.has(normalizedKey)) {
      continue;
    }
    seen.add(normalizedKey);
    result.push(trimmed);
  }
  return result;
}

/**
 * Pure lexical stand-in for `NSString.standardizingPath`. See SANCTIONED
 * DIVERGENCE #2 above for the exact rule set:
 * (a) expand a leading "~" / "~/" using `homeDir` when provided (named-user
 *     forms like "~bob" stay as-is, matching NSString for unknown users);
 * (b) collapse repeated "/" and remove "." segments;
 * (c) resolve ".." lexically, but ONLY in absolute paths ("a/../b" stays,
 *     "/a/b/../c" → "/a/c", and root's parent is root);
 * (d) strip the trailing slash ("/" stays "/");
 * (e) empty input → "" (the caller then falls back to the trimmed original,
 *     mirroring the `standardized.isEmpty` guard at Indexer.swift:78).
 */
export function standardizePathLexically(path: string, homeDir?: string): string {
  if (path === "") {
    return "";
  }
  let p = path;
  if (homeDir !== undefined && homeDir !== "") {
    if (p === "~") {
      p = homeDir;
    } else if (p.startsWith("~/")) {
      p = homeDir + p.slice(1);
    }
  }
  const isAbsolute = p.startsWith("/");
  const out: string[] = [];
  for (const segment of p.split("/")) {
    if (segment === "" || segment === ".") {
      continue;
    }
    if (segment === ".." && isAbsolute) {
      // Lexical parent: pop, or stay at root ("/.." → "/").
      out.pop();
      continue;
    }
    out.push(segment);
  }
  if (isAbsolute) {
    return `/${out.join("/")}`;
  }
  return out.join("/");
}

/**
 * Pure stand-in for `NSString.abbreviatingWithTildeInPath`: abbreviates only
 * at a path-component boundary — the exact home directory becomes "~" and a
 * `homeDir + "/"` prefix becomes "~/..."; anything else (including a
 * non-boundary string prefix) is unchanged. No `homeDir` → unchanged.
 */
export function abbreviateWithTilde(path: string, homeDir?: string): string {
  if (homeDir === undefined || homeDir === "") {
    return path;
  }
  if (path === homeDir) {
    return "~";
  }
  if (path.startsWith(`${homeDir}/`)) {
    return `~${path.slice(homeDir.length)}`;
  }
  return path;
}

/**
 * Last path component with `URL(fileURLWithPath:isDirectory:true)`
 * semantics: strip trailing slashes, then take the segment after the last
 * "/" ("/a/b/" → "b", "/" → "/"). Backslashes are not separators.
 */
export function lastPathComponent(path: string): string {
  let p = path;
  while (p.length > 1 && p.endsWith("/")) {
    p = p.slice(0, -1);
  }
  if (p === "/") {
    return "/";
  }
  const slash = p.lastIndexOf("/");
  return slash >= 0 ? p.slice(slash + 1) : p;
}

/**
 * Directory tokens. Mirrors `directoryTokensForSearch` (Indexer.swift:70-90):
 * trim → standardize → fall back to the trimmed original when
 * standardization yields "" → abbreviate with tilde. Workspace detail
 * indexes the whole values; surface detail adds the basename and the
 * delimiter-split components.
 */
function directoryTokens(rawDirectory: string, detail: MetadataDetail, homeDir?: string): string[] {
  const trimmed = trimWhitespaceAndNewlines(rawDirectory);
  if (trimmed === "") {
    return [];
  }
  const standardized = standardizePathLexically(trimmed, homeDir);
  const canonical = standardized === "" ? trimmed : standardized;
  const abbreviated = abbreviateWithTilde(canonical, homeDir);
  if (detail === "workspace") {
    return uniqueNormalizedPreservingOrder([trimmed, canonical, abbreviated]);
  }
  const basename = lastPathComponent(canonical);
  const components = splitOnMetadataDelimiters(canonical);
  return uniqueNormalizedPreservingOrder([trimmed, canonical, abbreviated, basename, ...components]);
}

/**
 * Branch tokens. Mirrors `branchTokensForSearch` (Indexer.swift:92-105):
 * workspace detail keeps the whole trimmed value; surface detail adds the
 * delimiter-split components.
 */
function branchTokens(rawBranch: string, detail: MetadataDetail): string[] {
  const trimmed = trimWhitespaceAndNewlines(rawBranch);
  if (trimmed === "") {
    return [];
  }
  if (detail === "workspace") {
    return [trimmed];
  }
  return uniqueNormalizedPreservingOrder([trimmed, ...splitOnMetadataDelimiters(trimmed)]);
}

/**
 * Port tokens. Mirrors `portTokensForSearch` (Indexer.swift:107-111): an
 * integer in 1...65535 yields `["3000", ":3000"]`, anything else nothing.
 * The `Number.isInteger` gate makes Swift's `Int` typing explicit.
 */
function portTokens(port: number): string[] {
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    return [];
  }
  const portText = String(port);
  return [portText, `:${portText}`];
}

/**
 * ICU `\s` as used by NSRegularExpression's `"\\s+"` in
 * `descriptionTokensForSearch` (Indexer.swift:116-120):
 * `[\t\n\f\r\p{Z}]`. Deliberately NOT JS `\s`, which includes
 * U+FEFF (ICU does not) and the same-but-different shorthand would drift.
 * U+000B (vertical tab) and U+0085 (NEL) are General Category Cc, NOT in
 * `\p{Z}`, and ICU's `\s` does NOT match them — so they are excluded here.
 * An interior VT/NEL therefore survives (matching Swift) instead of
 * collapsing to a space.
 */
const ICU_WHITESPACE_RUN = /[\t\n\f\r\p{Z}]+/gu;

/**
 * Description tokens. Mirrors `descriptionTokensForSearch`
 * (Indexer.swift:113-123): trim, collapse ICU whitespace runs to single
 * spaces, then index the trimmed original, the collapsed form, and the
 * delimiter-split components.
 */
function descriptionTokens(rawDescription: string | null | undefined): string[] {
  const trimmed = trimWhitespaceAndNewlines(rawDescription ?? "");
  if (trimmed === "") {
    return [];
  }
  const normalizedWhitespace = trimmed.replace(ICU_WHITESPACE_RUN, " ");
  const components = splitOnMetadataDelimiters(normalizedWhitespace);
  return uniqueNormalizedPreservingOrder([trimmed, normalizedWhitespace, ...components]);
}

/**
 * Metadata-derived keywords. Mirrors `metadataKeywordsForSearch`
 * (Indexer.swift:44-68): every non-empty token family gates its fixed
 * context words, and ALL context words come first (in
 * directory/branch/port/description order), then the token groups in the
 * same order.
 */
function metadataKeywords(
  metadata: SwitcherSearchMetadata,
  detail: MetadataDetail,
  homeDir?: string,
): string[] {
  const dirTokens = (metadata.directories ?? []).flatMap((d) => directoryTokens(d, detail, homeDir));
  const brTokens = (metadata.branches ?? []).flatMap((b) => branchTokens(b, detail));
  const prtTokens = (metadata.ports ?? []).flatMap(portTokens);
  const descTokens = descriptionTokens(metadata.description);

  const contextKeywords: string[] = [];
  if (dirTokens.length > 0) {
    contextKeywords.push("directory", "dir", "cwd", "path");
  }
  if (brTokens.length > 0) {
    contextKeywords.push("branch", "git");
  }
  if (prtTokens.length > 0) {
    contextKeywords.push("port", "ports");
  }
  if (descTokens.length > 0) {
    contextKeywords.push("description", "descriptions", "notes", "note");
  }

  return [...contextKeywords, ...dirTokens, ...brTokens, ...prtTokens, ...descTokens];
}

/**
 * The unique, order-preserving keyword list for one switcher entry. Mirrors
 * the `keywords` property (Indexer.swift:39-42).
 *
 * Caller contract (for the future entries-builder port,
 * ContentView.swift:5275-5331): workspace entries pass baseKeywords
 * `["workspace", "switch", "go", "open", workspaceName, ...windowKeywords]`
 * with detail "workspace"; surface entries pass `["surface", "tab",
 * "switch", "go", "open", surfaceName, workspaceName,
 * ...surfaceKindKeywords, ...windowKeywords]` with detail "surface".
 */
export function switcherSearchKeywords(
  baseKeywords: string[],
  metadata: SwitcherSearchMetadata,
  detail: MetadataDetail,
  env: SwitcherIndexEnv = {},
): string[] {
  return uniqueNormalizedPreservingOrder([
    ...baseKeywords,
    ...metadataKeywords(metadata, detail, env.homeDir),
  ]);
}
