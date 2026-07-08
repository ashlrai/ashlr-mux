// Directional-split plan: maps a canonical split direction onto the session
// command's (orientation, insertFirst) pair — the JS caller half of C1 (the
// Rust `session_split` command has threaded `insert_first` end-to-end since
// `48a685739`).
//
// Parity (macOS bonsplit `SplitDirection` → insertion side): splitting LEFT or
// UP places the NEW pane before the existing one (`insertFirst: true`);
// splitting RIGHT or DOWN appends it after (`insertFirst: false`, the
// historical default). `horizontal` lays children side-by-side (first = left),
// `vertical` stacks them (first = top) — see `session/paneRects.ts`.

import type { SessionSplitOrientation } from "@cmux/core-types";

export type SplitDirection = "left" | "right" | "up" | "down";

export interface DirectionalSplitPlan {
  orientation: SessionSplitOrientation;
  insertFirst: boolean;
}

/** The (orientation, insertFirst) pair `session_split` needs for `direction`. */
export function directionalSplitPlan(
  direction: SplitDirection,
): DirectionalSplitPlan {
  switch (direction) {
    case "left":
      return { orientation: "horizontal", insertFirst: true };
    case "right":
      return { orientation: "horizontal", insertFirst: false };
    case "up":
      return { orientation: "vertical", insertFirst: true };
    case "down":
      return { orientation: "vertical", insertFirst: false };
  }
}
