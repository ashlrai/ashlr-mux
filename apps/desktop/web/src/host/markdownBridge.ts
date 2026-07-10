import { host } from "./host";

/**
 * Thin typed wrappers over the three markdown Tauri commands
 * (`src-tauri/src/markdown.rs`):
 *
 *  - `markdown_set_document(path, panelId)` binds the panel's document path
 *    (the local-image jail root) before any render,
 *  - `markdown_render(markdown, panelId)` pushes a document into the panel's
 *    sandboxed markdown iframe (`__cmuxRenderMarkdown`), and
 *  - `markdown_apply_theme(background, panelId)` derives + applies a theme from the
 *    panel's 8-bit sRGB background color (`__cmuxApplyTheme`), and
 *  - `markdown_apply_typography(panelId)` applies persisted markdown typography
 *    defaults (`__cmuxApplyTypography`), and
 *  - `markdown_zoom_in|out|reset(panelId)` adjust the panel's persisted zoom.
 *
 * The render/theme pair are fire-and-forget on the Rust side; these wrappers
 * only shape the argument objects the way Tauri's camelCase→snake_case mapping
 * expects (`markdown` and `background` are single words, so they pass through
 * unchanged) and await the (void) reply. `invoke` is injectable so call sites can
 * be shaping-tested with a double, mirroring `host.test.ts`.
 */

/**
 * A non-unwrapping-agnostic invoke seam. Non-generic on purpose: a concrete test
 * double cannot implement a generic call signature, so keeping this monomorphic
 * lets a plain `async (channel, payload) => …` stand in (same rationale as
 * `MacHostShimOptions.invokeRaw` in `host.ts`).
 */
export type MarkdownInvoke = (
  channel: string,
  payload?: Record<string, unknown>,
) => Promise<unknown>;

/** The sRGB background color a markdown theme is derived from (`[r, g, b]`, 0–255). */
export type MarkdownThemeBackground = readonly [number, number, number];

/**
 * Bind a markdown panel's document `path` — the root the Rust-side
 * local-image jail resolves `cmux-local-image://` requests against. Port of
 * `Coordinator.bind`'s `filePath` assignment (`MarkdownWebRenderer.swift:190-194`).
 * `panelId` is required because the Windows port hosts several markdown iframes
 * inside one top-level Tauri webview; the native side keys both the markdown
 * jail state and iframe delivery by that stable panel id.
 */
export async function setMarkdownDocument(
  panelId: string,
  path: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_set_document", { panelId, path });
}

/** A markdown document plus the file path its relative resources resolve against. */
export interface MarkdownDocument {
  path: string;
  markdown: string;
}

/** Push a markdown `document` into the calling panel's markdown webview. */
export async function renderMarkdown(
  panelId: string,
  document: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_render", { panelId, markdown: document });
}

/** Apply a markdown theme derived from the panel's `background` sRGB color. */
export async function applyMarkdownTheme(
  panelId: string,
  background: MarkdownThemeBackground,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_apply_theme", { panelId, background });
}

export async function applyMarkdownTypography(
  panelId: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_apply_typography", { panelId });
}

export async function zoomMarkdownIn(
  panelId: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_zoom_in", { panelId });
}

export async function zoomMarkdownOut(
  panelId: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_zoom_out", { panelId });
}

export async function resetMarkdownZoom(
  panelId: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_zoom_reset", { panelId });
}

/**
 * Feed a full document: set-document, **awaited**, then render. The single
 * composed entrypoint keeps the ordering un-invertible — the jail path must be
 * bound before the render dispatch (markdown.rs:299-301; Swift performs both in
 * one `updateNSView` pass, `MarkdownWebRenderer.swift:99-106`). Always
 * set-then-render, even when only the markdown changed: canonical re-binds on
 * every pass, and the Rust write is field-scoped (overwrites `file_path` only,
 * `requested_libs` untouched).
 */
export async function renderMarkdownDocument(
  panelId: string,
  doc: MarkdownDocument,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await setMarkdownDocument(panelId, doc.path, invoke);
  await renderMarkdown(panelId, doc.markdown, invoke);
}
