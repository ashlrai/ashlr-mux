import type { CSSProperties } from "react";

export function swiftToken(value: string | undefined, fallback: string): string {
  return (value ?? fallback)
    .replace(/^(?:\.|SwiftUI\.|Color\.|Text\.|Image\.|SymbolVariants\.)/, "")
    .replace(/[^a-zA-Z0-9_-]/g, "")
    .trim();
}

export function swiftRedactionReasons(value: string | undefined): string | undefined {
  const tokens = (value ?? ".placeholder")
    .split(/[^a-zA-Z0-9_]+/)
    .map((part) => swiftToken(part, ""))
    .filter((part) => part.length > 0 && part !== "RedactionReasons");
  return tokens.length > 0 ? [...new Set(tokens)].join(",") : undefined;
}

export function swiftTabViewStyleToken(value: string | undefined): string {
  const token = swiftToken(value, "automatic");
  if (token === "page" || token === "PageTabViewStyle") return "page";
  return token || "automatic";
}

export function swiftTransformOrigin(anchor: string): string {
  switch (anchor) {
    case "top":
      return "center top";
    case "bottom":
      return "center bottom";
    case "leading":
      return "left center";
    case "trailing":
      return "right center";
    case "topLeading":
      return "left top";
    case "topTrailing":
      return "right top";
    case "bottomLeading":
      return "left bottom";
    case "bottomTrailing":
      return "right bottom";
    default:
      return "center center";
  }
}

export function swiftKeyboardShortcutAria(
  key: string | undefined,
  modifiers: string | undefined,
): string | undefined {
  if (key === undefined || key.trim() === "") return undefined;
  const parts: string[] = [];
  for (const modifier of (modifiers ?? "").split(",").filter(Boolean)) {
    switch (modifier) {
      case "command":
        parts.push("Meta");
        break;
      case "option":
        parts.push("Alt");
        break;
      case "control":
        parts.push("Control");
        break;
      case "shift":
        parts.push("Shift");
        break;
      default:
        break;
    }
  }
  parts.push(key);
  return parts.join("+");
}

export function swiftStackAlignmentClass(value: string | undefined): string {
  return value === undefined
    ? ""
    : ` cmux-custom-sidebar-swift-stack-alignment-${swiftToken(value, "center")}`;
}

export function swiftLazyStackClass(value: boolean | undefined): string {
  return value === true ? " cmux-custom-sidebar-swift-stack-lazy" : "";
}

export function swiftPinnedViewsClass(value: string | undefined): string {
  return value === undefined
    ? ""
    : value
        .split(",")
        .map((token) => ` cmux-custom-sidebar-swift-pinned-${swiftToken(token, "view")}`)
        .join("");
}

export function swiftHorizontalAlignmentToken(
  value: string | undefined,
): "leading" | "center" | "trailing" | undefined {
  const token = swiftToken(value, "");
  if (token === "leading" || token === "center" || token === "trailing") return token;
  return undefined;
}

export function swiftGridSelfAlignment(
  value: "leading" | "center" | "trailing",
): CSSProperties["justifySelf"] {
  if (value === "leading") return "start";
  if (value === "trailing") return "end";
  return "center";
}

export function swiftGridCellAnchorToken(value: string | undefined): string | undefined {
  const token = swiftToken(value, "");
  switch (token) {
    case "center":
    case "top":
    case "bottom":
    case "leading":
    case "trailing":
    case "topLeading":
    case "topTrailing":
    case "bottomLeading":
    case "bottomTrailing":
      return token;
    default:
      return undefined;
  }
}

export function swiftGridCellAnchorPlaceSelf(value: string): CSSProperties["placeSelf"] {
  const vertical = swiftVerticalAlignment(value).grid;
  const horizontal = swiftHorizontalAlignment(value).grid;
  return `${vertical} ${horizontal}`;
}

export function swiftAlignmentGuideIsVertical(value: string | undefined): boolean {
  const token = swiftToken(value, "center");
  return (
    token === "top" ||
    token === "bottom" ||
    token === "firstTextBaseline" ||
    token === "lastTextBaseline"
  );
}

export function swiftStackAlignmentStyle(
  axis: "vertical" | "horizontal" | "zstack",
  value: string | undefined,
): CSSProperties {
  if (value === undefined) return {};
  const horizontal = swiftHorizontalAlignment(value);
  const vertical = swiftVerticalAlignment(value);
  if (axis === "zstack") {
    return {
      placeItems: `${vertical.grid} ${horizontal.grid}`,
    };
  }
  if (axis === "vertical") {
    return {
      alignItems: horizontal.flex,
      ...(vertical.explicit ? { justifyContent: vertical.flex } : {}),
    };
  }
  return {
    alignItems: vertical.flex,
    ...(horizontal.explicit ? { justifyContent: horizontal.flex } : {}),
  };
}

export function swiftHorizontalAlignment(value: string): {
  flex: CSSProperties["alignItems"];
  grid: "start" | "center" | "end";
  explicit: boolean;
} {
  const token = swiftToken(value, "center");
  if (token === "leading" || token.endsWith("Leading")) {
    return { flex: "flex-start", grid: "start", explicit: true };
  }
  if (token === "trailing" || token.endsWith("Trailing")) {
    return { flex: "flex-end", grid: "end", explicit: true };
  }
  return { flex: "center", grid: "center", explicit: token === "center" };
}

export function swiftVerticalAlignment(value: string): {
  flex: CSSProperties["alignItems"];
  grid: "start" | "center" | "end";
  explicit: boolean;
} {
  const token = swiftToken(value, "center");
  if (token === "top" || token.startsWith("top")) {
    return { flex: "flex-start", grid: "start", explicit: true };
  }
  if (token === "bottom" || token.startsWith("bottom")) {
    return { flex: "flex-end", grid: "end", explicit: true };
  }
  return { flex: "center", grid: "center", explicit: token === "center" };
}

export function swiftFontStyle(value: string | undefined): CSSProperties {
  switch (value?.replace(/^\./, "")) {
    case "largeTitle":
      return { fontSize: "22px", fontWeight: 780, letterSpacing: "-0.02em" };
    case "title":
      return { fontSize: "20px", fontWeight: 760, letterSpacing: "-0.02em" };
    case "title2":
      return { fontSize: "18px", fontWeight: 740, letterSpacing: "-0.01em" };
    case "title3":
      return { fontSize: "16px", fontWeight: 720 };
    case "headline":
      return { fontSize: "14px", fontWeight: 760 };
    case "subheadline":
      return { fontSize: "12px", fontWeight: 650 };
    case "caption":
      return { fontSize: "11px", lineHeight: 1.35 };
    case "caption2":
      return { fontSize: "10px", lineHeight: 1.3 };
    case "body":
    default:
      return {};
  }
}

export function swiftFontWeight(value: string | undefined): CSSProperties["fontWeight"] {
  switch (swiftToken(value, "regular")) {
    case "ultraLight":
      return 200;
    case "thin":
      return 250;
    case "light":
      return 300;
    case "regular":
      return 400;
    case "medium":
      return 520;
    case "semibold":
      return 650;
    case "bold":
      return 760;
    case "heavy":
      return 820;
    case "black":
      return 880;
    default: {
      const numeric = Number(value);
      return Number.isFinite(numeric) ? Math.max(1, Math.min(1000, numeric)) : 400;
    }
  }
}

export function swiftColor(value: string | undefined): string {
  switch (value?.replace(/^(?:\.|Color\.)/, "")) {
    case "primary":
      return "#f4fffb";
    case "secondary":
      return "#8ea8a2";
    case "tertiary":
      return "#6f8580";
    case "quaternary":
      return "#536661";
    case "quinary":
      return "#40514d";
    case "accent":
    case "accentColor":
      return "#2dd4bf";
    case "red":
      return "#fda4af";
    case "orange":
      return "#fdba74";
    case "yellow":
      return "#fde68a";
    case "green":
      return "#86efac";
    case "blue":
      return "#93c5fd";
    case "mint":
      return "#6ee7b7";
    case "indigo":
      return "#a5b4fc";
    case "purple":
      return "#c4b5fd";
    case "pink":
      return "#f9a8d4";
    case "brown":
      return "#d6a77a";
    case "teal":
    case "cyan":
      return "#67e8f9";
    case "gray":
      return "#94a3b8";
    case "white":
      return "#ffffff";
    case "black":
      return "#020617";
    default:
      return "#dceee9";
  }
}

export function applySwiftForegroundStyle(
  style: CSSProperties,
  classes: string[],
  value: string | undefined,
): void {
  const gradient = swiftGradientStyle(value);
  if (gradient !== undefined) {
    style.background = gradient;
    style.color = "transparent";
    (style as CSSProperties & Record<string, string>).WebkitBackgroundClip = "text";
    (style as CSSProperties & Record<string, string>).backgroundClip = "text";
    (style as CSSProperties & Record<string, string>).WebkitTextFillColor = "transparent";
    classes.push("cmux-custom-sidebar-swift-gradient-foreground");
    return;
  }
  const hierarchy = swiftHierarchyToken(value);
  if (hierarchy !== undefined) {
    classes.push(`cmux-custom-sidebar-swift-foreground-${hierarchy}`);
  }
  style.color = swiftColor(value);
}

export function swiftHierarchyToken(value: string | undefined): string | undefined {
  const token = value?.replace(/^(?:\.|Color\.|ShapeStyle\.)/, "");
  switch (token) {
    case "primary":
    case "secondary":
    case "tertiary":
    case "quaternary":
    case "quinary":
      return token;
    default:
      return undefined;
  }
}

export function swiftShadowColor(value: string | undefined): string {
  if (value === undefined) return "rgba(0, 0, 0, 0.32)";
  const token = value.replace(/^(?:\.|Color\.)/, "");
  if (token === "black") return "rgba(0, 0, 0, 0.42)";
  if (token === "white") return "rgba(255, 255, 255, 0.2)";
  return swiftColor(value);
}

export function swiftTextAlign(value: string | undefined): CSSProperties["textAlign"] {
  switch (value?.replace(/^\./, "")) {
    case "center":
      return "center";
    case "trailing":
    case "right":
      return "right";
    case "leading":
    case "left":
    default:
      return "left";
  }
}

export function swiftTextDecorationStyle(
  value: string | undefined,
): CSSProperties["textDecorationStyle"] {
  switch (swiftToken(value, "solid")) {
    case "dot":
      return "dotted";
    case "dash":
    case "dashDot":
    case "dashDotDot":
      return "dashed";
    case "solid":
    default:
      return "solid";
  }
}

export function swiftBlendMode(value: string | undefined): CSSProperties["mixBlendMode"] | undefined {
  switch (swiftToken(value, "normal")) {
    case "normal":
      return "normal";
    case "multiply":
      return "multiply";
    case "screen":
      return "screen";
    case "overlay":
      return "overlay";
    case "darken":
      return "darken";
    case "lighten":
      return "lighten";
    case "colorDodge":
      return "color-dodge";
    case "colorBurn":
      return "color-burn";
    case "softLight":
      return "soft-light";
    case "hardLight":
      return "hard-light";
    case "difference":
      return "difference";
    case "exclusion":
      return "exclusion";
    case "hue":
      return "hue";
    case "saturation":
      return "saturation";
    case "color":
      return "color";
    case "luminosity":
      return "luminosity";
    default:
      return undefined;
  }
}

export function swiftBackgroundStyle(value: string | undefined): string {
  return swiftGradientStyle(value) ?? swiftMaterialBackgroundStyle(value) ?? swiftBackgroundColor(value);
}

export function swiftBackgroundColor(value: string | undefined): string {
  switch (value?.replace(/^(?:\.|Color\.)/, "")) {
    case "secondary":
    case "gray":
      return "rgba(148, 163, 184, 0.13)";
    case "tertiary":
      return "rgba(111, 133, 128, 0.14)";
    case "quaternary":
    case "quinary":
      return "rgba(83, 102, 97, 0.14)";
    case "accent":
    case "accentColor":
      return "rgba(45, 212, 191, 0.15)";
    case "red":
      return "rgba(244, 63, 94, 0.14)";
    case "orange":
      return "rgba(249, 115, 22, 0.14)";
    case "yellow":
      return "rgba(234, 179, 8, 0.13)";
    case "green":
      return "rgba(34, 197, 94, 0.14)";
    case "blue":
      return "rgba(59, 130, 246, 0.14)";
    case "mint":
      return "rgba(52, 211, 153, 0.14)";
    case "indigo":
      return "rgba(99, 102, 241, 0.15)";
    case "purple":
      return "rgba(139, 92, 246, 0.15)";
    case "brown":
      return "rgba(146, 64, 14, 0.16)";
    case "teal":
    case "cyan":
      return "rgba(20, 184, 166, 0.15)";
    default:
      return "rgba(15, 23, 42, 0.45)";
  }
}

export function swiftMaterialToken(value: string | undefined): string | undefined {
  const token = value?.replace(/^(?:\.|Material\.)/, "");
  switch (token) {
    case "ultraThinMaterial":
    case "thinMaterial":
    case "regularMaterial":
    case "thickMaterial":
    case "ultraThickMaterial":
    case "bar":
      return token;
    default:
      return undefined;
  }
}

export function swiftMaterialBackgroundStyle(value: string | undefined): string | undefined {
  switch (swiftMaterialToken(value)) {
    case "ultraThinMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.08), rgba(15, 23, 42, 0.22))";
    case "thinMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.1), rgba(15, 23, 42, 0.3))";
    case "regularMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.13), rgba(15, 23, 42, 0.42))";
    case "thickMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.16), rgba(15, 23, 42, 0.56))";
    case "ultraThickMaterial":
      return "linear-gradient(135deg, rgba(226, 232, 240, 0.2), rgba(15, 23, 42, 0.68))";
    case "bar":
      return "linear-gradient(180deg, rgba(15, 23, 42, 0.78), rgba(2, 6, 23, 0.66))";
    default:
      return undefined;
  }
}

export function swiftMaterialBackdropFilter(value: string | undefined): string {
  switch (swiftMaterialToken(value)) {
    case "ultraThinMaterial":
      return "blur(18px) saturate(1.12)";
    case "thinMaterial":
      return "blur(16px) saturate(1.16)";
    case "thickMaterial":
      return "blur(12px) saturate(1.28)";
    case "ultraThickMaterial":
      return "blur(10px) saturate(1.36)";
    case "bar":
      return "blur(14px) saturate(1.22)";
    case "regularMaterial":
    default:
      return "blur(14px) saturate(1.2)";
  }
}

export function swiftContainerRelativeFrameSize(
  count: number | undefined,
  span: number | undefined,
  spacing: number | undefined,
): string {
  if (count === undefined) return "100%";
  const safeSpan = Math.min(span ?? 1, count);
  const percentage = (safeSpan / count) * 100;
  if (spacing === undefined || spacing === 0 || count <= 1) {
    return `${percentage}%`;
  }
  return `calc(${percentage}% - ${spacing}px)`;
}

export function swiftFontWidth(value: string | undefined): CSSProperties["fontStretch"] | undefined {
  switch (swiftToken(value, "standard")) {
    case "compressed":
      return "75%";
    case "condensed":
      return "87.5%";
    case "standard":
      return "normal";
    case "expanded":
      return "112.5%";
    default:
      return undefined;
  }
}

export function swiftDynamicTypeFontSize(token: string): CSSProperties["fontSize"] | undefined {
  switch (token) {
    case "xSmall":
      return "0.82rem";
    case "small":
      return "0.88rem";
    case "medium":
      return "1rem";
    case "large":
      return "1.06rem";
    case "xLarge":
      return "1.18rem";
    case "xxLarge":
      return "1.3rem";
    case "xxxLarge":
      return "1.44rem";
    case "accessibility1":
      return "1.6rem";
    case "accessibility2":
      return "1.78rem";
    case "accessibility3":
      return "1.98rem";
    case "accessibility4":
      return "2.2rem";
    case "accessibility5":
      return "2.44rem";
    default:
      return undefined;
  }
}

export function swiftGradientStyle(value: string | undefined): string | undefined {
  const expression = value?.trim();
  if (!expression) return undefined;
  const colorGradient = expression.match(/^(?:\.|Color\.)?([A-Za-z]+)\.gradient$/);
  if (colorGradient) {
    const color = swiftColor(colorGradient[1]);
    return `linear-gradient(135deg, ${color}, ${swiftTransparentColor(color)})`;
  }

  const call = expression.match(/^([A-Za-z]+Gradient)\(([\s\S]*)\)$/);
  if (!call) return undefined;
  const name = call[1];
  const args = call[2] ?? "";
  const colors = swiftGradientColors(args);
  if (colors.length === 0) return undefined;

  if (name === "LinearGradient") {
    const endPoint = swiftNamedArg(args, "endPoint") ?? ".bottom";
    return `linear-gradient(${swiftLinearGradientDirection(endPoint)}, ${colors.join(", ")})`;
  }
  if (name === "RadialGradient") {
    return `radial-gradient(circle, ${colors.join(", ")})`;
  }
  if (name === "AngularGradient") {
    return `conic-gradient(${colors.join(", ")})`;
  }
  return undefined;
}

export function swiftGradientColors(args: string): string[] {
  const directColors = swiftNamedArg(args, "colors");
  const colorsSource =
    directColors ??
    (() => {
      const gradientArg = swiftNamedArg(args, "gradient");
      if (gradientArg === undefined) return undefined;
      const match = gradientArg.trim().match(/^Gradient\(([\s\S]*)\)$/);
      return match ? swiftNamedArg(match[1] ?? "", "colors") : undefined;
    })();
  if (colorsSource === undefined) return [];
  const trimmed = colorsSource.trim();
  if (!trimmed.startsWith("[") || !trimmed.endsWith("]")) return [];
  return swiftSplitTopLevel(trimmed.slice(1, -1), ",")
    .map((color) => swiftColor(color.trim()))
    .slice(0, 8);
}

export function swiftLinearGradientDirection(endPoint: string): string {
  const token = endPoint.replace(/^(?:\.|UnitPoint\.)/, "");
  switch (token) {
    case "top":
      return "to top";
    case "topLeading":
    case "topLeft":
      return "to top left";
    case "topTrailing":
    case "topRight":
      return "to top right";
    case "leading":
    case "left":
      return "to left";
    case "trailing":
    case "right":
      return "to right";
    case "bottomLeading":
    case "bottomLeft":
      return "to bottom left";
    case "bottomTrailing":
    case "bottomRight":
      return "to bottom right";
    case "bottom":
    default:
      return "to bottom";
  }
}

export function swiftTransparentColor(color: string): string {
  if (color.startsWith("#") && color.length === 7) {
    return `${color}33`;
  }
  return "rgba(15, 23, 42, 0.2)";
}

export function swiftNamedArg(args: string, label: string): string | undefined {
  for (const arg of swiftSplitTopLevel(args, ",")) {
    const colon = swiftTopLevelIndexOf(arg, ":");
    if (colon < 0) continue;
    if (arg.slice(0, colon).trim() === label) {
      return arg.slice(colon + 1).trim();
    }
  }
  return undefined;
}

export function swiftSplitTopLevel(text: string, separator: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote = false;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index] ?? "";
    if (char === '"' && text[index - 1] !== "\\") quote = !quote;
    if (quote) continue;
    if (char === "(" || char === "[" || char === "{") depth += 1;
    if (char === ")" || char === "]" || char === "}") depth -= 1;
    if (depth === 0 && text.startsWith(separator, index)) {
      parts.push(text.slice(start, index).trim());
      index += separator.length - 1;
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts.filter((part) => part !== "");
}

export function swiftTopLevelIndexOf(text: string, needle: string): number {
  let depth = 0;
  let quote = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index] ?? "";
    if (char === '"' && text[index - 1] !== "\\") quote = !quote;
    if (quote) continue;
    if (char === "(" || char === "[" || char === "{") depth += 1;
    if (char === ")" || char === "]" || char === "}") depth -= 1;
    if (depth === 0 && text.startsWith(needle, index)) return index;
  }
  return -1;
}

export function swiftSystemImageGlyph(systemName: string): string {
  const normalized = systemName.toLowerCase();
  if (normalized.includes("folder")) return "folder";
  if (normalized.includes("terminal")) return "term";
  if (normalized.includes("bolt")) return "bolt";
  if (normalized.includes("checkmark")) return "check";
  if (normalized.includes("circle")) return "dot";
  if (normalized.includes("exclamation")) return "!";
  if (normalized.includes("clock")) return "time";
  if (normalized.includes("gear")) return "gear";
  return systemName.replace(/\..*$/, "") || "icon";
}
