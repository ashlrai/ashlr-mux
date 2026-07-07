import { describe, expect, it } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { CommandPaletteOverlay } from "./CommandPaletteOverlay";

// The overlay owns its own visibility (starts hidden until an open shortcut),
// so a server render — where effects never fire — must produce nothing and, more
// importantly, must not throw while `useCommandPalette` composes the ported
// catalog/switcher/scope modules and the session hook during the render pass.
describe("CommandPaletteOverlay", () => {
  it("renders nothing (and does not throw) while hidden", () => {
    const html = renderToStaticMarkup(<CommandPaletteOverlay />);
    expect(html).toBe("");
  });
});
