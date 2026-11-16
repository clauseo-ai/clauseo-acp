// TauriTransport's open, reveal, quit and settings calls: what each sends, and the host's refusals.
import { afterEach, describe, expect, test, vi } from "vitest";

// Tauri's core module, replaced as in tauri-transport.test.ts: `invoke` answers per command from `answers`.
const answers = new Map<string, (args: unknown) => unknown>();
const opened: { onmessage: (m: unknown) => void }[] = [];
const invoked: { command: string; args: unknown }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: async (command: string, args: unknown) => {
    invoked.push({ command, args });
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

const { TauriTransport } = await import("./tauri-transport.ts");

afterEach(() => {
  answers.clear();
  opened.length = 0;
  invoked.length = 0;
});

describe("TauriTransport host calls", () => {
  test("open_external, open_file and reveal send their argument; open_file resolves to what the host opened", async () => {
    const t = new TauriTransport({ warn: () => {} });
    await expect(t.openExternal("https://example.com/a")).resolves.toBeUndefined();
    await expect(t.openExternal("https://example.com/b", "example.com")).resolves.toBeUndefined();
    await expect(t.openExternal("https://example.com/c", undefined, true)).resolves.toBeUndefined();
    answers.set("open_file", () => ({ protected_view: true }));
    await expect(t.openFile("ref-1")).resolves.toEqual({ protected_view: true });
    await expect(t.reveal({ matter: "m-1" })).resolves.toBeUndefined();
    await expect(t.reveal("app_data")).resolves.toBeUndefined();
    const sent = invoked.filter((i) =>
      ["open_external", "open_file", "reveal"].includes(i.command),
    );
    expect(sent).toEqual([
      {
        command: "open_external",
        args: { url: "https://example.com/a", shown: null, alwaysAsk: null },
      },
      {
        command: "open_external",
        args: { url: "https://example.com/b", shown: "example.com", alwaysAsk: null },
      },
      {
        command: "open_external",
        args: { url: "https://example.com/c", shown: null, alwaysAsk: true },
      },
      { command: "open_file", args: { file: "ref-1" } },
      { command: "reveal", args: { target: { matter: "m-1" } } },
      { command: "reveal", args: { target: "app_data" } },
    ]);
  });

  test("quit_app asks the host to quit; the call resolves once the host has it", async () => {
    const t = new TauriTransport({ warn: () => {} });
    await expect(t.quitApp()).resolves.toBeUndefined();
    expect(invoked.filter((i) => i.command === "quit_app")).toHaveLength(1);
  });

  test("open_settings asks the host to open or focus the settings window; a refusal rejects as a HostError", async () => {
    const t = new TauriTransport({ warn: () => {} });
    await expect(t.openSettings()).resolves.toBeUndefined();
    expect(invoked.filter((i) => i.command === "open_settings")).toHaveLength(1);
    answers.set("open_settings", () => Promise.reject("command open_settings not allowed"));
    await expect(t.openSettings()).rejects.toEqual({
      code: "internal",
      message: "command open_settings not allowed",
      retryable: false,
    });
  });

  test("a declined confirm and a refused open or reveal reject with the host's error", async () => {
    const declined = { code: "transport.unauthorized", message: "declined", retryable: false };
    const missing = { code: "matter.folder_missing", message: "gone", retryable: true };
    answers.set("open_external", () => Promise.reject(declined));
    answers.set("open_file", () => Promise.reject(missing));
    answers.set("reveal", () => Promise.reject("command reveal not allowed"));
    const t = new TauriTransport({ warn: () => {} });
    await expect(t.openExternal("https://example.com")).rejects.toEqual(declined);
    await expect(t.openFile("ref-1")).rejects.toEqual(missing);
    await expect(t.reveal("app_data")).rejects.toEqual({
      code: "internal",
      message: "command reveal not allowed",
      retryable: false,
    });
  });
});
