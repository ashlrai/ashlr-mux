import type { DockSurfaceSnapshot } from "../dock";
import { BrowserSurface } from "./BrowserSurface";
import { TerminalSurface } from "./TerminalSurface";

export function DockRuntimeSurface({
  surface,
  active,
}: {
  surface: DockSurfaceSnapshot;
  active: boolean;
}): React.JSX.Element {
  return surface.runtime.type === "terminal" ? (
    <TerminalSurface
      panelId={surface.id}
      cwd={surface.runtime.working_directory ?? undefined}
      initialCommand={surface.runtime.command ?? undefined}
      environment={surface.runtime.environment}
      isActive={active}
    />
  ) : (
    <BrowserSurface panelId={surface.id} url={surface.runtime.url} />
  );
}
