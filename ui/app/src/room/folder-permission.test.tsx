// A guarded folder picked in New matter (issue 5, the pick's part; room/folder-permission.tsx), over FakeTransport: each
// kind's reason line with Allow, nothing for a folder without one, Allow asking ask_permission with the pick's ref and
// nothing asked before the press, granted clearing the line, and denied keeping Create with the System Settings line.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import type { CommandEnvelope, CommandOutcome, MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import {
  provideFolderPermission,
  type FolderPermissionId,
  type FolderPermissionRow,
} from "../transport/folder-permission.ts";
import { uuid } from "../testing/stream-script.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const MADE = uuid(5, 2);
const REF = "ref-guarded";
const PATH = "System Settings › Privacy & Security › Files and Folders";

/** A host whose picker gives REF, carrying `permission` as the desktop's PickedFolder does. */
function guarded(permission: FolderPermissionId | null): FakeHost {
  let list: MatterSummary[] = [A];
  return {
    query: () => ({ matters: list }),
    dispatch: (command: CommandEnvelope): CommandOutcome => {
      if (!("create_matter" in command.command)) return "done";
      list = [A, { matter: MADE, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: MADE } };
    },
    pickFolder: () => ({ folder: REF, display: "Desktop/Rathi Steels", permission }),
  };
}

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

/** The room over `h`, its ask_permission answered by `answer` and every ask recorded. */
async function mount(
  h: FakeHost,
  answer: (id: FolderPermissionId) => FolderPermissionRow | Promise<FolderPermissionRow>,
) {
  const transport = new FakeTransport(h);
  const asks: [FolderPermissionId, string][] = [];
  provideFolderPermission(transport, async (id, folder) => {
    asks.push([id, folder]);
    return answer(id);
  });
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return { transport, asks };
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

/** New matter, then Existing folder: REF picked at once. */
async function pick() {
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  await press("matter-existing");
}

const row = (id: FolderPermissionId, state: FolderPermissionRow["state"]): FolderPermissionRow => ({
  id,
  state,
  folder: null,
  settings_path: state === "granted" ? null : PATH,
  note: null,
});

const LINES: Record<FolderPermissionId, string> = {
  files_desktop: "Clauseo needs macOS's permission to read and save papers in your Desktop folder.",
  files_downloads:
    "Clauseo needs macOS's permission to read and save papers in your Downloads folder.",
  files_removable:
    "Clauseo needs macOS's permission to read and save papers on this external drive.",
  files_network: "Clauseo needs macOS's permission to read and save papers on this network drive.",
  files_cloud: "Clauseo needs macOS's permission to read and save papers in this cloud folder.",
};

test.each(Object.entries(LINES) as [FolderPermissionId, string][])(
  "a %s folder picked: its reason line with Allow, above Create, and nothing asked without the press",
  async (id, line) => {
    const { asks } = await mount(guarded(id), (asked) => row(asked, "granted"));
    await pick();
    const shown = byId("folder-permission");
    expect(shown?.textContent).toBe(`${line}Allow`);
    expect(shown?.getAttribute("role")).toBe("status");
    expect(byId("folder-permission-allow")).not.toBeNull();
    expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
    expect(asks).toEqual([]);
  },
);

test("a folder without a permission shows no line, and nothing is asked", async () => {
  const { asks } = await mount(guarded(null), (id) => row(id, "granted"));
  await pick();
  expect(byId("matter-name")).not.toBeNull();
  expect(byId("folder-permission")).toBeNull();
  expect(asks).toEqual([]);
});

test("Allow asks ask_permission for the kind with the pick's ref; granted, the line goes and focus moves to Create", async () => {
  const { asks } = await mount(guarded("files_downloads"), (id) => row(id, "granted"));
  await pick();
  byId("folder-permission-allow")?.focus();
  await press("folder-permission-allow");
  expect(asks).toEqual([["files_downloads", REF]]);
  expect(byId("folder-permission")).toBeNull();
  expect(document.activeElement).toBe(byId("create-matter"));
});

test("denied: the line says what won't work and where to change it, and Create still makes the matter", async () => {
  const { transport, asks } = await mount(guarded("files_network"), (id) => row(id, "denied"));
  await pick();
  await press("folder-permission-allow");
  expect(asks).toEqual([["files_network", REF]]);
  expect(byId("folder-permission")?.textContent).toBe(
    `Without it, Clauseo can't open or save papers in this folder. You can allow it later in ${PATH}.`,
  );
  expect(byId("folder-permission-allow")).toBeNull();
  expect(document.activeElement).toBe(byId("folder-permission"));
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
  await press("create-matter");
  const made = transport
    .only("dispatch")
    .map((d) => d.command.command)
    .filter((c) => "create_matter" in c);
  expect(made).toEqual([
    { create_matter: { name: "Rathi Steels", source: { existing_folder: { folder: REF } } } },
  ]);
});

test("dismissed (the ask fails): the same denied line, with System Settings' Privacy & Security", async () => {
  await mount(guarded("files_cloud"), () => Promise.reject(new Error("dismissed")));
  await pick();
  await press("folder-permission-allow");
  expect(byId("folder-permission")?.textContent).toBe(
    "Without it, Clauseo can't open or save papers in this folder. You can allow it later in System Settings › Privacy & Security.",
  );
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(false);
});
