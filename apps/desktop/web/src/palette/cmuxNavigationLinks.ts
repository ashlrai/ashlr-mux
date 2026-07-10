// Same-session cmux navigation-link builders — port of
// `CmuxNavigationURLRequest.workspaceLink` / `surfaceLink`
// (`Sources/CmuxSSHURLRequest.swift:609-626`).
//
// The host chooses the active callback scheme from build/runtime auth
// environment (`AuthEnvironment.callbackScheme`). Pure callers default to the
// stable product scheme; live command-palette callers pass the Tauri-provided
// active scheme.

const CMUX_NAVIGATION_SCHEME = "cmux";

export function workspaceLink(
  workspaceId: string,
  scheme = CMUX_NAVIGATION_SCHEME,
): string {
  return `${scheme}://workspace/${workspaceId}`;
}

export function paneLink(
  workspaceId: string,
  paneId: string,
  scheme = CMUX_NAVIGATION_SCHEME,
): string {
  return `${scheme}://workspace/${workspaceId}/pane/${paneId}`;
}

export function surfaceLink(
  workspaceId: string,
  surfaceId: string,
  scheme = CMUX_NAVIGATION_SCHEME,
): string {
  return `${scheme}://workspace/${workspaceId}/surface/${surfaceId}`;
}
