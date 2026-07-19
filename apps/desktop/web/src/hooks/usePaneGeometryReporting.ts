import { useEffect, type RefObject } from "react";

import { host } from "../host/host";

export function usePaneGeometryReporting(
  containerRef: RefObject<HTMLElement | null>,
  workspaceId: string | undefined,
  enabled: boolean,
): void {
  useEffect(() => {
    const container = containerRef.current;
    if (!container || !workspaceId || !enabled) {
      return;
    }

    let disposed = false;
    let lastGeometry = "";
    const reportGeometry = (): void => {
      const { x, y, width, height } = container.getBoundingClientRect();
      if (width <= 0 || height <= 0) {
        return;
      }
      const geometry = `${x}:${y}:${width}:${height}`;
      if (geometry === lastGeometry) {
        return;
      }
      lastGeometry = geometry;
      void host
        .invoke("pane_report_geometry", { workspaceId, x, y, width, height })
        .catch((error) => {
          if (!disposed) {
            console.error("pane geometry report failed", error);
          }
        });
    };

    reportGeometry();
    const observer =
      typeof ResizeObserver === "undefined"
        ? undefined
        : new ResizeObserver(reportGeometry);
    observer?.observe(container);
    window.addEventListener("resize", reportGeometry);
    return () => {
      disposed = true;
      observer?.disconnect();
      window.removeEventListener("resize", reportGeometry);
    };
  }, [containerRef, enabled, workspaceId]);
}
