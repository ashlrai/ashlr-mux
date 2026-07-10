import { useCallback, useEffect, useState } from "react";

import { host } from "../host/host";

export interface WindowChromeState {
  isMaximized: boolean;
  title: string;
}

const FALLBACK_STATE: WindowChromeState = {
  isMaximized: false,
  title: "cmux",
};

const WINDOW_STATE_CHANGED_EVENT = "cmux://window-state-changed";

export interface UseWindowChrome {
  state: WindowChromeState;
  minimize: () => void;
  toggleMaximize: () => void;
  toggleFullscreen: () => void;
  newWindow: () => void;
  close: () => void;
}

export function useWindowChrome(): UseWindowChrome {
  const [state, setState] = useState<WindowChromeState>(FALLBACK_STATE);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void host
      .invoke<WindowChromeState>("window_state")
      .then((next) => {
        if (!disposed) {
          setState(next);
        }
      })
      .catch(() => {});

    void host
      .on<WindowChromeState>(WINDOW_STATE_CHANGED_EVENT, (next) => {
        if (!disposed) {
          setState(next);
        }
      })
      .then((off) => {
        if (disposed) {
          off();
        } else {
          unlisten = off;
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const minimize = useCallback(() => {
    void host
      .invoke<WindowChromeState>("window_minimize")
      .then(setState)
      .catch((error) => console.error("window_minimize failed", error));
  }, []);

  const toggleMaximize = useCallback(() => {
    void host
      .invoke<WindowChromeState>("window_toggle_maximize")
      .then(setState)
      .catch((error) => console.error("window_toggle_maximize failed", error));
  }, []);

  const toggleFullscreen = useCallback(() => {
    void host
      .invoke<WindowChromeState>("window_toggle_fullscreen")
      .then(setState)
      .catch((error) => console.error("window_toggle_fullscreen failed", error));
  }, []);

  const newWindow = useCallback(() => {
    void host.invoke<string>("window_new").catch((error) => {
      console.error("window_new failed", error);
    });
  }, []);

  const close = useCallback(() => {
    void host.invoke("window_close").catch((error) => {
      console.error("window_close failed", error);
    });
  }, []);

  return {
    state,
    minimize,
    toggleMaximize,
    toggleFullscreen,
    newWindow,
    close,
  };
}
