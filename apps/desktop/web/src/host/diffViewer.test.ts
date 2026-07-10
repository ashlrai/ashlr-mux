import { describe, expect, mock, test } from "bun:test";

const invokeCalls: Array<{ method: string; params: unknown }> = [];

mock.module("./host", () => ({
  host: {
    invoke: async (method: string, params?: unknown) => {
      invokeCalls.push({ method, params });
      return { token: "tok-abcdef0123456789", requestPath: "/index.html" };
    },
  },
}));

const { createDiffSession } = await import("./diffViewer");

describe("createDiffSession", () => {
  test("invokes the native diff session creator", async () => {
    invokeCalls.length = 0;
    await expect(createDiffSession()).resolves.toEqual({
      token: "tok-abcdef0123456789",
      requestPath: "/index.html",
    });
    expect(invokeCalls).toEqual([{ method: "diff_create_session", params: undefined }]);
  });
});
