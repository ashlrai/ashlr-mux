/**
 * Resolve which panes own a mounted agent surface.
 *
 * Each agent pane gets its own mounted `AgentSessionSurface`. Multiple mounted
 * surfaces are safe: the native side broadcasts every agent event to all
 * instances, and each chat instance filters by its own `runningSessionId`
 * (`webviews/agent-session/shared/sessionModel.ts` `reduceSession`), so
 * concurrent agents in separate panes route correctly — live-confirmed with
 * the multi-session store (`b6b9ed726`). The one residual hazard is two panes
 * starting the SAME provider in the same tick racing for `provider.started`;
 * that race predates this module and is unaffected by mount bookkeeping.
 *
 * What a surface may NOT do is unmount while its run is in flight: the agent
 * app has NO transcript replay — `loadInitialData` fetches only context +
 * providers, the transcript is rebuilt purely from live events, and a fresh
 * mount re-fires auto-start — so unmounting is irrecoverable and can even
 * spawn a second run. The workspace therefore keeps a pane's surface mounted
 * (hidden) across a toggle back to the terminal. This function encodes that
 * stickiness. Given the previous owners:
 *  - every pane that is CURRENTLY an agent owns a surface;
 *  - a previous owner keeps its (hidden) surface as long as the pane still
 *    exists — toggling it to a terminal does not tear down its in-flight run;
 *  - a closed pane's ownership is dropped.
 *
 * @param previous       the panes that owned a surface on the last render
 * @param liveAgentPanes the panes whose `surface_kind` is currently `"agent"`
 * @param livePanelIds   ids of every pane that currently exists
 */
export function stickyAgentPanes(
  previous: ReadonlySet<string>,
  liveAgentPanes: ReadonlySet<string>,
  livePanelIds: ReadonlySet<string>,
): ReadonlySet<string> {
  const owners = new Set<string>();
  for (const panelId of previous) {
    if (livePanelIds.has(panelId)) {
      owners.add(panelId);
    }
  }
  for (const panelId of liveAgentPanes) {
    owners.add(panelId);
  }
  return owners;
}
