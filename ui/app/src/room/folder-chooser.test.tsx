// Clauseo's own folder chooser in the room (transport/chooser.ts, folder-chooser-host.tsx), over FakeTransport whose
// picker answers `choose` with the scripted machine (fake-chooser.ts): it opens over New matter, moves by index, Up,
// start point and typed location, chooses with chooser_choose and resolves the pick with the folder, cancels with
// chooser_cancel and resolves null, stays open when the confirm is declined, and Try again reads a folder that didn't
// answer. The folder picker's line covers the check before the chooser shows, and goes once it shows.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { CommandEnvelope, CommandOutcome, MatterSummary } from "../transport/protocol.ts";
import { FAKE_CHOOSER, type FakeChooser } from "../transport/fake-chooser.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.useRealTimers();
  for (const n of document.body.querySelectorAll("[data-base-ui-portal], [role=dialog]"))
    n.remove();
});

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const MADE = uuid(5, 2);
const NAME = "Mehta v. Mehta";

/** Today's host with no default location, its picker the chooser over `chooser`. */
function today(chooser: FakeHost["chooser"] = FAKE_CHOOSER): FakeHost {
  let list: MatterSummary[] = [A];
  return {
    query: () => ({ matters: list }),
    dispatch: (command: CommandEnvelope): CommandOutcome => {
      if (!("create_matter" in command.command)) return "done";
      list = [A, { matter: MADE, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: MADE } };
    },
    chooser,
  };
}

async function mount(h: FakeHost) {
  const transport = new FakeTransport(h);
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Room transport={transport} />));
  await flush();
  return transport;
}

async function flush() {
  for (let i = 0; i < 3; i += 1) {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => settled());
  }
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);
const allById = (id: string) => [
  ...document.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`),
];
const chooser = () => byId("folder-chooser");
const newMatter = () => document.querySelector(".new-matter-dialog");
const names = () => allById("chooser-item").map((o) => o.textContent);

function type(input: HTMLInputElement, text: string) {
  Reflect.set(HTMLInputElement.prototype, "value", text, input);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

async function openForm() {
  await act(async () => byId("new-matter")?.click());
  await flush();
  await act(async () => type(byId<HTMLInputElement>("matter-name") as HTMLInputElement, NAME));
}

async function press(el: HTMLElement | null | undefined) {
  await act(async () => el?.click());
  await flush();
}

async function key(k: string) {
  await act(async () => {
    byId("chooser-list")?.dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
    );
  });
  await flush();
}

/** The session's calls, in order, past the pick. */
const sessionCalls = (transport: FakeTransport) =>
  transport.calls.filter((c) => c.call.startsWith("chooser_"));
const lists = (transport: FakeTransport) =>
  transport.calls.flatMap((c) => (c.call === "chooser_list" ? [c.go] : []));

const created = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((d) => d.command.command)
    .filter((c) => "create_matter" in c);

test("Change opens the chooser over New matter at the last place; choosing resolves the pick with the folder", async () => {
  const transport = await mount(today());
  await openForm();
  await press(byId("matter-existing"));
  expect(chooser()).not.toBeNull();
  expect(newMatter()).not.toBeNull();
  expect(lists(transport)).toEqual([{ kind: "start", key: 4 }]);
  expect(byId("chooser-where")?.textContent).toBe("Home›Documents›Matters");
  expect(names()).toEqual(["Iyer v. Iyer", "Mehta v. Mehta", "Rathi Steels", "Zaveri Exports"]);
  // into Rathi Steels by its index, with the keys
  await key("ArrowDown");
  await key("ArrowDown");
  await key("Enter");
  expect(lists(transport).at(-1)).toEqual({ kind: "into", index: 2 });
  expect(names()).toEqual(["Orders", "Pleadings"]);
  await press(byId("chooser-choose"));
  expect(sessionCalls(transport).at(-1)).toEqual({ call: "chooser_choose", chooser: 1 });
  expect(chooser()).toBeNull();
  expect(newMatter()?.textContent).toContain("Documents\\Matters\\Rathi Steels");
  // the pick went the transport's one way, and the folder registered is the one the form holds
  expect(transport.only("pick_folder")).toHaveLength(1);
});

test("Up, a start point and a typed location each list where they say", async () => {
  const transport = await mount(today());
  await openForm();
  await press(byId("matter-existing"));
  await press(byId("chooser-up"));
  expect(lists(transport).at(-1)).toEqual({ kind: "up" });
  expect(names()).toEqual(["ClauseoCan't be a matter", "Matters", "Old drafts"]);
  await key("Backspace");
  expect(lists(transport).at(-1)).toEqual({ kind: "up" });
  expect(byId("chooser-where")?.textContent).toBe("Home");
  await press(allById("chooser-start")[0]);
  expect(lists(transport).at(-1)).toEqual({ kind: "start", key: 0 });
  expect(byId("chooser-where")?.textContent).toBe("C:");
  const field = byId<HTMLInputElement>("chooser-typed") as HTMLInputElement;
  await act(async () => type(field, "d:\\cases"));
  await press(byId("chooser-go"));
  expect(lists(transport).at(-1)).toEqual({ kind: "typed", path: "d:\\cases" });
  expect(byId("chooser-where")?.textContent).toBe("D:›Cases");
  expect(names()).toEqual(["Acme Traders", "Khan Leather Works"]);
  await act(async () => type(field, "C:\\Nowhere"));
  await press(byId("chooser-go"));
  expect(byId("chooser-state")?.textContent).toBe("This folder can't be found.");
  // a location the desktop refuses: Clauseo can't use it, and the chooser stays
  await act(async () => type(field, "C:\\Matters\\..\\Windows"));
  await press(byId("chooser-go"));
  expect(byId("chooser-state")?.textContent).toBe("Clauseo can't use this folder.");
  expect(chooser()).not.toBeNull();
});

test("Cancel closes the session and the pick is null; Escape does the same and leaves New matter open", async () => {
  const transport = await mount(today());
  await openForm();
  await press(byId("matter-existing"));
  await press(byId("chooser-cancel"));
  expect(sessionCalls(transport).at(-1)).toEqual({ call: "chooser_cancel", chooser: 1 });
  expect(chooser()).toBeNull();
  expect(newMatter()).not.toBeNull();
  expect(newMatter()?.textContent).not.toContain("Documents\\");
  await press(byId("matter-existing"));
  expect(chooser()).not.toBeNull();
  await key("Escape");
  expect(sessionCalls(transport).at(-1)).toEqual({ call: "chooser_cancel", chooser: 2 });
  expect(chooser()).toBeNull();
  expect(newMatter()).not.toBeNull();
});

test("a folder that doesn't answer says so, and Try again reads it again; a refused folder doesn't open", async () => {
  const tree: FakeChooser = { ...FAKE_CHOOSER, last: null };
  const transport = await mount(today(tree));
  await openForm();
  await press(byId("matter-existing"));
  // home: Desktop, Documents, Downloads, OneDrive (which doesn't answer)
  expect(byId("chooser-where")?.textContent).toBe("Home");
  await press(allById("chooser-item")[3]);
  expect(byId("chooser-state")?.textContent).toBe("This folder didn't answer in time.");
  await press(byId("chooser-again"));
  expect(lists(transport).at(-1)).toEqual({ kind: "again" });
  expect(byId("chooser-state")?.textContent).toBe("This folder didn't answer in time.");
  // C:, where Program Files and Windows are refused: neither a press nor Enter opens one
  await press(allById("chooser-start")[0]);
  const before = lists(transport).length;
  const refused = allById("chooser-item").filter((o) => o.getAttribute("aria-disabled"));
  expect(refused.map((o) => o.textContent)).toEqual([
    "Program FilesCan't be a matter",
    "WindowsCan't be a matter",
  ]);
  await press(refused[0]);
  await key("ArrowDown");
  await key("Enter");
  expect(lists(transport).length).toBe(before);
  // D:\Archive has more than it shows
  await press(allById("chooser-start")[1]);
  await press(allById("chooser-item")[0]);
  expect(byId("chooser-more")?.textContent).toBe("Some folders aren't shown.");
});

test("a declined confirm keeps the chooser open where it was; the next confirm chooses", async () => {
  let confirm = false;
  const tree: FakeChooser = {
    ...FAKE_CHOOSER,
    choose: (path) =>
      confirm ? { folder: "ref-chosen", display: path.replace("C:\\Users\\rmehra\\", "") } : null,
  };
  const transport = await mount(today(tree));
  await openForm();
  await press(byId("matter-existing"));
  await press(byId("chooser-choose"));
  expect(sessionCalls(transport).at(-1)).toEqual({ call: "chooser_choose", chooser: 1 });
  expect(chooser()).not.toBeNull();
  expect(byId("chooser-where")?.textContent).toBe("Home›Documents›Matters");
  expect(byId<HTMLButtonElement>("chooser-choose")?.disabled).toBe(false);
  confirm = true;
  await press(byId("chooser-choose"));
  expect(chooser()).toBeNull();
  expect(newMatter()?.textContent).toContain("Documents\\Matters");
});

test("while the confirm is up, the chooser waits and says so", async () => {
  let answer: (picked: { folder: string; display: string } | null) => void = () => {};
  const tree: FakeChooser = {
    ...FAKE_CHOOSER,
    choose: () => new Promise((resolve) => (answer = resolve)),
  };
  await mount(today(tree));
  await openForm();
  await press(byId("matter-existing"));
  await press(byId("chooser-choose"));
  expect(byId("chooser-waiting")?.textContent).toBe("Waiting for you to confirm…");
  expect(byId<HTMLButtonElement>("chooser-choose")?.disabled).toBe(true);
  await act(async () => answer(null));
  await flush();
  expect(byId("chooser-waiting")).toBeNull();
  expect(chooser()).not.toBeNull();
});

test("Create with no folder picked: the chooser picks the parent, and the matter is made in it", async () => {
  const transport = await mount(today());
  await openForm();
  await press(byId("create-matter"));
  expect(chooser()).not.toBeNull();
  await press(byId("chooser-choose"));
  expect(created(transport)).toEqual([
    {
      create_matter: {
        name: NAME,
        source: { new_folder: { folder: "chosen:C:\\Users\\rmehra\\Documents\\Matters" } },
      },
    },
  ]);
});

test("the picker's line covers the check before the chooser, and goes once the chooser shows", async () => {
  let check: (tree: FakeChooser) => void = () => {};
  await mount(today(() => new Promise((resolve) => (check = resolve))));
  await openForm();
  await press(byId("matter-existing"));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300);
  });
  expect(byId("picker-opening")?.textContent).toBe("Opening the folder picker…");
  await act(async () => check(FAKE_CHOOSER));
  await flush();
  expect(chooser()).not.toBeNull();
  expect(byId("picker-opening")).toBeNull();
  await press(byId("chooser-cancel"));
  expect(byId("picker-opening")).toBeNull();
});
