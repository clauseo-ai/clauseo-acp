// The room's rail over FakeTransport, at the room's interface: the scratch matter only at the foot, each row's
// status words, date groups across a Sunday, finding by name only without a NextHost and by client with one, Pinned and
// Recent, Remove from the list, and matters_changed updating a row.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { MatterInfo, NextCommand } from "./host-next.ts";
import { Room } from "./room.tsx";
import { ScratchLine } from "./scratch-line.tsx";
import { connectRoom } from "./start.ts";
import { clockOf, localZone } from "./when.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.useRealTimers();
});

const SCRATCH: MatterSummary = { matter: uuid(9, 2), name: "Scratch", kind: "scratch" };
const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const B: MatterSummary = { matter: uuid(2, 2), name: "Sharma v. State of Delhi", kind: "folder" };
const C: MatterSummary = { matter: uuid(3, 2), name: "Rao v. Union of India", kind: "folder" };

const today = (matters: MatterSummary[]): FakeHost => ({
  query: (query) => {
    if (query === "list_matters") return { matters };
    throw new Error("not scripted");
  },
});

const withNext = (matters: () => MatterInfo[], command?: (c: NextCommand) => void): FakeHost => ({
  next: fakeNext({ matters, ...(command === undefined ? {} : { command }) }),
});

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

const rail = () => host?.querySelector("nav.room-rail") ?? null;
const rows = () => [...(rail()?.querySelectorAll<HTMLElement>('[data-testid="matter-row"]') ?? [])];
const names = () => rows().map((r) => r.querySelector(".room-matter-name")?.textContent);
const rowOf = (name: string) =>
  rows().find((r) => r.querySelector(".room-matter-name")?.textContent === name);
const headings = () =>
  [...(rail()?.querySelectorAll(".room-when") ?? [])].map((h) => h.textContent);
const title = () => host?.querySelector('[data-testid="matter"]')?.textContent;
/** The status line's words, with the clock's held spaces as spaces. */
const said = (name: string) => rowOf(name)?.textContent?.replace(/\u00a0/g, " ") ?? "";

/** Types into a field as the lawyer would, so React hears it. */
function type(input: HTMLInputElement, text: string) {
  Reflect.set(HTMLInputElement.prototype, "value", text, input);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

const find = () => rail()?.querySelector<HTMLInputElement>('input[type="search"]') ?? null;

test("the scratch matter is never a row of the list: only the foot's row opens it, titled Scratch matter", async () => {
  await mount(today([A, SCRATCH, B]));
  expect(names()).toEqual([B.name, A.name]);
  const foot = rail()?.querySelector<HTMLButtonElement>('[data-kind="scratch"]');
  expect(foot).not.toBeNull();
  // the foot comes after every group
  const groups = rail()?.querySelector(".room-groups");
  expect(groups?.compareDocumentPosition(foot as Node)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  expect(groups?.textContent).not.toContain("Scratch");
  await act(async () => foot?.click());
  expect(title()).toBe("Scratch matter");
  expect(foot?.getAttribute("aria-current")).toBe("page");
});

test("with a NextHost, a scratch matter with a date and a pin still stands only at the foot", async () => {
  const next = new Date(Date.now() + 86_400_000).toISOString().slice(0, 10);
  await mount(withNext(() => [detailed(SCRATCH, { pinned: true, next_date: next }), detailed(A)]));
  expect(names()).toEqual([A.name]);
  expect(headings()).not.toContain("Pinned");
});

test("each status says its words; a moved folder says so over its status", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  const now = new Date(2026, 10, 2, 11, 0).getTime();
  vi.setSystemTime(now);
  const at = new Date(2026, 10, 2, 16, 31).getTime();
  const later = new Date(2026, 10, 4, 9, 0).getTime();
  const zone = localZone();
  const time = clockOf(at, now, zone, "en").time;
  const other = clockOf(later, now, zone, "en");
  const m = (n: number, status: MatterInfo["status"], over: Partial<MatterInfo> = {}) =>
    detailed({ matter: uuid(n, 2), name: `Matter ${n}`, kind: "folder" }, { status, ...over });
  await mount(
    withNext(() => [
      m(1, "working"),
      m(2, "needs_answer"),
      m(3, "needs_permission"),
      m(4, "ready"),
      m(5, { stopped: { at } }),
      m(6, { paused_at_limit: { resets_at: at } }),
      m(7, { paused_at_limit: { resets_at: later } }),
      m(8, { paused_at_limit: {} }),
      m(9, "working", { folder_state: "missing" }),
      m(10, "idle"),
    ]),
  );
  const words: [number, string, string][] = [
    [1, "working", "Answering."],
    [2, "question", "A question for you"],
    [3, "permission", "Needs your permission"],
    [4, "ready", "Answer ready"],
    [5, "stopped", `Stopped at ${time}`],
    [6, "limit", `Limit reached until ${time}`],
    [7, "limit", `Limit reached until ${other.day} ${other.time}`],
    [8, "limit", "Limit reached. Reset not given."],
    [9, "moved", "Folder moved"],
  ];
  for (const [n, state, line] of words) {
    expect(rowOf(`Matter ${n}`)?.dataset["state"], `Matter ${n}`).toBe(state);
    expect(said(`Matter ${n}`), `Matter ${n}`).toContain(line);
  }
  // a question is never a permission, and working never names an end time
  expect(said("Matter 2")).not.toContain("permission");
  expect(said("Matter 1")).not.toContain("Expected");
  expect(rowOf("Matter 10")?.dataset["state"]).toBe("idle");
  expect(said("Matter 10")).toBe("Matter 10");
});

test("date groups across a Sunday, in the rail's order, with no empty group", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 1, 12, 0)); // Sunday 1 November 2026
  const m = (n: number, next_date?: string) =>
    detailed(
      { matter: uuid(n, 2), name: `Matter ${n}`, kind: "folder" },
      next_date === undefined ? {} : { next_date },
    );
  await mount(
    withNext(() => [
      m(1, "2026-11-03"),
      m(2, "2026-11-02"),
      m(3, "2026-11-01"),
      m(4),
      m(5, "2026-11-30"),
    ]),
  );
  expect(headings()).toEqual(["Tomorrow", "This week", "Next week", "Later", "No date yet"]);
  expect(names()).toEqual(["Matter 2", "Matter 3", "Matter 1", "Matter 5", "Matter 4"]);
});

test("without a NextHost: one group, No date yet, newest first; no Pinned, and find matches names only", async () => {
  await mount(today([A, B, C]));
  expect(headings()).toEqual(["No date yet"]);
  expect(names()).toEqual([C.name, B.name, A.name]);
  // focused and empty: no Recent, the groups stay
  await act(async () => find()?.focus());
  expect(headings()).toEqual(["No date yet"]);
  await act(async () => type(find() as HTMLInputElement, "sharma"));
  expect(names()).toEqual([B.name]);
  await act(async () => type(find() as HTMLInputElement, "Asha"));
  expect(names()).toEqual([]);
  expect(rail()?.textContent).toContain("No matter matches 'Asha'.");
});

test("with a NextHost: find by client, Pinned above the dates, and Recent by last opened", async () => {
  await mount(
    withNext(() => [
      detailed(A, { client: "Asha Rao", last_opened: 30 }),
      detailed(B, { pinned: true, last_opened: 10 }),
      detailed(C, { last_opened: 20 }),
    ]),
  );
  expect(headings()).toEqual(["Pinned", "No date yet"]);
  // pinned above every date; the rest newest first
  expect(names()).toEqual([B.name, C.name, A.name]);
  await act(async () => find()?.focus());
  expect(headings()).toEqual(["Recent"]);
  expect(names()).toEqual([A.name, C.name, B.name]);
  await act(async () => type(find() as HTMLInputElement, "asha"));
  expect(names()).toEqual([A.name]);
  expect(rowOf(A.name)?.textContent).toContain("Client, Asha Rao");
});

test("Remove from the list sends forget_matter and the row leaves; without a NextHost the row has no menu", async () => {
  let list = [detailed(A), detailed(B)];
  const sent: NextCommand[] = [];
  await mount(
    withNext(
      () => list,
      (c) => {
        sent.push(c);
        if ("forget_matter" in c) list = list.filter((m) => m.matter !== c.forget_matter.matter);
      },
    ),
  );
  const row = rowOf(A.name);
  await act(async () =>
    row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  await settle();
  const remove = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((i) =>
    i.textContent?.includes("Remove from the list"),
  );
  expect(remove).toBeDefined();
  await act(async () => remove?.click());
  await settle();
  expect(sent).toContainEqual({ forget_matter: { matter: A.matter } });
  expect(names()).toEqual([B.name]);
  await act(async () => root?.unmount());
  host?.remove();
  await mount(today([A]));
  await act(async () =>
    rowOf(A.name)?.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    ),
  );
  await settle();
  expect(document.querySelector('[role="menu"]')).toBeNull();
});

test("matters_changed updates a row in place", async () => {
  const transport = await mount(withNext(() => [detailed(A), detailed(B)]));
  expect(rowOf(A.name)?.dataset["state"]).toBe("idle");
  await act(async () =>
    transport.appEvent({
      matters_changed: { matters: [detailed(A, { status: "needs_answer" }), detailed(B)] },
    }),
  );
  expect(rowOf(A.name)?.dataset["state"]).toBe("question");
  expect(said(A.name)).toContain("A question for you");
});

test("pressing a row opens its matter: the title and the current row move, and with H5 the host hears it", async () => {
  const transport = await mount(withNext(() => [detailed(A), detailed(B)]));
  expect(title()).toBe(B.name);
  await act(async () => rowOf(A.name)?.click());
  expect(title()).toBe(A.name);
  expect(rowOf(A.name)?.getAttribute("aria-current")).toBe("page");
  expect(rowOf(B.name)?.getAttribute("aria-current")).toBeNull();
  expect(transport.only("next_command").map((c) => c.command)).toContainEqual({
    open_matter: { matter: A.matter },
  });
});

// QU.7 (a): before stage C the scratch matter's line promises no Indian Kanoon, no statutes and nothing about other
// matters' papers; "kept, with its versions" is true on main
for (const language of ["en", "hi"] as const) {
  test(`the scratch matter's line promises only what holds before stage C (${language})`, async () => {
    connectRoom(new FakeTransport(), { language });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root?.render(<ScratchLine />));
    const text = host.textContent ?? "";
    expect(text).not.toBe("");
    for (const held of ["Indian Kanoon", "statute", "क़ानून", "another matter", "दूसरे मामले"])
      expect(text).not.toContain(held);
  });
}
