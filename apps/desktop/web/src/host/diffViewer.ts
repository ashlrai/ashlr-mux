import { host } from "./host";

export interface CreatedDiffSession {
  token: string;
  requestPath: string;
}

export function createDiffSession(): Promise<CreatedDiffSession> {
  return host.invoke<CreatedDiffSession>("diff_create_session");
}
