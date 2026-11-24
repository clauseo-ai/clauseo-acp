// The desktop's permissions host offers the matters-folder choice now the host has set_default_location: the picker,
// then the command with the folder picked; a cancelled pick sends nothing, and a refusal comes back as its code.
import { expect, test } from "vitest";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { CommandEnvelope } from "../../transport/protocol.ts";
import { permissionsHostOf } from "./permissions-host.ts";

const desktop = (
  picked: { folder: string; display: string } | null,
  refuse?: string,
): { transport: HostTransport; sent: CommandEnvelope[] } => {
  const sent: CommandEnvelope[] = [];
  const transport = {
    kind: "tauri",
    pickFolder: () => Promise.resolve(picked),
    dispatch: (command: CommandEnvelope) => {
      sent.push(command);
      return refuse === undefined
        ? Promise.resolve("done")
        : Promise.reject({ code: refuse, message: "x", retryable: false });
    },
  } as unknown as HostTransport;
  return { transport, sent };
};

test("the desktop's host picks a folder, then sends set_default_location with it", async () => {
  const { transport, sent } = desktop({ folder: "folder-ref-1", display: "D:\\Matters" });
  const choose = permissionsHostOf(transport).chooseMattersFolder;
  expect(choose).toBeDefined();
  expect(await choose!()).toEqual({ done: "D:\\Matters" });
  expect(sent.map((c) => c.command)).toEqual([
    { set_default_location: { folder: "folder-ref-1" } },
  ]);
  expect(typeof sent[0]?.command_id).toBe("string");
});

test("a cancelled pick sends nothing", async () => {
  const { transport, sent } = desktop(null);
  expect(await permissionsHostOf(transport).chooseMattersFolder!()).toBeNull();
  expect(sent).toEqual([]);
});

test("a refusal comes back as its code; an unknown one as other", async () => {
  const locked = desktop({ folder: "f", display: "D:\\M" }, "settings.locked");
  expect(await permissionsHostOf(locked.transport).chooseMattersFolder!()).toEqual({
    refused: "settings.locked",
  });
  const odd = desktop({ folder: "f", display: "D:\\M" }, "internal");
  expect(await permissionsHostOf(odd.transport).chooseMattersFolder!()).toEqual({
    refused: "other",
  });
});
