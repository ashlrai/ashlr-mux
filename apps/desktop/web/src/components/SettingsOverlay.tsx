import { useEffect, useRef, useState } from "react";

import type { Config } from "@cmux/core-types";

import type { ConfigAction } from "../settings/configReducer";
import type { BrowserImportStartRequest } from "../settings/browserImportPlan";
import type { SettingsPaneSection } from "../settings/settingsSearchResults";
import {
  SettingsPane,
  type AgentProviderStatusView,
  type BrowserImportDestinationProfileView,
  type BrowserImportProfileView,
  type CliInstallStatusView,
  type ConfigExtensionStatusView,
  type ControlSocketStatusView,
  type DesktopCoreStatusView,
  type DefaultTerminalStatusView,
  type GlobalHotkeyStatusView,
  type MobilePairingStatusView,
  type UpdaterStatusView,
} from "./SettingsPane";

export interface SettingsOverlayProps {
  open: boolean;
  config: Config | null;
  targetSection?: SettingsPaneSection | null;
  navigationKey?: number;
  loading?: boolean;
  saving?: boolean;
  error?: string | null;
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
  onChange: (next: Config) => void;
  onAction?: (action: ConfigAction, next: Config) => void;
  agentProviderStatus?: AgentProviderStatusView[] | null;
  agentProviderStatusError?: string | null;
  browserImportDestinationProfiles?: BrowserImportDestinationProfileView[] | null;
  browserImportDestinationProfilesError?: string | null;
  browserImportProfiles?: BrowserImportProfileView[] | null;
  browserImportProfilesError?: string | null;
  browserImportStartStatus?: string | null;
  globalHotkeyStatus?: GlobalHotkeyStatusView | null;
  globalHotkeyStatusError?: string | null;
  mobilePairingStatus?: MobilePairingStatusView | null;
  mobilePairingStatusError?: string | null;
  updaterStatus?: UpdaterStatusView | null;
  updaterStatusError?: string | null;
  cliInstallStatus?: CliInstallStatusView | null;
  cliInstallStatusError?: string | null;
  configExtensionStatus?: ConfigExtensionStatusView | null;
  configExtensionStatusError?: string | null;
  desktopCoreStatus?: DesktopCoreStatusView | null;
  desktopCoreStatusError?: string | null;
  defaultTerminalStatus?: DefaultTerminalStatusView | null;
  defaultTerminalStatusError?: string | null;
  controlSocketStatus?: ControlSocketStatusView | null;
  controlSocketStatusError?: string | null;
  vscodeInlineAvailable?: boolean | null;
  vscodeInlineStatusError?: string | null;
  onRefreshAgentProviderStatus?: () => void;
  onRefreshBrowserImportProfiles?: () => void;
  onStartBrowserImport?: (request: BrowserImportStartRequest) => void;
  onRefreshGlobalHotkeyStatus?: () => void;
  onRefreshMobilePairingStatus?: () => void;
  onRefreshUpdaterStatus?: () => void;
  onInstallCli?: () => void;
  onUninstallCli?: () => void;
  onRefreshCliInstallStatus?: () => void;
  onRefreshConfigExtensionStatus?: () => void;
  onRefreshDesktopCoreStatus?: () => void;
  onMakeDefaultTerminal?: () => void;
  onRefreshDefaultTerminalStatus?: () => void;
  onRefreshVSCodeInlineStatus?: () => void;
  onOpenFolderInVSCodeInline?: () => void;
  onRestartVSCodeServeWeb?: () => void;
  onStopVSCodeServeWeb?: () => void;
  onRefreshControlSocketStatus?: () => void;
  onRestartControlSocket?: () => void;
  onOpenTaskManager?: () => void;
  onRestorePreviousLaunch?: () => void;
  notificationCommandTestStatus?: string | null;
  onTestNotificationCommand?: () => void;
  notificationDeliveryPreviewStatus?: string | null;
  onPreviewNotificationDelivery?: () => void;
  notificationToastSendStatus?: string | null;
  onSendTestNotificationToast?: () => void;
  onOpenSettingsFile?: () => void;
  onOpenSettingsFileInCmux?: () => void;
  onOpenGhosttySettingsFile?: () => void;
  rawSettingsPath?: string | null;
  rawSettingsDraft?: string;
  rawSettingsLoading?: boolean;
  rawSettingsSaving?: boolean;
  rawSettingsError?: string | null;
  rawSettingsStatus?: string | null;
  onLoadRawSettings?: () => void;
  onRawSettingsDraftChange?: (draft: string) => void;
  onSaveRawSettings?: () => void;
  onResetConfig?: () => void;
  onReload?: () => void;
  onClose: () => void;
}

export function SettingsOverlay({
  open,
  config,
  targetSection = null,
  navigationKey = 0,
  loading = false,
  saving = false,
  error,
  searchQuery,
  onSearchQueryChange,
  onChange,
  onAction,
  agentProviderStatus,
  agentProviderStatusError,
  browserImportDestinationProfiles,
  browserImportDestinationProfilesError,
  browserImportProfiles,
  browserImportProfilesError,
  browserImportStartStatus,
  globalHotkeyStatus,
  globalHotkeyStatusError,
  mobilePairingStatus,
  mobilePairingStatusError,
  updaterStatus,
  updaterStatusError,
  cliInstallStatus,
  cliInstallStatusError,
  configExtensionStatus,
  configExtensionStatusError,
  desktopCoreStatus,
  desktopCoreStatusError,
  defaultTerminalStatus,
  defaultTerminalStatusError,
  controlSocketStatus,
  controlSocketStatusError,
  vscodeInlineAvailable,
  vscodeInlineStatusError,
  onRefreshAgentProviderStatus,
  onRefreshBrowserImportProfiles,
  onStartBrowserImport,
  onRefreshGlobalHotkeyStatus,
  onRefreshMobilePairingStatus,
  onRefreshUpdaterStatus,
  onInstallCli,
  onUninstallCli,
  onRefreshCliInstallStatus,
  onRefreshConfigExtensionStatus,
  onRefreshDesktopCoreStatus,
  onMakeDefaultTerminal,
  onRefreshDefaultTerminalStatus,
  onRefreshVSCodeInlineStatus,
  onOpenFolderInVSCodeInline,
  onRestartVSCodeServeWeb,
  onStopVSCodeServeWeb,
  onRefreshControlSocketStatus,
  onRestartControlSocket,
  onOpenTaskManager,
  onRestorePreviousLaunch,
  notificationCommandTestStatus,
  onTestNotificationCommand,
  notificationDeliveryPreviewStatus,
  onPreviewNotificationDelivery,
  notificationToastSendStatus,
  onSendTestNotificationToast,
  onOpenSettingsFile,
  onOpenSettingsFileInCmux,
  onOpenGhosttySettingsFile,
  rawSettingsPath,
  rawSettingsDraft,
  rawSettingsLoading,
  rawSettingsSaving,
  rawSettingsError,
  rawSettingsStatus,
  onLoadRawSettings,
  onRawSettingsDraftChange,
  onSaveRawSettings,
  onResetConfig,
  onReload,
  onClose,
}: SettingsOverlayProps): React.JSX.Element | null {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const [pendingSection, setPendingSection] = useState<SettingsPaneSection | null>(
    null,
  );

  useEffect(() => {
    if (!open) {
      setPendingSection(null);
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  useEffect(() => {
    if (open && targetSection != null) {
      setPendingSection(targetSection);
    }
  }, [open, targetSection, navigationKey]);

  useEffect(() => {
    if (!open || pendingSection == null || searchQuery.trim() !== "") {
      return;
    }
    const target = bodyRef.current?.querySelector<HTMLElement>(
      `[data-section="${pendingSection}"]`,
    );
    target?.scrollIntoView({ block: "start", behavior: "smooth" });
    setPendingSection(null);
  }, [open, pendingSection, searchQuery]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="cmux-settings-overlay"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <section
        className="cmux-settings-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="cmux-settings-modal-header">
          <div>
            <h2 className="cmux-settings-modal-title">Settings</h2>
            <p className="cmux-settings-modal-subtitle">
              App, automation, browser, terminal, sidebar, and cmux.json.
            </p>
          </div>
          <div className="cmux-settings-modal-actions">
            {saving ? <span className="cmux-settings-status">Saving…</span> : null}
            <button
              type="button"
              className="cmux-settings-close"
              aria-label="Close settings"
              onClick={onClose}
            >
              Close
            </button>
          </div>
        </header>
        {error ? (
          <div className="cmux-settings-banner" role="alert">
            <span>{error}</span>
            {onReload ? (
              <button type="button" onClick={onReload}>
                Retry
              </button>
            ) : null}
          </div>
        ) : null}
        <div ref={bodyRef} className="cmux-settings-modal-body">
          {loading || config == null ? (
            <div className="cmux-settings-loading">Loading settings…</div>
          ) : (
            <SettingsPane
              config={config}
              onChange={onChange}
              onAction={onAction}
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
              onRefreshAgentProviderStatus={onRefreshAgentProviderStatus}
              onRefreshBrowserImportProfiles={onRefreshBrowserImportProfiles}
              onStartBrowserImport={onStartBrowserImport}
              onRefreshGlobalHotkeyStatus={onRefreshGlobalHotkeyStatus}
              onRefreshMobilePairingStatus={onRefreshMobilePairingStatus}
              onRefreshUpdaterStatus={onRefreshUpdaterStatus}
              onInstallCli={onInstallCli}
              onUninstallCli={onUninstallCli}
              onRefreshCliInstallStatus={onRefreshCliInstallStatus}
              onRefreshConfigExtensionStatus={onRefreshConfigExtensionStatus}
              onRefreshDesktopCoreStatus={onRefreshDesktopCoreStatus}
              onMakeDefaultTerminal={onMakeDefaultTerminal}
              onRefreshDefaultTerminalStatus={onRefreshDefaultTerminalStatus}
              onRefreshVSCodeInlineStatus={onRefreshVSCodeInlineStatus}
              onOpenFolderInVSCodeInline={onOpenFolderInVSCodeInline}
              onRestartVSCodeServeWeb={onRestartVSCodeServeWeb}
              onStopVSCodeServeWeb={onStopVSCodeServeWeb}
              onRefreshControlSocketStatus={onRefreshControlSocketStatus}
              onRestartControlSocket={onRestartControlSocket}
              onOpenTaskManager={onOpenTaskManager}
              onRestorePreviousLaunch={onRestorePreviousLaunch}
              notificationCommandTestStatus={notificationCommandTestStatus}
              onTestNotificationCommand={onTestNotificationCommand}
              notificationDeliveryPreviewStatus={notificationDeliveryPreviewStatus}
              onPreviewNotificationDelivery={onPreviewNotificationDelivery}
              notificationToastSendStatus={notificationToastSendStatus}
              onSendTestNotificationToast={onSendTestNotificationToast}
              onOpenSettingsFile={onOpenSettingsFile}
              onOpenSettingsFileInCmux={onOpenSettingsFileInCmux}
              onOpenGhosttySettingsFile={onOpenGhosttySettingsFile}
              rawSettingsPath={rawSettingsPath}
              rawSettingsDraft={rawSettingsDraft}
              rawSettingsLoading={rawSettingsLoading}
              rawSettingsSaving={rawSettingsSaving}
              rawSettingsError={rawSettingsError}
              rawSettingsStatus={rawSettingsStatus}
              onLoadRawSettings={onLoadRawSettings}
              onRawSettingsDraftChange={onRawSettingsDraftChange}
              onSaveRawSettings={onSaveRawSettings}
              onResetConfig={onResetConfig}
              searchQuery={searchQuery}
              onSearchQueryChange={onSearchQueryChange}
              onNavigate={(section) => setPendingSection(section)}
            />
          )}
        </div>
      </section>
    </div>
  );
}
