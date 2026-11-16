// The folder picker's line (picker-wait.ts) at New matter's Choose folder…, over FakeTransport with a held pick:
// under 300 ms no line; from 300 ms a polite status line; gone the moment the pick resolves, picked, cancelled or
// errored. A pick answered inside 300 ms never shows it.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { CommandEnvelope, CommandOutcome, MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { Room } from "./room.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  // only the timers the picker's wait uses are faked: the lazy chunks land as they do
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.useRealTimers();
});

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const MADE = uuid(5, 2);
const NAME = "Mehta v. Mehta";

/** Today's host with no default location: Create with no folder picked asks the picker for a parent. */
function today(): FakeHost {
  let list: MatterSummary[] = [A];
  return {
    query: () => ({ matters: list }),
    dispatch: (command: CommandEnvelope): CommandOutcome => {
      if (!("create_matter" in command.command)) return "done";
      list = [A, { matter: MADE, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: MADE } };
    },
  };
}

/** A picker the test answers when it likes: its pick is held until then. */
function heldPicker() {
  let answer: (picked: { folder: string; display: string } | null) => void = () => {};
  let refuse: (error: Error) => void = () => {};
  const held = new Promise<{ folder: string; display: string } | null>((resolve, reject) => {
    answer = resolve;
    refuse = reject;
  });
  return { pick: () => held, answer, refuse };
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

/** Lets the work the last step started run: its promises, and a zero-delay timer. */
async function flush() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  await act(async () => settled());
}

/** Time passes, as the picker waits: the timers that fall due run inside act. */
async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

const byId = <E extends HTMLElement>(id: string) =>
  document.querySelector<E>(`[data-testid="${id}"]`);
const dialog = () => document.querySelector('[role="dialog"]');
const line = () => byId<HTMLElement>("picker-opening");

function type(input: HTMLInputElement, text: string) {
  Reflect.set(HTMLInputElement.prototype, "value", text, input);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

/** New matter, pressed, and the name typed. The dialog's chunk loads on the press: settled waits for it. */
async function openForm(name = NAME) {
  await act(async () => byId("new-matter")?.click());
  await flush();
  const field = byId<HTMLInputElement>("matter-name");
  await act(async () => type(field as HTMLInputElement, name));
}

/** A press, its work run to the point it waits. */
async function press(id: string) {
  await act(async () => byId(id)?.click());
  await flush();
}

const created = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((d) => d.command.command)
    .filter((c) => "create_matter" in c);

/** The line's life under one pick: none at 299 ms, the status at 300, gone once `end` resolves the pick. */
async function expectLineThrough(end: () => Promise<void>) {
  await advance(299);
  expect(line()).toBeNull();
  await advance(1);
  expect(line()?.getAttribute("role")).toBe("status");
  expect(line()?.textContent).toBe("Opening the folder picker…");
  await act(end);
  await flush();
  expect(line()).toBeNull();
}

test("Create waits for a folder: with none chosen it is held back, and no picker opens behind it", async () => {
  const picker = heldPicker();
  const transport = await mount({ ...today(), pickFolder: picker.pick });
  await openForm();
  expect(byId<HTMLButtonElement>("create-matter")?.disabled).toBe(true);
  await press("create-matter");
  await advance(500);
  expect(transport.only("pick_folder")).toEqual([]);
  expect(line()).toBeNull();
  expect(created(transport)).toEqual([]);
  expect(dialog()).not.toBeNull();
});

test("Choose folder…: the line shows from 300 ms, and goes when the pick is picked", async () => {
  const picker = heldPicker();
  await mount({ ...today(), pickFolder: picker.pick });
  await openForm();
  await press("matter-existing");
  await expectLineThrough(async () =>
    picker.answer({ folder: "ref-existing", display: "Documents/Mehta" }),
  );
  expect(dialog()?.textContent).toContain("Documents/Mehta");
});

test("Choose folder…: the line goes when the pick is cancelled", async () => {
  const picker = heldPicker();
  await mount({ ...today(), pickFolder: picker.pick });
  await openForm();
  await press("matter-existing");
  await expectLineThrough(async () => picker.answer(null));
  expect(dialog()?.textContent).not.toContain("Documents");
});

test("Choose folder…: the line goes when the picker fails", async () => {
  const picker = heldPicker();
  await mount({ ...today(), pickFolder: picker.pick });
  await openForm();
  await press("matter-existing");
  await expectLineThrough(async () => picker.refuse(new Error("picker failed")));
  expect(byId<HTMLButtonElement>("matter-existing")?.disabled).toBe(false);
});

test("Choose folder…: a pick answered inside 300 ms never shows the line, even once 300 ms have passed", async () => {
  const picker = heldPicker();
  await mount({ ...today(), pickFolder: picker.pick });
  await openForm();
  await press("matter-existing");
  await advance(120);
  await act(async () => picker.answer({ folder: "ref-existing", display: "Documents/Mehta" }));
  await flush();
  await advance(500);
  expect(line()).toBeNull();
  expect(dialog()?.textContent).toContain("Documents/Mehta");
});
