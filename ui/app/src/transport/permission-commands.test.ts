// The desktop's permissions commands as the page calls them (permission-commands.ts), over a mocked Tauri invoke: the
// rows permissions() answers come back as the desktop writes them (crates/desktop/src/permissions.rs), ask_permission
// takes a listed id with no folder, and a refusal rejects as a HostError. Also the folder pick's start, as TauriTransport sends it.
import { afterEach, expect, test, vi } from "vitest";

const answers = new Map<string, (args: unknown) => unknown>();
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
    }
  },
}));

const { askPermissionCommand, permissionsCommand } = await import("./permission-commands.ts");
const { TauriTransport } = await import("./tauri-transport.ts");

afterEach(() => {
  answers.clear();
  invoked.length = 0;
});

/** A Mac's rows as permissions.rs writes them: Documents denied (with its path), the keychain still to ask. */
const MAC = [
  {
    id: "files_documents",
    state: "denied",
    folder: "/Users/rmehra/Documents",
    settings_path: "System Settings › Privacy & Security › Files and Folders › Clauseo",
    note: null,
  },
  {
    id: "keychain",
    state: "would_ask",
    folder: null,
    settings_path: null,
    note: "Clauseo Safe Storage (rmehra)",
  },
];

test("permissions(): the desktop's rows as it wrote them, by the real command's name and no arguments", async () => {
  answers.set("permissions", () => MAC);
  expect(await permissionsCommand()).toEqual(MAC);
  expect(invoked).toEqual([{ command: "permissions", args: undefined }]);
});

test("ask_permission(id): a listed id with no folder, and the row after", async () => {
  answers.set("ask_permission", () => ({ ...MAC[1], state: "granted" }));
  expect((await askPermissionCommand("keychain")).state).toBe("granted");
  expect(invoked).toEqual([{ command: "ask_permission", args: { id: "keychain", folder: null } }]);
});

test("a refusal rejects as the host's error", async () => {
  answers.set("permissions", () => {
    throw { code: "command.invalid", message: "not listed", retryable: false };
  });
  await expect(permissionsCommand()).rejects.toMatchObject({ code: "command.invalid" });
});

test("the folder pick sends its start, and null without one (the last pick's folder)", async () => {
  answers.set("pick_folder_or_choose", () => ({ kind: "cancelled" }));
  const transport = new TauriTransport({ warn: () => {} });
  await transport.pickFolder("documents");
  await transport.pickFolder();
  expect(invoked.filter((i) => i.command === "pick_folder_or_choose")).toEqual([
    { command: "pick_folder_or_choose", args: { start: "documents" } },
    { command: "pick_folder_or_choose", args: { start: null } },
  ]);
});
