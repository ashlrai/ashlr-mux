import { describe, expect, test } from "bun:test";

import {
  minimalModeTabStripCssVariables,
  minimalModeTabStripInsets,
  MINIMAL_MODE_CHROME_METRICS,
  windowsCaptionControlReservedInlineSize,
  WINDOWS_CAPTION_CONTROL_METRICS,
} from "./chromeMetrics";

describe("window chrome metrics", () => {
  test("minimal mode reserves the Windows caption side for top tab strips", () => {
    expect(windowsCaptionControlReservedInlineSize()).toBe(138);
    expect(minimalModeTabStripInsets({ minimalMode: true })).toEqual({
      topStripHeight: MINIMAL_MODE_CHROME_METRICS.titlebarHeight,
      inlineStart: 0,
      inlineEnd:
        WINDOWS_CAPTION_CONTROL_METRICS.buttonWidth *
        WINDOWS_CAPTION_CONTROL_METRICS.buttonCount,
    });
  });

  test("regular mode does not inset a tab strip", () => {
    expect(
      minimalModeTabStripInsets({
        minimalMode: false,
        captionControlSide: "inline-start",
        captionControlWidth: 999,
      }),
    ).toEqual({ topStripHeight: 0, inlineStart: 0, inlineEnd: 0 });
  });

  test("custom caption-side inputs are clamped and exported as CSS variables", () => {
    expect(
      minimalModeTabStripInsets({
        minimalMode: true,
        captionControlSide: "inline-start",
        captionControlWidth: Number.NaN,
      }),
    ).toEqual({
      topStripHeight: MINIMAL_MODE_CHROME_METRICS.titlebarHeight,
      inlineStart: 0,
      inlineEnd: 0,
    });

    expect(minimalModeTabStripCssVariables(true)).toEqual({
      "--cmux-minimal-tabstrip-height": "28px",
      "--cmux-minimal-tabstrip-inset-inline-start": "0px",
      "--cmux-minimal-tabstrip-inset-inline-end": "138px",
    });
  });
});
