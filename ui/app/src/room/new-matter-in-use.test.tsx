// The folder in use (folder refusals): New matter on a folder a matter on the list already uses is refused
// (matter.folder_is_matter, naming it), at the pick's check or at Create. The form names the folder and the matter,
// offers Open {name}, which opens that matter and closes the form, and holds Create until another folder is picked; the
// matter is never opened for the lawyer. A folder nested with one still being made names the folder too.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import type { HostQuery, MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { NextCommand, NextHostError } from "./host-next.ts";
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
const OPEN: MatterSummary = { matter: uuid(3, 2), name: "Rao v. Union of India", kind: "folder" };
const NAME = "Mehta family trust";
const IN_USE: NextHostError = {
  code: "matter.folder_is_matter",
  message: "SECRET-DETAIL",
  retryable: false,
  matter: A.matter,
};
const iso = (text: string) => `\u2068${text}\u2069`;
/** UC-264 for the folder picked first. */
const IN_USE_LINE = `${iso(A.name)} already uses ${iso("~/Documents/Folder 1")}. Open ${iso(A.name)}, or choose another folder.`;

/** A NextHost listing A and OPEN (opened last, so the room shows it), picking "Documents/Folder n" in turn. The first
 * pick's FolderSupport check refuses with `check`, and a create from it with `create`; commands land in `sent`. */
function hostOf(sent: NextCommand[], refusals: { check?: NextHostError; create?: NextHostError }) {
  let picks = 0;
  const h: FakeHost = {
    query: (query: HostQuery) => {
      if (typeof query === "object" && "folder_support" in query) {
        if (refusals.check !== undefined && picks === 1) throw refusals.check;
        return { folder_support: {} };
      }
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    pickFolder: () => {
      picks += 1;
      return { folder: `ref-${picks}`, display: `Documents/Folder ${picks}` };
    },
    next: fakeNext({
      matters: () => [detailed(A), detailed(OPEN, { last_opened: 1 })],
      location: "Documents/Clauseo",
      command: (c) => {
        sent.push(c);
        if ("create_matter" in c && refusals.create !== undefined) throw refusals.create;
        return undefined;
      },
    }),
  };
  return h;
}

async function mount(h: FakeHost) {
  const transport = new FakeTransport(h);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await settle();
  return transport;
}

async function settle() {
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);
const dialog = () => document.querySelector('[role="dialog"]');
const alert = () => dialog()?.querySelector('[role="alert"]')?.textContent ?? null;
const title = () => host?.querySelector('[data-testid="matter"]')?.textContent;
const createHeld = () => byId<HTMLButtonElement>("create-matter")?.disabled;

function type(input: HTMLInputElement, text: string) {
  Reflect.set(HTMLInputElement.prototype, "value", text, input);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

async function openForm() {
  await act(async () => byId("new-matter")?.click());
  await act(async () => settled());
  await act(async () => type(byId<HTMLInputElement>("matter-name") as HTMLInputElement, NAME));
}

async function press(id: string) {
  await act(async () => byId(id)?.click());
  await settle();
}

const opened = (sent: NextCommand[]) => sent.filter((c) => "open_matter" in c);
const creates = (sent: NextCommand[]) => sent.filter((c) => "create_matter" in c);

test("refused at Create: the folder and the matter using it named, and Open {name} opens that matter, nothing made", async () => {
  const sent: NextCommand[] = [];
  await mount(hostOf(sent, { create: IN_USE }));
  expect(title()).toBe(OPEN.name);
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(alert()).toBe(IN_USE_LINE);
  expect(dialog()?.textContent).not.toContain("SECRET-DETAIL");
  // never opened for the lawyer: the room still shows the matter it showed
  expect(opened(sent)).toEqual([]);
  expect(title()).toBe(OPEN.name);
  expect(createHeld()).toBe(true);
  expect(byId("open-matter")?.textContent).toBe(`Open ${iso(A.name)}`);
  await press("open-matter");
  expect(sent).toContainEqual({ open_matter: { matter: A.matter } });
  expect(creates(sent)).toHaveLength(1);
  expect(dialog()).toBeNull();
  expect(title()).toBe(A.name);
});

test("refused at the pick's check: said at once, Create held until another folder is picked, which drops the line", async () => {
  const sent: NextCommand[] = [];
  const transport = await mount(hostOf(sent, { check: IN_USE }));
  await openForm();
  await press("matter-existing");
  expect(alert()).toBe(IN_USE_LINE);
  expect(byId("open-matter")?.textContent).toBe(`Open ${iso(A.name)}`);
  expect(createHeld()).toBe(true);
  // Create pressed anyway does nothing
  await press("create-matter");
  expect(creates(sent)).toEqual([]);
  // the usual Choose folder… picks another
  await press("matter-existing");
  expect(transport.only("pick_folder")).toHaveLength(2);
  expect(alert()).toBeNull();
  expect(byId("open-matter")).toBeNull();
  expect(dialog()?.textContent).toContain("~/Documents/Folder 2");
  expect(createHeld()).toBe(false);
  expect(opened(sent)).toEqual([]);
  expect(title()).toBe(OPEN.name);
});

test("a folder nested with one still being made names the folder; Use a new folder names the new folder", async () => {
  const sent: NextCommand[] = [];
  await mount(
    hostOf(sent, {
      create: { code: "matter.folder_being_made", message: "", retryable: false },
    }),
  );
  await openForm();
  await press("matter-existing");
  await press("create-matter");
  expect(alert()).toBe(
    `${iso("~/Documents/Folder 1")} is inside, or contains, a folder Clauseo is still making into a matter. Try again in a moment, or choose another folder.`,
  );
  expect(byId("open-matter")).toBeNull();
  expect(createHeld()).toBe(true);
  // the default location's new folder: <location>/<name>
  await press("matter-made");
  await press("create-matter");
  expect(alert()).toContain(iso(`~/Documents/Clauseo/${NAME}`));
});
