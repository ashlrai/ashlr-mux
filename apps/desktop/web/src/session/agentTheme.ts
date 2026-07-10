import type { AgentSessionTheme } from "@cmux/webviews/src/agent-session/shared/types";

import type { ColorScheme } from "../settings/appearanceMode";

interface AgentThemeBridgeWindow {
  cmuxAgentBridge?: {
    receive(event: unknown): void;
  };
}

const DARK_AGENT_THEME: AgentSessionTheme = {
  isDark: true,
  pageBackground: "transparent",
  surfaceBackground: "rgba(28, 31, 27, 0.34)",
  surfaceElevatedBackground: "rgba(28, 31, 27, 0.48)",
  inputBackground: "rgba(8, 10, 8, 0.36)",
  border: "rgba(233, 231, 216, 0.12)",
  borderStrong: "rgba(233, 231, 216, 0.22)",
  text: "#f1f0e8",
  mutedText: "rgba(241, 240, 232, 0.58)",
  softText: "rgba(241, 240, 232, 0.78)",
  accent: "#8ab4f8",
  accentSoft: "rgba(138, 180, 248, 0.2)",
  danger: "#ff8d7e",
  shadow: "rgba(0, 0, 0, 0.2)",
};

const LIGHT_AGENT_THEME: AgentSessionTheme = {
  isDark: false,
  pageBackground: "transparent",
  surfaceBackground: "rgba(250, 248, 240, 0.78)",
  surfaceElevatedBackground: "rgba(255, 253, 246, 0.92)",
  inputBackground: "rgba(255, 255, 255, 0.86)",
  border: "rgba(33, 38, 45, 0.12)",
  borderStrong: "rgba(33, 38, 45, 0.22)",
  text: "#20242a",
  mutedText: "rgba(32, 36, 42, 0.58)",
  softText: "rgba(32, 36, 42, 0.74)",
  accent: "#0b57d0",
  accentSoft: "rgba(11, 87, 208, 0.14)",
  danger: "#b3261e",
  shadow: "rgba(15, 23, 42, 0.16)",
};

export function agentSessionThemeForColorScheme(
  scheme: ColorScheme,
): AgentSessionTheme {
  return scheme === "dark" ? DARK_AGENT_THEME : LIGHT_AGENT_THEME;
}

export function emitAgentSessionTheme(
  scheme: ColorScheme,
  target: AgentThemeBridgeWindow | undefined =
    typeof window === "undefined"
      ? undefined
      : (window as unknown as AgentThemeBridgeWindow),
): void {
  target?.cmuxAgentBridge?.receive({
    type: "app.theme",
    theme: agentSessionThemeForColorScheme(scheme),
  });
}
