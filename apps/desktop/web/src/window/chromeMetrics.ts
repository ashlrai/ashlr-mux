export const WINDOW_CHROME_METRICS = {
  sharedChromeBarHeight: 28,
  appTitlebarHeight: 28,
  bonsplitTabBarHeight: 28,
  secondaryTitlebarHeight: 28,
  minimumTitlebarHeight: 28,
  maximumTitlebarHeight: 72,
  defaultTitlebarHeight: 28,
} as const;

export const MINIMAL_MODE_CHROME_METRICS = {
  titlebarHeight: WINDOW_CHROME_METRICS.appTitlebarHeight,
  macosTrafficLightTabBarLeadingInset: 80,
} as const;

export const HEADER_CHROME_CONTROL_METRICS = {
  buttonSize: 20,
  iconSize: 12,
  iconFrameSize: 14,
  cornerRadius: 6,
  titlebarControlsLeadingPadding: 4,
} as const;

export const WINDOWS_CAPTION_CONTROL_METRICS = {
  side: "inline-end",
  buttonWidth: 46,
  buttonCount: 3,
} as const;

export type CaptionControlSide = "inline-start" | "inline-end";

export interface MinimalModeTabStripInsets {
  topStripHeight: number;
  inlineStart: number;
  inlineEnd: number;
}

export interface MinimalModeTabStripInsetOptions {
  minimalMode: boolean;
  captionControlSide?: CaptionControlSide;
  captionControlWidth?: number;
}

function nonNegativeFinite(value: number, fallback: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : fallback;
}

export function windowsCaptionControlReservedInlineSize(): number {
  return (
    WINDOWS_CAPTION_CONTROL_METRICS.buttonWidth *
    WINDOWS_CAPTION_CONTROL_METRICS.buttonCount
  );
}

export function minimalModeTabStripInsets({
  minimalMode,
  captionControlSide = WINDOWS_CAPTION_CONTROL_METRICS.side,
  captionControlWidth = windowsCaptionControlReservedInlineSize(),
}: MinimalModeTabStripInsetOptions): MinimalModeTabStripInsets {
  if (!minimalMode) {
    return { topStripHeight: 0, inlineStart: 0, inlineEnd: 0 };
  }

  const reservedInlineSize = nonNegativeFinite(captionControlWidth, 0);
  return {
    topStripHeight: MINIMAL_MODE_CHROME_METRICS.titlebarHeight,
    inlineStart: captionControlSide === "inline-start" ? reservedInlineSize : 0,
    inlineEnd: captionControlSide === "inline-end" ? reservedInlineSize : 0,
  };
}

export function minimalModeTabStripCssVariables(
  minimalMode: boolean,
): Record<`--cmux-${string}`, string> {
  const insets = minimalModeTabStripInsets({ minimalMode });
  return {
    "--cmux-minimal-tabstrip-height": `${insets.topStripHeight}px`,
    "--cmux-minimal-tabstrip-inset-inline-start": `${insets.inlineStart}px`,
    "--cmux-minimal-tabstrip-inset-inline-end": `${insets.inlineEnd}px`,
  };
}
