import { Icon } from "@cmux/webviews/src/icons";

import { Workspace } from "./components/Workspace";

/**
 * The Phase 1 shell + the Phase 2 live workspace. The header uses a reused
 * `cmux/webviews` component (`Icon`); the body renders `Workspace` — a flat
 * portal layer of live terminals driven by the Rust session snapshot, with
 * per-pane split/close controls and draggable dividers.
 */
export function App(): React.JSX.Element {
  return (
    <>
      <header className="cmux-app-header flex items-center gap-2 px-3 py-1.5 text-[13px] text-neutral-300 select-none">
        <span className="cmux-icon inline-flex h-4 w-4 text-neutral-400">
          <Icon name="classic" />
        </span>
        <span className="font-medium">cmux for Windows</span>
      </header>
      <div className="cmux-workspace">
        <Workspace />
      </div>
    </>
  );
}
