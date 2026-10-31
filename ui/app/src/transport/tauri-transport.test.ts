import { afterEach, describe, expect, test, vi } from "vitest";

// Tauri's core module, replaced: `invoke` answers per command from `answers`, and the channel a transport opens is
// kept so a test can play messages into it.
const answers = new Map<string, (args: unknown) => unknown>();
const opened: { onmessage: (m: unknown) => void }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: async (command: string, args: unknown) => {
    const answer = answers.get(command);
    if (answer === undefined) return null;
    return answer(args);
  },
  Channel: class {
    onmessage: (m: unknown) => void;
    constructor(onmessage: (m: unknown) => void) {
      this.onmessage = onmessage;
      opened.push(this);
    }
  },
}));

const { TauriTransport, pageTransport } = await import("./tauri-transport.ts");

afterEach(() => {
  answers.clear();
  opened.length = 0;
});

describe("TauriTransport", () => {
  test("a host error rejects as itself", async () => {
    const error = { code: "thread.turn_running", message: "busy", retryable: true };
    answers.set("dispatch", () => Promise.reject(error));
    const t = new TauriTransport(() => {});
    await expect(
      t.dispatch({ command_id: "c", command: { cancel: { thread: "x" } } }),
    ).rejects.toEqual(error);
  });

  test("a Tauri string rejection and a thrown Error each reject as internal", async () => {
    answers.set("query", () => Promise.reject("command query not allowed"));
    answers.set("pick_folder", () => {
      throw new Error("ipc down");
    });
    const t = new TauriTransport(() => {});
    await expect(t.query("list_matters")).rejects.toEqual({
      code: "internal",
      message: "command query not allowed",
      retryable: false,
    });
    await expect(t.pickFolder()).rejects.toEqual({
      code: "internal",
      message: "ipc down",
      retryable: false,
    });
  });

  test("one channel per transport; batches reach the stream's listener and count frames", async () => {
    const seen: string[] = [];
    answers.set("subscribe", (args) => {
      seen.push(JSON.stringify(args));
      return null;
    });
    const t = new TauriTransport(() => {});
    const got: unknown[] = [];
    t.subscribe("thread:x", null, (b) => got.push(b));
    await vi.waitFor(() => expect(seen).toEqual(['{"stream":"thread:x","after":null}']));
    const batch = { stream: "thread:x", epoch: "e", patches: [] };
    opened[0]!.onmessage(JSON.stringify({ patches: batch }));
    expect(opened).toHaveLength(1);
    expect(got).toEqual([batch]);
    expect(t.frames()).toBe(1);
  });

  test("outside Tauri the page transport reaches nothing", async () => {
    const t = pageTransport();
    expect(t.kind).toBe("null");
    expect(pageTransport()).toBe(t);
    await expect(t.query("list_matters")).rejects.toMatchObject({ code: "internal" });
  });
});
