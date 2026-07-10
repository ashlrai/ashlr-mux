import { describe, expect, test } from "bun:test";

import {
  agentSessionThemeForColorScheme,
  emitAgentSessionTheme,
} from "./agentTheme";

describe("agent session theme bridge", () => {
  test("maps shell dark mode to the existing agent-session dark token shape", () => {
    const theme = agentSessionThemeForColorScheme("dark");

    expect(theme.isDark).toBe(true);
    expect(theme.pageBackground).toBe("transparent");
    expect(theme.surfaceBackground).toBe("rgba(28, 31, 27, 0.34)");
    expect(theme.text).toBe("#f1f0e8");
  });

  test("maps shell light mode to a light agent-session token shape", () => {
    const theme = agentSessionThemeForColorScheme("light");

    expect(theme.isDark).toBe(false);
    expect(theme.pageBackground).toBe("transparent");
    expect(theme.surfaceBackground).toBe("rgba(250, 248, 240, 0.78)");
    expect(theme.text).toBe("#20242a");
  });

  test("emits app.theme through the reused cmuxAgentBridge when present", () => {
    const received: unknown[] = [];

    emitAgentSessionTheme("light", {
      cmuxAgentBridge: {
        receive: (event) => received.push(event),
      },
    });

    expect(received).toEqual([
      {
        type: "app.theme",
        theme: agentSessionThemeForColorScheme("light"),
      },
    ]);
  });

  test("is a safe no-op before the reused agent bridge installs", () => {
    expect(() => emitAgentSessionTheme("dark", {})).not.toThrow();
    expect(() => emitAgentSessionTheme("dark", undefined)).not.toThrow();
  });
});
