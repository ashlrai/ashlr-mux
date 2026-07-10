import { describe, expect, test } from "bun:test";

import {
  buildCommandCatalog,
  type CommandContext,
} from "./commandCatalog";
import { buildSettingsToggleContributions } from "./settingsToggleContributions";
import { defaultSettingsConfig } from "../settings/defaultConfig";

const EMPTY_CTX: CommandContext = {};

describe("settingsToggleContributions", () => {
  function rowById(rows: ReturnType<typeof buildSettingsToggleContributions>, id: string) {
    const row = rows.find((candidate) => candidate.commandId === id);
    if (!row) {
      throw new Error(`missing settings toggle row ${id}`);
    }
    return row;
  }

  test("builds live settings-toggle rows with enable/disable titles and on/off subtitles", () => {
    const config = defaultSettingsConfig();
    config.sidebar!.hideAllDetails = false;
    config.notifications!.dockBadge = true;

    const rows = buildSettingsToggleContributions(config);
    const hideAllDetails = rowById(
      rows,
      "palette.toggleSetting.sidebar.hideAllDetails",
    );
    const dockBadge = rowById(
      rows,
      "palette.toggleSetting.notifications.dockBadge",
    );

    expect(hideAllDetails.title(EMPTY_CTX)).toBe("Enable Hide All Details");
    expect(hideAllDetails.subtitle(EMPTY_CTX)).toBe("Sidebar • Off");
    expect(hideAllDetails.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleSidebarFlag", key: "hideAllDetails" },
    });

    expect(dockBadge.title(EMPTY_CTX)).toBe("Disable Dock Badge");
    expect(dockBadge.subtitle(EMPTY_CTX)).toBe("Notifications • On");
    expect(dockBadge.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleNotificationsFlag", key: "dockBadge" },
    });
  });

  test("builds settings-toggle rows for app, automation, browser, terminal, canvas, and file editor booleans", () => {
    const config = defaultSettingsConfig();
    config.app!.menuBarOnly = true;
    config.app!.minimalMode = false;
    config.automation!.claudeCodeIntegration = false;
    config.automation!.codexIntegration = false;
    config.automation!.opencodeIntegration = false;
    config.automation!.workspaceAutoNaming = false;
    config.browser!.showSearchSuggestions = false;
    config.terminal!.copyOnSelect = true;
    config.terminal!.rendererRealization.enabled = false;
    config.canvas!.snappingEnabled = false;
    config.file_editor!.wordWrap = true;

    const rows = buildSettingsToggleContributions(config);
    const menuBarOnly = rowById(rows, "palette.toggleSetting.app.menuBarOnly");
    const minimalMode = rowById(rows, "palette.toggleSetting.app.minimalMode");
    const workspaceAutoNaming = rowById(
      rows,
      "palette.toggleSetting.automation.workspaceAutoNaming",
    );
    const claudeCodeIntegration = rowById(
      rows,
      "palette.toggleSetting.automation.claudeCodeIntegration",
    );
    const codexIntegration = rowById(
      rows,
      "palette.toggleSetting.automation.codexIntegration",
    );
    const opencodeIntegration = rowById(
      rows,
      "palette.toggleSetting.automation.opencodeIntegration",
    );
    const searchSuggestions = rowById(
      rows,
      "palette.toggleSetting.browser.showSearchSuggestions",
    );
    const copyOnSelect = rowById(
      rows,
      "palette.toggleSetting.terminal.copyOnSelect",
    );
    const rendererRealization = rowById(
      rows,
      "palette.toggleSetting.terminal.rendererRealization.enabled",
    );
    const snapping = rowById(rows, "palette.toggleSetting.canvas.snappingEnabled");
    const wordWrap = rowById(rows, "palette.toggleSetting.fileEditor.wordWrap");

    expect(menuBarOnly.title(EMPTY_CTX)).toBe("Disable Menu Bar Only");
    expect(menuBarOnly.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleAppFlag", key: "menuBarOnly" },
    });
    expect(minimalMode.title(EMPTY_CTX)).toBe("Enable Minimal Mode");
    expect(minimalMode.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleAppFlag", key: "minimalMode" },
    });
    expect(workspaceAutoNaming.title(EMPTY_CTX)).toBe(
      "Enable Workspace Auto Naming",
    );
    expect(workspaceAutoNaming.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleAutomationFlag", key: "workspaceAutoNaming" },
    });
    expect(claudeCodeIntegration.title(EMPTY_CTX)).toBe(
      "Install Claude Code Integration",
    );
    expect(claudeCodeIntegration.keywords).toEqual(
      expect.arrayContaining([
        "install",
        "setup",
        "hooks",
        "claude-code",
        "automation.claudeCodeIntegration",
      ]),
    );
    expect(claudeCodeIntegration.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleAutomationFlag", key: "claudeCodeIntegration" },
    });
    expect(codexIntegration.title(EMPTY_CTX)).toBe("Install Codex Integration");
    expect(codexIntegration.keywords).toEqual(
      expect.arrayContaining([
        "install",
        "setup",
        "hooks",
        "codex",
        "automation.codexIntegration",
      ]),
    );
    expect(codexIntegration.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleAutomationFlag", key: "codexIntegration" },
    });
    expect(opencodeIntegration.title(EMPTY_CTX)).toBe(
      "Install OpenCode Integration",
    );
    expect(opencodeIntegration.keywords).toEqual(
      expect.arrayContaining([
        "install",
        "setup",
        "hooks",
        "opencode",
        "automation.opencodeIntegration",
      ]),
    );
    expect(opencodeIntegration.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleAutomationFlag", key: "opencodeIntegration" },
    });
    expect(searchSuggestions.subtitle(EMPTY_CTX)).toBe("Browser • Off");
    expect(searchSuggestions.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleBrowserFlag", key: "showSearchSuggestions" },
    });
    expect(copyOnSelect.title(EMPTY_CTX)).toBe("Disable Copy on Select");
    expect(copyOnSelect.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleTerminalFlag", key: "copyOnSelect" },
    });
    expect(rendererRealization.title(EMPTY_CTX)).toBe(
      "Enable Renderer Realization",
    );
    expect(rendererRealization.intent).toEqual({
      kind: "toggleSetting",
      action: {
        type: "toggleTerminalNestedFlag",
        section: "rendererRealization",
        key: "enabled",
      },
    });
    expect(snapping.title(EMPTY_CTX)).toBe("Enable Canvas Snapping");
    expect(snapping.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleCanvasFlag", key: "snappingEnabled" },
    });
    expect(wordWrap.title(EMPTY_CTX)).toBe("Disable File Editor Word Wrap");
    expect(wordWrap.intent).toEqual({
      kind: "toggleSetting",
      action: { type: "toggleFileEditorFlag", key: "wordWrap" },
    });
  });

  test("sidebar detail-dependent rows disappear when hideAllDetails is enabled", () => {
    const config = defaultSettingsConfig();
    config.sidebar!.hideAllDetails = true;

    const ids = buildSettingsToggleContributions(config).map((row) => row.commandId);
    expect(ids).toContain("palette.toggleSetting.sidebar.hideAllDetails");
    expect(ids).not.toContain(
      "palette.toggleSetting.sidebar.showNotificationMessage",
    );
    expect(ids).not.toContain(
      "palette.toggleSetting.sidebar.openPullRequestLinksInCmuxBrowser",
    );
  });

  test("injected settings rows splice between browser toggles and workspace actions", () => {
    const catalog = buildCommandCatalog(
      { hasWorkspace: true },
      {
        dynamicContributions: {
          settingsToggle: buildSettingsToggleContributions(defaultSettingsConfig()),
        },
      },
    ).map((row) => row.id);

    const enableBrowser = catalog.indexOf("palette.enableBrowser");
    const firstToggle = catalog.indexOf("palette.toggleSetting.sidebar.hideAllDetails");
    const renameWorkspace = catalog.indexOf("palette.renameWorkspace");

    expect(firstToggle).toBeGreaterThan(enableBrowser);
    expect(firstToggle).toBeLessThan(renameWorkspace);
  });
});
