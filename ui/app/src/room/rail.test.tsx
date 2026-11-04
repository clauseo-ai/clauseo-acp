// The room's rail over FakeTransport, at the room's interface: the scratch matter only at the foot, each row's
// status words (none on the open matter's), date groups across a Sunday, finding by name only without a NextHost and
// by client with one, each saying so, Pinned and Recent (five), no Find on an empty rail, Remove from the list, Show in
// Finder and the open matter's chevron, isolated folder paths, matters_changed updating a row, and a left matter's thread waiting on a
// permission, and U12's paused rail.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { MatterSummary } from "../transport/protocol.ts";
import { settled } from "@clauseo/components/lib/late.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import { header, reset, upsert, uuid } from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import { mountRoom, summary, THE_MATTER } from "./thread/harness.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { MatterInfo, NextCommand } from "./host-next.ts";
import { Room } from "./room.tsx";
import { ScratchLine } from "./scratch-line.tsx";
import { connectRoom } from "./start.ts";
import { clockOf, localZone } from "./when.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

// a fixed instant (Tuesday 3 November 2026); a test that needs another day sets its own
const NOW = Date.UTC(2026, 10, 3, 17, 0);
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});

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
  // Find's results come in their own chunk
  await act(async () => settled());
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

const find = () => rail()?.querySelector<HTMLInputElement>('[data-testid="find"]') ?? null;
/** Find's label, read by a screen reader. */
const findLabel = () => find()?.closest("label")?.querySelector(".sr-only")?.textContent;

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
  const tomorrow = new Date(NOW + 86_400_000);
  const pad = (n: number) => String(n).padStart(2, "0");
  // tomorrow on the computer's own calendar: the app's day is local, not UTC
  const next = `${tomorrow.getFullYear()}-${pad(tomorrow.getMonth() + 1)}-${pad(tomorrow.getDate())}`;
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
  await settle();
  expect(names()).toEqual([B.name]);
  // this host answers no search: nothing new shows, and the names still match
  await act(async () => type(find() as HTMLInputElement, "Asha"));
  await settle();
  expect(names()).toEqual([]);
  expect(findLabel()).toBe("Find");
  expect(rail()?.textContent).not.toContain("client");
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
  await settle();
  expect(names()).toEqual([A.name]);
  expect(rowOf(A.name)?.textContent).toContain("Client, Asha Rao");
  expect(findLabel()).toBe("Find");
});

test("Recent shows the five matters opened last", async () => {
  const list = [1, 2, 3, 4, 5, 6, 7].map((n) =>
    detailed({ matter: uuid(n, 2), name: `Matter ${n}`, kind: "folder" }, { last_opened: n * 10 }),
  );
  await mount(withNext(() => list));
  await act(async () => find()?.focus());
  expect(headings()).toEqual(["Recent"]);
  expect(names()).toEqual(["Matter 7", "Matter 6", "Matter 5", "Matter 4", "Matter 3"]);
});

test("the empty rail has no Find: nothing to find", async () => {
  await mount(today([SCRATCH]));
  expect(rail()?.textContent).toContain("No matters yet");
  expect(find()).toBeNull();
});

test("the open matter's row says nothing of its work, which is in view; the others say theirs", async () => {
  await mount(
    withNext(() => [
      detailed(A, { status: "working", last_opened: 10 }),
      detailed(B, { status: "working" }),
    ]),
  );
  expect(title()).toBe(A.name);
  expect(rowOf(A.name)?.dataset["state"]).toBe("idle");
  expect(said(A.name)).not.toContain("Answering.");
  expect(rowOf(B.name)?.dataset["state"]).toBe("working");
});

test("the open matter with a missing folder says nothing, yet its chevron still offers Locate folder", async () => {
  await mount(
    withNext(() => [
      detailed(A, { folder_state: "missing", status: "working", last_opened: 10 }),
      detailed(B),
    ]),
  );
  expect(title()).toBe(A.name);
  expect(rowOf(A.name)?.dataset["state"]).toBe("idle");
  expect(said(A.name)).not.toContain("Folder moved");
  const more = rail()?.querySelector<HTMLButtonElement>(".room-house-more");
  await act(async () => more?.click());
  await settle();
  const items = [...document.querySelectorAll('[role="menuitem"]')].map((i) => i.textContent);
  expect(items).toContainEqual(expect.stringContaining("Locate folder"));
});

test("a stop on another day says its day and its time", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  const now = new Date(2026, 10, 4, 11, 0).getTime();
  vi.setSystemTime(now);
  const at = new Date(2026, 10, 2, 16, 31).getTime();
  const { day, time } = clockOf(at, now, localZone(), "en");
  expect(day).not.toBe("");
  await mount(
    withNext(() => [detailed(A, { last_opened: 1 }), detailed(B, { status: { stopped: { at } } })]),
  );
  expect(said(B.name)).toContain(`Stopped on ${day} at ${time}`);
});

test("a row's folder, under the pointer, is isolated: a right-to-left override can't reorder it", async () => {
  const spoof = "Documents/a\u202Efdp.exe";
  await mount(withNext(() => [detailed(A, { folder_display: spoof })]));
  expect(rowOf(A.name)?.getAttribute("title")).toBe(`\u2068${spoof}\u2069`);
});

test("with H5 the open matter's row carries its actions' chevron, reached by keyboard", async () => {
  await mount(withNext(() => [detailed(A, { last_opened: 10 }), detailed(B)]));
  const more = rail()?.querySelector<HTMLButtonElement>(".room-house-more");
  expect(more?.getAttribute("aria-label")).toBe(`Actions for ${A.name}`);
  await act(async () => more?.click());
  await settle();
  const items = [...document.querySelectorAll('[role="menuitem"], [role="menuitemcheckbox"]')];
  expect(items.map((i) => i.textContent)).toEqual(
    expect.arrayContaining([
      expect.stringContaining("Pin to the top"),
      expect.stringContaining("Remove from the list"),
    ]),
  );
});

test("Remove from the list sends forget_matter and the row leaves", async () => {
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
});

/** Opens a row's context menu and presses its one item, which without a NextHost is Show in Finder. */
async function showInFinder(name: string) {
  await act(async () =>
    rowOf(name)?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  await settle();
  const items = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
  expect(items.map((i) => i.textContent)).toEqual(["Show in Finder"]);
  await act(async () => items[0]?.click());
  await settle();
}

test("without a NextHost a row's menu is Show in Finder alone, and it reveals the matter's folder", async () => {
  const transport = await mount({ ...today([A, B]), platform: "mac" });
  await showInFinder(A.name);
  expect(transport.only("reveal")).toEqual([{ call: "reveal", target: { matter: A.matter } }]);
});

test("a refused Show in Finder only goes to the console; the rail stays as it was", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const transport = await mount({
    ...today([A, B]),
    platform: "mac",
    reveal: () => {
      throw { code: "matter.folder_missing", message: "gone", retryable: false };
    },
  });
  const before = names();
  await showInFinder(A.name);
  expect(transport.only("reveal")).toEqual([{ call: "reveal", target: { matter: A.matter } }]);
  expect(warn).toHaveBeenCalledWith(expect.stringContaining("reveal failed"));
  expect(names()).toEqual(before);
  warn.mockRestore();
});

test("U12's paused rail: the at-limit words muted, never yellow or red; a new view formats against its own today", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  // Monday 2 November 2026, 11 pm: the reset is tomorrow's, at 9 am
  vi.setSystemTime(new Date(2026, 10, 2, 23, 0));
  const resets = new Date(2026, 10, 3, 9, 0).getTime();
  const paused = (over: Partial<MatterInfo> = {}) => [
    detailed(A, { status: { paused_at_limit: { resets_at: resets } }, ...over }),
    detailed(B),
  ];
  const transport = await mount(withNext(() => paused()));
  const row = () => rowOf(A.name);
  expect(row()?.dataset["state"]).toBe("limit");
  expect(said(A.name)).toContain("Limit reached until Tuesday 9:00 am");
  const line = row()?.querySelector(".text-small");
  expect(line?.classList.contains("text-muted")).toBe(true);
  // the plan's limit's own icon, in the line's muted tone, with no yellow disc: no choice is asked for
  expect(line?.querySelector(".room-mark")?.hasAttribute("data-waits")).toBe(false);
  expect(row()?.outerHTML).not.toMatch(/yellow|red|danger|destructive/);
  // past midnight, the next view reads today again: the same reset is now today's, with no day
  vi.setSystemTime(new Date(2026, 10, 3, 0, 30));
  await act(async () =>
    transport.appEvent({
      matters_changed: { matters: paused({ pinned: true }), removed: [], all: true },
    }),
  );
  expect(said(A.name)).toContain("Limit reached until 9:00 am");
  expect(said(A.name)).not.toContain("Tuesday");
});

test("matters_changed updates a row in place", async () => {
  const transport = await mount(withNext(() => [detailed(A), detailed(B)]));
  expect(rowOf(A.name)?.dataset["state"]).toBe("idle");
  await act(async () =>
    transport.appEvent({
      matters_changed: {
        matters: [detailed(A, { status: "needs_answer" })],
        removed: [],
        all: false,
      },
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

test("a matter left while its thread waits on a permission says Needs your permission, until the entry resolves", async () => {
  // older than THE_MATTER, so THE_MATTER opens first, with the scripted thread; this one has none
  const other: MatterSummary = {
    matter: uuid(0, 2),
    name: "Rao v. Union of India",
    kind: "folder",
  };
  const h = await mountRoom({
    matters: [THE_MATTER, other],
    threads: (m) => (m === THE_MATTER.matter ? [summary()] : []),
  });
  try {
    const row = (name: string) =>
      h.all("matter-row").find((r) => r.querySelector(".room-matter-name")?.textContent === name);
    const perm = (state: "pending" | "resolved"): ViewEntry => ({
      permission: {
        order: 3,
        request: uuid(1, 8),
        options: [{ option_id: "once", kind: "allow_once", agent_label: "Allow" }],
        deliverable: "live",
        state:
          state === "pending"
            ? "pending"
            : {
                resolved: {
                  choice: { selected: { option_id: "once", kind: "allow_once" } },
                  by: "local_desktop",
                },
              },
      },
    });
    h.deliver([reset([perm("pending")], header("needs_you"))]);
    expect(h.one("permission")?.dataset["state"]).toBe("pending");
    // the open matter's row says nothing: its prompt is in view
    expect(row(THE_MATTER.name)?.dataset["state"]).toBe("idle");
    await h.click(row(other.name));
    expect(h.one("matter")?.textContent).toBe(other.name);
    expect(row(THE_MATTER.name)?.dataset["state"]).toBe("permission");
    expect(row(THE_MATTER.name)?.textContent).toContain("Needs your permission");
    h.deliver([upsert(perm("resolved")), upsert(header("running"))]);
    expect(row(THE_MATTER.name)?.dataset["state"]).toBe("working");
    expect(row(THE_MATTER.name)?.textContent).not.toContain("permission");
  } finally {
    await h.unmount();
  }
});
