import { host } from "./host";

/**
 * Thin typed wrappers over the three markdown Tauri commands
 * (`src-tauri/src/markdown.rs`):
 *
 *  - `markdown_set_document(path)` binds the calling webview's document path
 *    (the local-image jail root) before any render,
 *  - `markdown_render(markdown)` pushes a document into the calling panel's
 *    markdown webview (`__cmuxRenderMarkdown`), and
 *  - `markdown_apply_theme(background)` derives + applies a theme from the
 *    panel's 8-bit sRGB background color (`__cmuxApplyTheme`).
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
 * Bind the calling webview's markdown document `path` — the root the Rust-side
 * local-image jail resolves `cmux-local-image://` requests against. Port of
 * `Coordinator.bind`'s `filePath` assignment (`MarkdownWebRenderer.swift:190-194`).
 * No label argument: the Rust command keys state by `webview.label()` server-side
 * (markdown.rs:317), and the sandboxed markdown iframe lives inside the main
 * window webview, so its subresource requests carry the same label this write
 * lands under. (All panes share that one label, so two simultaneously fed
 * markdown panes would share one jail path — matching the per-webview isolation
 * note at markdown.rs:12-14; today one document is fed per invoke.)
 */
export async function setMarkdownDocument(
  path: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_set_document", { path });
}

/** A markdown document plus the file path its relative resources resolve against. */
export interface MarkdownDocument {
  path: string;
  markdown: string;
}

/** Push a markdown `document` into the calling panel's markdown webview. */
export async function renderMarkdown(
  document: string,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_render", { markdown: document });
}

/** Apply a markdown theme derived from the panel's `background` sRGB color. */
export async function applyMarkdownTheme(
  background: MarkdownThemeBackground,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await invoke("markdown_apply_theme", { background });
}

/**
 * Feed a full document: set-document, **awaited**, then render. The single
 * composed entrypoint keeps the ordering un-invertible — the jail path must be
 * bound before the render dispatch (markdown.rs:299-301; Swift performs both in
 * one `updateNSView` pass, `MarkdownWebRenderer.swift:99-106`). Always
 * set-then-render, even when only the markdown changed: canonical re-binds on
 * every pass, and the Rust write is field-scoped (overwrites `file_path` only,
 * `requested_libs` untouched).
 *
 * Note: `markdown_render`'s eval currently lands in the top-level webview
 * document, not the sandboxed cmux-md iframe where `__cmuxRenderMarkdown` is
 * defined — live delivery is the deferred GUI tail (markdown.rs:302-306). The
 * set→render sequencing contract here is correct independently of that.
 */
export async function renderMarkdownDocument(
  doc: MarkdownDocument,
  invoke: MarkdownInvoke = host.invoke,
): Promise<void> {
  await setMarkdownDocument(doc.path, invoke);
  await renderMarkdown(doc.markdown, invoke);
}
