import { host } from "./host";

/**
 * Thin typed wrappers over the two markdown Tauri commands
 * (`src-tauri/src/markdown.rs`):
 *
 *  - `markdown_render(markdown)` pushes a document into the calling panel's
 *    markdown webview (`__cmuxRenderMarkdown`), and
 *  - `markdown_apply_theme(background)` derives + applies a theme from the
 *    panel's 8-bit sRGB background color (`__cmuxApplyTheme`).
 *
 * Both are fire-and-forget on the Rust side (`Result<(), ()>`); these wrappers
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
