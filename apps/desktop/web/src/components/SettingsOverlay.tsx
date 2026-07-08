// Modal host for the Settings pane (E4): a scrim + centered panel wrapping the
// presentational `SettingsPane`, bound to the live config via `useConfig`.
// Escape, the ✕ button, and a backdrop click all close it; the panel itself
// swallows clicks so form interaction never dismisses.

import { useEffect } from "react";

import { useConfig } from "../hooks/useConfig";
import { SettingsPane } from "./SettingsPane";

export interface SettingsOverlayProps {
  open: boolean;
  onClose: () => void;
}

export function SettingsOverlay({
  open,
  onClose,
}: SettingsOverlayProps): React.JSX.Element | null {
  const { config, dispatch } = useConfig();

  useEffect(() => {
    if (!open) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    // Capture phase so Escape closes settings before any lower layer reacts.
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, onClose]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="cmux-settings-scrim"
      role="presentation"
      onClick={onClose}
    >
      <div
        className="cmux-settings-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="cmux-settings-titlebar">
          <span className="cmux-settings-title">Settings</span>
          <button
            type="button"
            className="cmux-settings-close"
            title="Close settings"
            aria-label="Close settings"
            onClick={onClose}
          >
            ✕
          </button>
        </div>
        <div className="cmux-settings-body">
          {config ? (
            <SettingsPane config={config} onAction={dispatch} />
          ) : (
            <div className="cmux-settings-loading">Loading…</div>
          )}
        </div>
      </div>
    </div>
  );
}
