import { useCallback, useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";

import type {
  AppSessionSnapshot,
  Config,
  SessionWorkspaceLayoutSnapshot,
} from "@cmux/core-types";

import { CommandPaletteOverlay } from "./components/CommandPaletteOverlay";
import { DirectorySearchOverlay } from "./components/DirectorySearchOverlay";
import {
  FileExplorerPanel,
  type RightSidebarMode,
} from "./components/FileExplorerPanel";
import { NotificationsOverlay } from "./components/NotificationsOverlay";
import { SettingsOverlay } from "./components/SettingsOverlay";
import type {
  AgentProviderStatusView,
  BrowserImportDestinationProfileView,
  BrowserImportProfileView,
  CliInstallStatusView,
  ConfigExtensionStatusView,
  ControlSocketStatusView,
  DesktopCoreStatusView,
  DefaultTerminalStatusView,
  GlobalHotkeyStatusView,
  MobilePairingStatusView,
  RightSidebarBetaSettingsView,
  UpdaterStatusView,
} from "./components/SettingsPane";
import { Sidebar } from "./components/Sidebar";
import { WindowTitlebar } from "./components/WindowTitlebar";
import { Workspace } from "./components/Workspace";
import { host } from "./host/host";
import { useAppearance } from "./hooks/useAppearance";
import { useWindowChrome } from "./hooks/useWindowChrome";
import {
  rightSidebarStateFromRemote,
  type RightSidebarRemotePayload,
  type RightSidebarState,
} from "./rightSidebarModes";
import {
  configReducer,
  type ConfigAction,
} from "./settings/configReducer";
import type { BrowserImportStartRequest } from "./settings/browserImportPlan";
import { configMutationFromAction } from "./settings/configMutation";
import {
  DEFAULT_SIDEBAR_CONFIG,
  defaultSettingsConfig,
} from "./settings/defaultConfig";
import { notificationDeliverySettingsRequest } from "./settings/notificationDelivery";
import { emitAgentSessionTheme } from "./session/agentTheme";
import type { SettingsPaneSection } from "./settings/settingsSearchResults";
import { minimalModeTabStripCssVariables } from "./window/chromeMetrics";

interface NotificationCommandRunReply {
  ran: boolean;
  skipped_reason?: string | null;
  root_pid?: number | null;
}

interface NotificationDeliveryPreviewReply {
  decision?: string | null;
  toast_xml?: string | null;
  tag?: string | null;
  group?: string | null;
  launch?: string | null;
  run_command: boolean;
  play_in_app_sound: boolean;
}

interface NotificationToastSendReply {
  sent: boolean;
  tag?: string | null;
  group?: string | null;
  message: string;
}

interface ConfigChangedPayload {
  config: Config;
  path: string;
}

interface RawConfigFile {
  path: string;
  contents: string;
}

interface NotificationActivationTarget {
  workspaceId?: string;
  surfaceId?: string;
  panelId?: string;
}

function selectedPanelId(
  layout: SessionWorkspaceLayoutSnapshot | null | undefined,
): string | undefined {
  if (layout == null) {
    return undefined;
  }
  if (layout.type === "pane") {
    return layout.pane.selected_panel_id ?? layout.pane.panel_ids[0];
  }
  return selectedPanelId(layout.split.first) ?? selectedPanelId(layout.split.second);
}

function notificationActivationTargetFromSnapshot(
  snapshot: AppSessionSnapshot,
): NotificationActivationTarget {
  const windowSnapshot = snapshot.windows[0];
  const selectedIndex = windowSnapshot?.tab_manager.selected_workspace_index;
  if (selectedIndex == null || selectedIndex < 0) {
    return {};
  }
  const workspace = windowSnapshot.tab_manager.workspaces[selectedIndex];
  const workspaceId = workspace?.workspace_id;
  if (workspaceId == null) {
    return {};
  }
  const panelId = selectedPanelId(workspace.layout);
  return {
    workspaceId,
    ...(panelId == null ? {} : { surfaceId: panelId, panelId }),
  };
}

/**
 * The app shell. A custom native-backed title bar over a two-column body:
 * the left {@link Sidebar} (live workspace list) and the main {@link Workspace}
 * (the flat portal of terminal/agent/diff/markdown panes).
 */
export function App(): React.JSX.Element {
  const { applied, setStoredAppearance } = useAppearance();
  const windowChrome = useWindowChrome();
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTargetSection, setSettingsTargetSection] =
    useState<SettingsPaneSection | null>(null);
  const [settingsNavigationKey, setSettingsNavigationKey] = useState(0);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [directorySearchOpen, setDirectorySearchOpen] = useState(false);
  const [fileExplorerOpen, setFileExplorerOpen] = useState(false);
  const [rightSidebarMode, setRightSidebarMode] =
    useState<RightSidebarMode>("files");
  const [rightSidebarBetaSettings, setRightSidebarBetaSettings] =
    useState<RightSidebarBetaSettingsView | null>(null);
  const rightSidebarStateRef = useRef<RightSidebarState>({
    visible: false,
    mode: "files",
  });
  const [settingsConfig, setSettingsConfig] = useState<Config | null>(null);
  const [settingsSearchQuery, setSettingsSearchQuery] = useState("");
  const [settingsLoading, setSettingsLoading] = useState(false);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [rawSettingsPath, setRawSettingsPath] = useState<string | null>(null);
  const [rawSettingsDraft, setRawSettingsDraft] = useState("");
  const [rawSettingsLoading, setRawSettingsLoading] = useState(false);
  const [rawSettingsSaving, setRawSettingsSaving] = useState(false);
  const [rawSettingsError, setRawSettingsError] = useState<string | null>(null);
  const [rawSettingsStatus, setRawSettingsStatus] = useState<string | null>(null);
  const [notificationCommandTestStatus, setNotificationCommandTestStatus] =
    useState<string | null>(null);
  const [notificationDeliveryPreviewStatus, setNotificationDeliveryPreviewStatus] =
    useState<string | null>(null);
  const [notificationToastSendStatus, setNotificationToastSendStatus] =
    useState<string | null>(null);
  const [agentProviderStatus, setAgentProviderStatus] =
    useState<AgentProviderStatusView[] | null>(null);
  const [agentProviderStatusError, setAgentProviderStatusError] =
    useState<string | null>(null);
  const [browserImportProfiles, setBrowserImportProfiles] =
    useState<BrowserImportProfileView[] | null>(null);
  const [browserImportProfilesError, setBrowserImportProfilesError] =
    useState<string | null>(null);
  const [browserImportDestinationProfiles, setBrowserImportDestinationProfiles] =
    useState<BrowserImportDestinationProfileView[] | null>(null);
  const [
    browserImportDestinationProfilesError,
    setBrowserImportDestinationProfilesError,
  ] = useState<string | null>(null);
  const [browserImportStartStatus, setBrowserImportStartStatus] =
    useState<string | null>(null);
  const [globalHotkeyStatus, setGlobalHotkeyStatus] =
    useState<GlobalHotkeyStatusView | null>(null);
  const [globalHotkeyStatusError, setGlobalHotkeyStatusError] =
    useState<string | null>(null);
  const [mobilePairingStatus, setMobilePairingStatus] =
    useState<MobilePairingStatusView | null>(null);
  const [mobilePairingStatusError, setMobilePairingStatusError] =
    useState<string | null>(null);
  const [updaterStatus, setUpdaterStatus] =
    useState<UpdaterStatusView | null>(null);
  const [updaterStatusError, setUpdaterStatusError] =
    useState<string | null>(null);
  const [cliInstallStatus, setCliInstallStatus] =
    useState<CliInstallStatusView | null>(null);
  const [cliInstallStatusError, setCliInstallStatusError] =
    useState<string | null>(null);
  const [configExtensionStatus, setConfigExtensionStatus] =
    useState<ConfigExtensionStatusView | null>(null);
  const [configExtensionStatusError, setConfigExtensionStatusError] =
    useState<string | null>(null);
  const [desktopCoreStatus, setDesktopCoreStatus] =
    useState<DesktopCoreStatusView | null>(null);
  const [desktopCoreStatusError, setDesktopCoreStatusError] =
    useState<string | null>(null);
  const [defaultTerminalStatus, setDefaultTerminalStatus] =
    useState<DefaultTerminalStatusView | null>(null);
  const [defaultTerminalStatusError, setDefaultTerminalStatusError] =
    useState<string | null>(null);
  const [controlSocketStatus, setControlSocketStatus] =
    useState<ControlSocketStatusView | null>(null);
  const [controlSocketStatusError, setControlSocketStatusError] =
    useState<string | null>(null);
  const [vscodeInlineAvailable, setVSCodeInlineAvailable] =
    useState<boolean | null>(null);
  const [vscodeInlineStatusError, setVSCodeInlineStatusError] =
    useState<string | null>(null);
  const settingsLoadSeq = useRef(0);
  const settingsSaveSeq = useRef(0);
  const agentProviderStatusSeq = useRef(0);
  const browserImportProfilesSeq = useRef(0);
  const browserImportDestinationProfilesSeq = useRef(0);
  const globalHotkeyStatusSeq = useRef(0);
  const mobilePairingStatusSeq = useRef(0);
  const updaterStatusSeq = useRef(0);
  const cliStatusSeq = useRef(0);
  const configExtensionSeq = useRef(0);
  const desktopCoreStatusSeq = useRef(0);
  const defaultTerminalSeq = useRef(0);
  const controlSocketSeq = useRef(0);
  const vscodeInlineSeq = useRef(0);

  const applyAppearanceFromConfig = useCallback(
    (config: Config) => {
      setStoredAppearance(config.app?.appearance ?? "system");
    },
    [setStoredAppearance],
  );

  useEffect(() => {
    emitAgentSessionTheme(applied.documentColorScheme);
  }, [applied.documentColorScheme]);

  const loadSettings = useCallback(async () => {
    const seq = ++settingsLoadSeq.current;
    setSettingsLoading(true);
    try {
      const config = await host.invoke<Config>("config_load");
      if (seq !== settingsLoadSeq.current) {
        return;
      }
      setSettingsConfig(config);
      applyAppearanceFromConfig(config);
      setSettingsError(null);
    } catch (error) {
      if (seq !== settingsLoadSeq.current) {
        return;
      }
      const fallback = defaultSettingsConfig();
      const message = error instanceof Error ? error.message : String(error);
      setSettingsConfig(fallback);
      applyAppearanceFromConfig(fallback);
      setSettingsError(
        message.includes("Tauri bridge is unavailable")
          ? null
          : message,
      );
    } finally {
      if (seq === settingsLoadSeq.current) {
        setSettingsLoading(false);
      }
    }
  }, [applyAppearanceFromConfig]);

  const loadControlSocketStatus = useCallback(async () => {
    const seq = ++controlSocketSeq.current;
    try {
      const status =
        await host.invoke<ControlSocketStatusView>("control_socket_status");
      if (seq !== controlSocketSeq.current) {
        return;
      }
      setControlSocketStatus(status);
      setControlSocketStatusError(null);
    } catch (error) {
      if (seq !== controlSocketSeq.current) {
        return;
      }
      setControlSocketStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load control socket status.",
      );
    }
  }, []);

  const loadAgentProviderStatus = useCallback(async () => {
    const seq = ++agentProviderStatusSeq.current;
    try {
      const status =
        await host.invoke<AgentProviderStatusView[]>("agent_provider_status");
      if (seq !== agentProviderStatusSeq.current) {
        return;
      }
      setAgentProviderStatus(status);
      setAgentProviderStatusError(null);
    } catch (error) {
      if (seq !== agentProviderStatusSeq.current) {
        return;
      }
      setAgentProviderStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load agent provider status.",
      );
    }
  }, []);

  const loadBrowserImportProfiles = useCallback(async () => {
    const seq = ++browserImportProfilesSeq.current;
    try {
      const profiles =
        await host.invoke<BrowserImportProfileView[]>("browser_import_profiles");
      if (seq !== browserImportProfilesSeq.current) {
        return;
      }
      setBrowserImportProfiles(profiles);
      setBrowserImportProfilesError(null);
    } catch (error) {
      if (seq !== browserImportProfilesSeq.current) {
        return;
      }
      setBrowserImportProfilesError(
        error instanceof Error
          ? error.message
          : "Failed to load browser import profiles.",
      );
    }
  }, []);

  const loadBrowserImportDestinationProfiles = useCallback(async () => {
    const seq = ++browserImportDestinationProfilesSeq.current;
    try {
      const profiles = await host.invoke<BrowserImportDestinationProfileView[]>(
        "browser_import_destination_profiles",
      );
      if (seq !== browserImportDestinationProfilesSeq.current) {
        return;
      }
      setBrowserImportDestinationProfiles(profiles);
      setBrowserImportDestinationProfilesError(null);
    } catch (error) {
      if (seq !== browserImportDestinationProfilesSeq.current) {
        return;
      }
      setBrowserImportDestinationProfilesError(
        error instanceof Error
          ? error.message
          : "Failed to load browser import destination profiles.",
      );
    }
  }, []);

  const startBrowserImport = useCallback(async (request: BrowserImportStartRequest) => {
    setBrowserImportStartStatus("Starting browser import...");
    try {
      const reply = await host.invoke<{ message: string }>("browser_import_start", {
        request,
      });
      setBrowserImportStartStatus(reply.message);
    } catch (error) {
      setBrowserImportStartStatus(
        error instanceof Error ? error.message : "Failed to start browser import.",
      );
    }
  }, []);

  const loadUpdaterStatus = useCallback(async () => {
    const seq = ++updaterStatusSeq.current;
    try {
      const status = await host.invoke<UpdaterStatusView>("updater_status");
      if (seq !== updaterStatusSeq.current) {
        return;
      }
      setUpdaterStatus(status);
      setUpdaterStatusError(null);
    } catch (error) {
      if (seq !== updaterStatusSeq.current) {
        return;
      }
      setUpdaterStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load updater status.",
      );
    }
  }, []);

  const loadGlobalHotkeyStatus = useCallback(async () => {
    const seq = ++globalHotkeyStatusSeq.current;
    try {
      const status =
        await host.invoke<GlobalHotkeyStatusView>("global_hotkey_status");
      if (seq !== globalHotkeyStatusSeq.current) {
        return;
      }
      setGlobalHotkeyStatus(status);
      setGlobalHotkeyStatusError(null);
    } catch (error) {
      if (seq !== globalHotkeyStatusSeq.current) {
        return;
      }
      setGlobalHotkeyStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load global hotkey status.",
      );
    }
  }, []);

  const loadMobilePairingStatus = useCallback(async () => {
    const seq = ++mobilePairingStatusSeq.current;
    try {
      const status =
        await host.invoke<MobilePairingStatusView>("mobile_pairing_status");
      if (seq !== mobilePairingStatusSeq.current) {
        return;
      }
      setMobilePairingStatus(status);
      setMobilePairingStatusError(null);
    } catch (error) {
      if (seq !== mobilePairingStatusSeq.current) {
        return;
      }
      setMobilePairingStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load mobile pairing status.",
      );
    }
  }, []);

  const loadCliInstallStatus = useCallback(async () => {
    const seq = ++cliStatusSeq.current;
    try {
      const status = await host.invoke<CliInstallStatusView>("cli_install_status");
      if (seq !== cliStatusSeq.current) {
        return;
      }
      setCliInstallStatus(status);
      setCliInstallStatusError(null);
    } catch (error) {
      if (seq !== cliStatusSeq.current) {
        return;
      }
      setCliInstallStatusError(
        error instanceof Error ? error.message : "Failed to load CLI status.",
      );
    }
  }, []);

  const loadConfigExtensionStatus = useCallback(async () => {
    const seq = ++configExtensionSeq.current;
    try {
      const status =
        await host.invoke<ConfigExtensionStatusView>("config_extension_status");
      if (seq !== configExtensionSeq.current) {
        return;
      }
      setConfigExtensionStatus(status);
      setConfigExtensionStatusError(null);
    } catch (error) {
      if (seq !== configExtensionSeq.current) {
        return;
      }
      setConfigExtensionStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load config extension status.",
      );
    }
  }, []);

  const loadDesktopCoreStatus = useCallback(async () => {
    const seq = ++desktopCoreStatusSeq.current;
    try {
      const status =
        await host.invoke<DesktopCoreStatusView>("desktop_core_status");
      if (seq !== desktopCoreStatusSeq.current) {
        return;
      }
      setDesktopCoreStatus(status);
      setDesktopCoreStatusError(null);
    } catch (error) {
      if (seq !== desktopCoreStatusSeq.current) {
        return;
      }
      setDesktopCoreStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load desktop core status.",
      );
    }
  }, []);

  const loadDefaultTerminalStatus = useCallback(async () => {
    const seq = ++defaultTerminalSeq.current;
    try {
      const status =
        await host.invoke<DefaultTerminalStatusView>("default_terminal_status");
      if (seq !== defaultTerminalSeq.current) {
        return;
      }
      setDefaultTerminalStatus(status);
      setDefaultTerminalStatusError(null);
    } catch (error) {
      if (seq !== defaultTerminalSeq.current) {
        return;
      }
      setDefaultTerminalStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load default terminal status.",
      );
    }
  }, []);

  const loadVSCodeInlineStatus = useCallback(async () => {
    const seq = ++vscodeInlineSeq.current;
    try {
      const available = await host.invoke<boolean>(
        "vscode_inline_open_target_available",
      );
      if (seq !== vscodeInlineSeq.current) {
        return;
      }
      setVSCodeInlineAvailable(available);
      setVSCodeInlineStatusError(null);
    } catch (error) {
      if (seq !== vscodeInlineSeq.current) {
        return;
      }
      setVSCodeInlineStatusError(
        error instanceof Error
          ? error.message
          : "Failed to load VS Code command status.",
      );
    }
  }, []);

  useEffect(() => {
    void loadSettings();
    void loadAgentProviderStatus();
    void loadBrowserImportProfiles();
    void loadBrowserImportDestinationProfiles();
    void loadGlobalHotkeyStatus();
    void loadMobilePairingStatus();
    void loadUpdaterStatus();
    void loadCliInstallStatus();
    void loadConfigExtensionStatus();
    void loadDesktopCoreStatus();
    void loadDefaultTerminalStatus();
    void loadControlSocketStatus();
    void loadVSCodeInlineStatus();
  }, [
    loadAgentProviderStatus,
    loadBrowserImportDestinationProfiles,
    loadBrowserImportProfiles,
    loadGlobalHotkeyStatus,
    loadMobilePairingStatus,
    loadUpdaterStatus,
    loadCliInstallStatus,
    loadConfigExtensionStatus,
    loadControlSocketStatus,
    loadDesktopCoreStatus,
    loadDefaultTerminalStatus,
    loadVSCodeInlineStatus,
    loadSettings,
  ]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void host
      .on<ConfigChangedPayload>("cmux://config-changed", (payload) => {
        if (disposed) {
          return;
        }
        settingsLoadSeq.current += 1;
        setSettingsConfig(payload.config);
        applyAppearanceFromConfig(payload.config);
        setSettingsError(null);
        setSettingsLoading(false);
        void loadConfigExtensionStatus();
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      })
      .catch((error) => {
        if (!disposed) {
          console.warn("config change subscription failed", error);
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyAppearanceFromConfig, loadConfigExtensionStatus]);

  const openSettings = useCallback((options?: {
    section?: SettingsPaneSection;
    query?: string;
  }) => {
    void host
      .invoke("settings_open_capture", {
        target: options?.section ?? null,
      })
      .catch(() => undefined);
    setSettingsSearchQuery(options?.query ?? "");
    setSettingsTargetSection(options?.section ?? null);
    setSettingsNavigationKey((key) => key + 1);
    setSettingsOpen(true);
  }, []);

  const openShortcutHelp = useCallback(() => {
    openSettings({ section: "shortcuts" });
  }, [openSettings]);

  const closeSettings = useCallback(() => {
    setSettingsOpen(false);
    setSettingsSearchQuery("");
    setSettingsTargetSection(null);
    setNotificationCommandTestStatus(null);
    setNotificationDeliveryPreviewStatus(null);
    setNotificationToastSendStatus(null);
  }, []);

  const toggleFileExplorer = useCallback(() => {
    setFileExplorerOpen((value) => {
      if (!value) {
        setRightSidebarMode("files");
      }
      return !value;
    });
  }, []);

  const openRightSidebarMode = useCallback((mode: RightSidebarMode) => {
    setRightSidebarMode(mode);
    setFileExplorerOpen(true);
    if (mode === "find") {
      setDirectorySearchOpen(true);
    }
  }, []);

  const openFindInDirectory = useCallback(() => {
    openRightSidebarMode("find");
  }, [openRightSidebarMode]);

  useEffect(() => {
    const next = { visible: fileExplorerOpen, mode: rightSidebarMode };
    rightSidebarStateRef.current = next;
    void host
      .invoke("right_sidebar_update_state", next)
      .catch((error) => console.warn("right sidebar state sync failed", error));
  }, [fileExplorerOpen, rightSidebarMode]);

  useEffect(() => {
    void host
      .invoke<RightSidebarBetaSettingsView>("right_sidebar_beta_settings")
      .then(setRightSidebarBetaSettings)
      .catch((error) => console.warn("right sidebar beta settings load failed", error));
  }, []);

  const setRightSidebarBetaFeature = useCallback(
    (feature: "feed" | "dock", enabled: boolean) => {
      void host
        .invoke<RightSidebarBetaSettingsView>("right_sidebar_set_beta_feature", {
          feature,
          enabled,
        })
        .then(setRightSidebarBetaSettings)
        .catch((error) => console.warn("right sidebar beta setting failed", error));
    },
    [],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void host
      .on<RightSidebarRemotePayload>("cmux://right-sidebar-changed", (payload) => {
        if (disposed) {
          return;
        }
        const next = rightSidebarStateFromRemote(
          rightSidebarStateRef.current,
          payload,
        );
        rightSidebarStateRef.current = next;
        setFileExplorerOpen(next.visible);
        setRightSidebarMode(next.mode);
        if (next.visible && next.mode === "find") {
          setDirectorySearchOpen(true);
        }
        if (payload.focus) {
          requestAnimationFrame(() => {
            document
              .querySelector<HTMLElement>(".cmux-file-explorer")
              ?.focus({ preventScroll: true });
          });
        }
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      })
      .catch((error) => {
        if (!disposed) {
          console.warn("right sidebar subscription failed", error);
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const handleSettingsAction = useCallback(
    (action: ConfigAction, nextConfig: Config) => {
      setSettingsConfig(nextConfig);
      if (action.type === "setAppearance") {
        applyAppearanceFromConfig(nextConfig);
      }

      const mutation = configMutationFromAction(nextConfig, action);
      if (mutation == null) {
        return;
      }

      const seq = ++settingsSaveSeq.current;
      setSettingsSaving(true);
      setSettingsError(null);
      void host
        .invoke<Config>("config_save", {
          path: mutation.path,
          value: mutation.value,
          remove: mutation.remove ?? false,
        })
        .then((saved) => {
          if (seq !== settingsSaveSeq.current) {
            return;
          }
          setSettingsConfig(saved);
          applyAppearanceFromConfig(saved);
          void loadConfigExtensionStatus();
        })
        .catch((error) => {
          if (seq !== settingsSaveSeq.current) {
            return;
          }
          setSettingsError(
            error instanceof Error
              ? error.message
              : "Failed to save cmux.json.",
          );
          setSettingsSaving(false);
          void loadSettings();
        })
        .finally(() => {
          if (seq === settingsSaveSeq.current) {
            setSettingsSaving(false);
          }
        });
    },
    [applyAppearanceFromConfig, loadConfigExtensionStatus, loadSettings],
  );

  const applyPaletteConfigAction = useCallback(
    (action: ConfigAction) => {
      const base = settingsConfig ?? defaultSettingsConfig();
      const next = configReducer(base, action);
      handleSettingsAction(action, next);
    },
    [handleSettingsAction, settingsConfig],
  );

  const openBrowserImportSettings = useCallback(() => {
    openSettings({ section: "browserImport" });
  }, [openSettings]);

  const dismissBrowserImportHint = useCallback(() => {
    if (settingsConfig?.browser?.showImportHintOnBlankTabs === false) {
      return;
    }
    applyPaletteConfigAction({
      type: "toggleBrowserFlag",
      key: "showImportHintOnBlankTabs",
    });
  }, [applyPaletteConfigAction, settingsConfig?.browser?.showImportHintOnBlankTabs]);

  const openSettingsFile = useCallback(() => {
    setSettingsError(null);
    void host.invoke("open_cmux_settings_file").catch((error) => {
      setSettingsError(
        error instanceof Error ? error.message : "Failed to open cmux.json.",
      );
    });
  }, []);

  const loadRawSettings = useCallback(() => {
    setRawSettingsLoading(true);
    setRawSettingsError(null);
    setRawSettingsStatus(null);
    void host
      .invoke<RawConfigFile>("config_read_raw")
      .then((file) => {
        setRawSettingsPath(file.path);
        setRawSettingsDraft(file.contents);
        setRawSettingsStatus(`Loaded ${file.path}`);
      })
      .catch((error) => {
        setRawSettingsError(
          error instanceof Error ? error.message : "Failed to load cmux.json.",
        );
      })
      .finally(() => setRawSettingsLoading(false));
  }, []);

  const saveRawSettings = useCallback(() => {
    setRawSettingsSaving(true);
    setRawSettingsError(null);
    setRawSettingsStatus(null);
    void host
      .invoke<Config>("config_write_raw", { contents: rawSettingsDraft })
      .then((saved) => {
        setSettingsConfig(saved);
        applyAppearanceFromConfig(saved);
        setRawSettingsStatus("Saved cmux.json");
        void loadConfigExtensionStatus();
      })
      .catch((error) => {
        setRawSettingsError(
          error instanceof Error ? error.message : "Failed to save cmux.json.",
        );
      })
      .finally(() => setRawSettingsSaving(false));
  }, [applyAppearanceFromConfig, loadConfigExtensionStatus, rawSettingsDraft]);

  const openGhosttySettingsFile = useCallback(() => {
    setSettingsError(null);
    void host.invoke("open_ghostty_settings_file").catch((error) => {
      setSettingsError(
        error instanceof Error
          ? error.message
          : "Failed to open Ghostty settings.",
      );
    });
  }, []);

  const openTaskManager = useCallback(() => {
    setSettingsError(null);
    void host.invoke("window_open_task_manager").catch((error) => {
      setSettingsError(
        error instanceof Error ? error.message : "Failed to open Task Manager.",
      );
    });
  }, []);

  const testNotificationCommand = useCallback(() => {
    const command = settingsConfig?.notifications?.command ?? "";
    setSettingsError(null);
    setNotificationCommandTestStatus("Launching notification command...");
    void host
      .invoke<NotificationCommandRunReply>("notification_run_custom_command", {
        command,
        title: "cmux test notification",
        subtitle: "Settings > Notifications",
        body: "Your custom notification command received cmux's environment variables.",
      })
      .then((reply) => {
        if (!reply.ran) {
          setNotificationCommandTestStatus(
            reply.skipped_reason ?? "No notification command is configured.",
          );
          return;
        }
        setNotificationCommandTestStatus(
          reply.root_pid != null
            ? `Notification command launched (PID ${reply.root_pid}).`
            : "Notification command launched.",
        );
      })
      .catch((error) => {
        setNotificationCommandTestStatus(null);
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to run notification command.",
        );
      });
  }, [settingsConfig?.notifications?.command]);

  const currentNotificationActivationTarget = useCallback(async () => {
    try {
      return notificationActivationTargetFromSnapshot(
        await host.invoke<AppSessionSnapshot>("session_snapshot"),
      );
    } catch {
      return {};
    }
  }, []);

  const openSettingsFileInCmux = useCallback(() => {
    setSettingsError(null);
    void Promise.all([
      host.invoke<string>("config_settings_file_path"),
      currentNotificationActivationTarget(),
    ])
      .then(([filePath, target]) => {
        const panelId = target.panelId ?? target.surfaceId;
        if (panelId == null || panelId === "") {
          throw new Error("No focused pane is available for cmux.json.");
        }
        return host.invoke("session_open_file", { panelId, filePath });
      })
      .then(() => {
        closeSettings();
      })
      .catch((error) => {
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to open cmux.json in cmux.",
        );
      });
  }, [closeSettings, currentNotificationActivationTarget]);

  const previewNotificationDelivery = useCallback(() => {
    const notificationDelivery = notificationDeliverySettingsRequest(
      settingsConfig?.notifications,
    );
    setSettingsError(null);
    setNotificationDeliveryPreviewStatus("Building Windows toast delivery plan...");
    void currentNotificationActivationTarget()
      .then((target) =>
        host.invoke<NotificationDeliveryPreviewReply>(
          "notification_preview_delivery_plan",
          {
            title: "cmux test notification",
            subtitle: "Settings > Notifications",
            body: "Preview of the Windows toast payload cmux will deliver.",
            sound: notificationDelivery.sound,
            customSoundFilePath: notificationDelivery.customSoundFilePath,
            ...target,
            desktop: notificationDelivery.desktop,
            soundEffect: notificationDelivery.soundEffect,
            commandEffect: notificationDelivery.commandEffect,
            suppressed: notificationDelivery.suppressed,
          },
        ),
      )
      .then((reply) => {
        const route = reply.launch == null ? "" : ` Launch ${reply.launch}.`;
        if (reply.toast_xml == null) {
          setNotificationDeliveryPreviewStatus(
            reply.decision == null
              ? "No toast would be delivered for these effects."
              : `${reply.decision}: no OS toast; in-app sound ${
                  reply.play_in_app_sound ? "enabled" : "disabled"
                }.`,
          );
          return;
        }
        setNotificationDeliveryPreviewStatus(
          `${reply.decision ?? "Desktop"} toast ready: tag ${
            reply.tag ?? "none"
          }, group ${reply.group ?? "none"}, command ${
            reply.run_command ? "enabled" : "disabled"
          }.${route}`,
        );
      })
      .catch((error) => {
        setNotificationDeliveryPreviewStatus(null);
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to preview notification delivery.",
        );
      });
  }, [currentNotificationActivationTarget, settingsConfig?.notifications]);

  const sendTestNotificationToast = useCallback(() => {
    const notificationDelivery = notificationDeliverySettingsRequest(
      settingsConfig?.notifications,
    );
    setSettingsError(null);
    setNotificationToastSendStatus("Sending Windows test toast...");
    void currentNotificationActivationTarget()
      .then((target) =>
        host.invoke<NotificationToastSendReply>("notification_send_test_toast", {
          sound: notificationDelivery.sound,
          customSoundFilePath: notificationDelivery.customSoundFilePath,
          ...target,
        }),
      )
      .then((reply) => {
        setNotificationToastSendStatus(
          reply.sent
            ? `${reply.message} Tag ${reply.tag ?? "none"}, group ${
                reply.group ?? "none"
              }. Click it to return to the current pane.`
            : reply.message,
        );
      })
      .catch((error) => {
        setNotificationToastSendStatus(null);
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to send Windows test toast.",
        );
      });
  }, [currentNotificationActivationTarget, settingsConfig?.notifications]);

  const restorePreviousLaunch = useCallback(() => {
    setSettingsError(null);
    void host.invoke("session_restore_previous_launch").catch((error) => {
      setSettingsError(
        error instanceof Error
          ? error.message
          : "Failed to restore previous launch.",
      );
    });
  }, []);

  const openBrowserWorkspaceUrl = useCallback((url: string) => {
    void host.invoke("session_new_browser_workspace", { url }).catch((error) => {
      setSettingsError(
        error instanceof Error
          ? error.message
          : "Failed to open browser workspace.",
      );
    });
  }, []);

  const openFolderInVSCodeInline = useCallback(() => {
    setSettingsError(null);
    void host
      .invoke<string | null>("open_folder_in_vscode_inline")
      .then((url) => {
        if (url) {
          openBrowserWorkspaceUrl(url);
        }
      })
      .catch((error) => {
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to open folder in VS Code.",
        );
      });
  }, [openBrowserWorkspaceUrl]);

  const restartVSCodeServeWeb = useCallback(() => {
    setSettingsError(null);
    void host
      .invoke<string | null>("vscode_serve_web_restart")
      .then((url) => {
        if (url) {
          openBrowserWorkspaceUrl(url);
        }
      })
      .catch((error) => {
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to restart VS Code web server.",
        );
      });
  }, [openBrowserWorkspaceUrl]);

  const stopVSCodeServeWeb = useCallback(() => {
    setSettingsError(null);
    void host.invoke("vscode_serve_web_stop").catch((error) => {
      setSettingsError(
        error instanceof Error
          ? error.message
          : "Failed to stop VS Code web server.",
      );
    });
  }, []);

  const resetSettingsConfig = useCallback(() => {
    if (
      typeof window !== "undefined" &&
      !window.confirm("Reset cmux.json and restore default settings?")
    ) {
      return;
    }

    const seq = ++settingsSaveSeq.current;
    setSettingsSaving(true);
    setSettingsError(null);
    void host
      .invoke<Config>("config_reset")
      .then((config) => {
        if (seq !== settingsSaveSeq.current) {
          return;
        }
        setSettingsConfig(config);
        applyAppearanceFromConfig(config);
        void loadConfigExtensionStatus();
      })
      .catch((error) => {
        if (seq !== settingsSaveSeq.current) {
          return;
        }
        setSettingsError(
          error instanceof Error
            ? error.message
            : "Failed to reset cmux.json.",
        );
      })
      .finally(() => {
        if (seq === settingsSaveSeq.current) {
          setSettingsSaving(false);
        }
      });
  }, [applyAppearanceFromConfig, loadConfigExtensionStatus]);

  const installCli = useCallback(() => {
    const seq = ++cliStatusSeq.current;
    setCliInstallStatusError(null);
    void host
      .invoke<CliInstallStatusView>("install_cli")
      .then((status) => {
        if (seq !== cliStatusSeq.current) {
          return;
        }
        setCliInstallStatus(status);
      })
      .catch((error) => {
        if (seq !== cliStatusSeq.current) {
          return;
        }
        setCliInstallStatusError(
          error instanceof Error ? error.message : "Failed to install CLI.",
        );
      });
  }, []);

  const uninstallCli = useCallback(() => {
    const seq = ++cliStatusSeq.current;
    setCliInstallStatusError(null);
    void host
      .invoke<CliInstallStatusView>("uninstall_cli")
      .then((status) => {
        if (seq !== cliStatusSeq.current) {
          return;
        }
        setCliInstallStatus(status);
      })
      .catch((error) => {
        if (seq !== cliStatusSeq.current) {
          return;
        }
        setCliInstallStatusError(
          error instanceof Error ? error.message : "Failed to uninstall CLI.",
        );
      });
  }, []);

  const makeDefaultTerminal = useCallback(() => {
    const seq = ++defaultTerminalSeq.current;
    setDefaultTerminalStatusError(null);
    void host
      .invoke<DefaultTerminalStatusView>("make_default_terminal")
      .then((status) => {
        if (seq !== defaultTerminalSeq.current) {
          return;
        }
        setDefaultTerminalStatus(status);
      })
      .catch((error) => {
        if (seq !== defaultTerminalSeq.current) {
          return;
        }
        setDefaultTerminalStatusError(
          error instanceof Error
            ? error.message
            : "Failed to make cmux the default terminal.",
        );
      });
  }, []);

  const restartControlSocket = useCallback(() => {
    const seq = ++controlSocketSeq.current;
    setControlSocketStatusError(null);
    void host
      .invoke<ControlSocketStatusView>("restart_control_socket_listener")
      .then((status) => {
        if (seq !== controlSocketSeq.current) {
          return;
        }
        setControlSocketStatus(status);
      })
      .catch((error) => {
        if (seq !== controlSocketSeq.current) {
          return;
        }
        setControlSocketStatusError(
          error instanceof Error
            ? error.message
            : "Failed to restart control socket listener.",
        );
      });
  }, []);

  const minimalModeEnabled = settingsConfig?.app?.minimalMode ?? false;
  const shellStyle = minimalModeTabStripCssVariables(
    minimalModeEnabled,
  ) as CSSProperties;
  const sidebarSettings = settingsConfig?.sidebar;
  const sidebarBadgeVisibilitySettings = {
    hideAllDetails:
      sidebarSettings?.hideAllDetails ?? DEFAULT_SIDEBAR_CONFIG.hideAllDetails,
    showBranchDirectory:
      sidebarSettings?.showBranchDirectory ??
      DEFAULT_SIDEBAR_CONFIG.showBranchDirectory,
    showGitBranch: true,
    showPullRequests:
      sidebarSettings?.showPullRequests ?? DEFAULT_SIDEBAR_CONFIG.showPullRequests,
    showSsh: sidebarSettings?.showSSH ?? DEFAULT_SIDEBAR_CONFIG.showSSH,
    showPorts: sidebarSettings?.showPorts ?? DEFAULT_SIDEBAR_CONFIG.showPorts,
  };

  return (
    <div
      className={minimalModeEnabled ? "cmux-shell cmux-shell--minimal" : "cmux-shell"}
      style={shellStyle}
    >
      <WindowTitlebar
        minimalMode={minimalModeEnabled}
        sidebarCollapsed={sidebarCollapsed}
        onToggleSidebar={() => setSidebarCollapsed((v) => !v)}
        fileExplorerOpen={fileExplorerOpen}
        onToggleFileExplorer={toggleFileExplorer}
        onOpenShortcutHelp={openShortcutHelp}
        onOpenSettings={openSettings}
      />
      <div className="cmux-body">
        <Sidebar
          collapsed={sidebarCollapsed}
          showWorkspaceDescription={
            sidebarSettings?.showWorkspaceDescription ??
            DEFAULT_SIDEBAR_CONFIG.showWorkspaceDescription
          }
          wrapWorkspaceTitles={
            sidebarSettings?.wrapWorkspaceTitles ??
            DEFAULT_SIDEBAR_CONFIG.wrapWorkspaceTitles
          }
          branchLayout={
            sidebarSettings?.branchLayout ?? DEFAULT_SIDEBAR_CONFIG.branchLayout
          }
          badgeVisibilitySettings={sidebarBadgeVisibilitySettings}
          showProgress={
            sidebarSettings?.showProgress ?? DEFAULT_SIDEBAR_CONFIG.showProgress
          }
          showLog={sidebarSettings?.showLog ?? DEFAULT_SIDEBAR_CONFIG.showLog}
          showCustomMetadata={
            sidebarSettings?.showCustomMetadata ??
            DEFAULT_SIDEBAR_CONFIG.showCustomMetadata
          }
          makePullRequestsClickable={
            sidebarSettings?.makePullRequestsClickable ??
            DEFAULT_SIDEBAR_CONFIG.makePullRequestsClickable
          }
          openPullRequestLinksInCmuxBrowser={
            sidebarSettings?.openPullRequestLinksInCmuxBrowser ??
            DEFAULT_SIDEBAR_CONFIG.openPullRequestLinksInCmuxBrowser
          }
          openPortLinksInCmuxBrowser={
            sidebarSettings?.openPortLinksInCmuxBrowser ??
            DEFAULT_SIDEBAR_CONFIG.openPortLinksInCmuxBrowser
          }
        />
        <main className="cmux-main">
          <Workspace
            canvasConfig={settingsConfig?.canvas}
            fileEditorWordWrap={settingsConfig?.file_editor?.wordWrap}
            markdownConfig={settingsConfig?.markdown}
            openTerminalLinksInCmuxBrowser={
              settingsConfig?.browser?.openTerminalLinksInCmuxBrowser ?? true
            }
            showBrowserImportHintOnBlankTabs={
              settingsConfig?.browser?.showImportHintOnBlankTabs ?? true
            }
            onOpenBrowserImportSettings={openBrowserImportSettings}
            onDismissBrowserImportHint={dismissBrowserImportHint}
          />
        </main>
        <FileExplorerPanel
          open={fileExplorerOpen}
          mode={rightSidebarMode}
          doubleClickAction={settingsConfig?.file_explorer?.doubleClickAction}
          preferredEditor={settingsConfig?.app?.preferredEditor}
          rightMaxWidth={sidebarSettings?.right_max_width}
          feedEnabled={rightSidebarBetaSettings?.feed_enabled ?? false}
          onModeChange={setRightSidebarMode}
          onOpenFind={openFindInDirectory}
          onClose={() => setFileExplorerOpen(false)}
        />
      </div>
      <CommandPaletteOverlay
        hostActions={{
          toggleSidebar: () => setSidebarCollapsed((v) => !v),
          toggleFileExplorer,
          setRightSidebarMode: openRightSidebarMode,
          rightSidebarModeAvailability: {
            feedEnabled: rightSidebarBetaSettings?.feed_enabled ?? false,
            dockEnabled: false,
          },
          openSettings,
          openNotifications: () => setNotificationsOpen(true),
          openFindInDirectory,
          newWindow: windowChrome.newWindow,
          closeWindow: windowChrome.close,
          toggleFullScreen: windowChrome.toggleFullscreen,
          settingsConfig,
          applyConfigAction: applyPaletteConfigAction,
        }}
      />
      <NotificationsOverlay
        open={notificationsOpen}
        onClose={() => setNotificationsOpen(false)}
      />
      <DirectorySearchOverlay
        open={directorySearchOpen}
        onClose={() => setDirectorySearchOpen(false)}
      />
      <SettingsOverlay
        open={settingsOpen}
        config={settingsConfig}
        loading={settingsLoading}
        saving={settingsSaving}
        error={settingsError}
        searchQuery={settingsSearchQuery}
        targetSection={settingsTargetSection}
        navigationKey={settingsNavigationKey}
        onSearchQueryChange={setSettingsSearchQuery}
        onChange={setSettingsConfig}
        onAction={handleSettingsAction}
        agentProviderStatus={agentProviderStatus}
        agentProviderStatusError={agentProviderStatusError}
        browserImportDestinationProfiles={browserImportDestinationProfiles}
        browserImportDestinationProfilesError={browserImportDestinationProfilesError}
        browserImportProfiles={browserImportProfiles}
        browserImportProfilesError={browserImportProfilesError}
        browserImportStartStatus={browserImportStartStatus}
        globalHotkeyStatus={globalHotkeyStatus}
        globalHotkeyStatusError={globalHotkeyStatusError}
        mobilePairingStatus={mobilePairingStatus}
        mobilePairingStatusError={mobilePairingStatusError}
        updaterStatus={updaterStatus}
        updaterStatusError={updaterStatusError}
        cliInstallStatus={cliInstallStatus}
        cliInstallStatusError={cliInstallStatusError}
        configExtensionStatus={configExtensionStatus}
        configExtensionStatusError={configExtensionStatusError}
        desktopCoreStatus={desktopCoreStatus}
        desktopCoreStatusError={desktopCoreStatusError}
        defaultTerminalStatus={defaultTerminalStatus}
        defaultTerminalStatusError={defaultTerminalStatusError}
        controlSocketStatus={controlSocketStatus}
        controlSocketStatusError={controlSocketStatusError}
        vscodeInlineAvailable={vscodeInlineAvailable}
        vscodeInlineStatusError={vscodeInlineStatusError}
        rightSidebarBetaSettings={rightSidebarBetaSettings}
        onSetRightSidebarBetaFeature={setRightSidebarBetaFeature}
        onOpenFolderInVSCodeInline={openFolderInVSCodeInline}
        onRestartVSCodeServeWeb={restartVSCodeServeWeb}
        onStopVSCodeServeWeb={stopVSCodeServeWeb}
        onRefreshAgentProviderStatus={() => void loadAgentProviderStatus()}
        onRefreshBrowserImportProfiles={() => {
          void loadBrowserImportProfiles();
          void loadBrowserImportDestinationProfiles();
        }}
        onStartBrowserImport={(request) => void startBrowserImport(request)}
        onRefreshGlobalHotkeyStatus={() => void loadGlobalHotkeyStatus()}
        onRefreshMobilePairingStatus={() => void loadMobilePairingStatus()}
        onRefreshUpdaterStatus={() => void loadUpdaterStatus()}
        onInstallCli={installCli}
        onUninstallCli={uninstallCli}
        onRefreshCliInstallStatus={() => void loadCliInstallStatus()}
        onRefreshConfigExtensionStatus={() => void loadConfigExtensionStatus()}
        onRefreshDesktopCoreStatus={() => void loadDesktopCoreStatus()}
        onMakeDefaultTerminal={makeDefaultTerminal}
        onRefreshDefaultTerminalStatus={() => void loadDefaultTerminalStatus()}
        onRefreshVSCodeInlineStatus={() => void loadVSCodeInlineStatus()}
        onRefreshControlSocketStatus={() => void loadControlSocketStatus()}
        onRestartControlSocket={restartControlSocket}
        onOpenTaskManager={openTaskManager}
        onRestorePreviousLaunch={restorePreviousLaunch}
        notificationCommandTestStatus={notificationCommandTestStatus}
        onTestNotificationCommand={testNotificationCommand}
        notificationDeliveryPreviewStatus={notificationDeliveryPreviewStatus}
        onPreviewNotificationDelivery={previewNotificationDelivery}
        notificationToastSendStatus={notificationToastSendStatus}
        onSendTestNotificationToast={sendTestNotificationToast}
        onOpenSettingsFile={openSettingsFile}
        onOpenSettingsFileInCmux={openSettingsFileInCmux}
        onOpenGhosttySettingsFile={openGhosttySettingsFile}
        rawSettingsPath={rawSettingsPath}
        rawSettingsDraft={rawSettingsDraft}
        rawSettingsLoading={rawSettingsLoading}
        rawSettingsSaving={rawSettingsSaving}
        rawSettingsError={rawSettingsError}
        rawSettingsStatus={rawSettingsStatus}
        onLoadRawSettings={loadRawSettings}
        onRawSettingsDraftChange={setRawSettingsDraft}
        onSaveRawSettings={saveRawSettings}
        onResetConfig={resetSettingsConfig}
        onReload={() => void loadSettings()}
        onClose={closeSettings}
      />
    </div>
  );
}
