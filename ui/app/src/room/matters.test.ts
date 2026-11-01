// matters.ts over FakeTransport: the list with and without a NextHost, the matter to open, kept header subscriptions
// until H4, each row's state, pin, locate, remove, and the app events that update the list.
import { afterEach, expect, test, vi } from "vitest";
import type { Batch } from "../store/batch.ts";
import { createViewStore } from "../store/store.ts";
import type { MatterSummary, ThreadStatus, ViewEntry } from "../transport/protocol.ts";
import { FakeTransport, type FakeHost, type FakeNextHost } from "../transport/fake-transport.ts";
import { batch, event, reset, uuid } from "../testing/stream-script.ts";
import { DEFAULT_UI } from "./display-store.ts";
import type { MatterInfo, NextCommand } from "./host-next.ts";
import { Matters, rowOf } from "./matters.ts";

const SCRATCH: MatterSummary = { matter: uuid(9, 2), name: "Scratch", kind: "scratch" };
const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const B: MatterSummary = { matter: uuid(2, 2), name: "Sharma v. State of Delhi", kind: "folder" };
const T1 = uuid(1, 1);

const info = (m: MatterSummary, over: Partial<MatterInfo> = {}): MatterInfo => ({
  ...m,
  pinned: false,
  folder_state: "found",
  status: "idle",
  ...over,
});

let stops: (() => void)[] = [];
afterEach(() => {
  for (const stop of stops.splice(0)) stop();
});

/** A started Matters over a fake host, with a store whose frames run by hand. */
async function rig(host: FakeHost) {
  const transport = new FakeTransport(host);
  const pending: (() => void)[] = [];
  const manual: Batch = (flush) => {
    pending.push(flush);
    return () => {};
  };
  const store = createViewStore(transport, { batch: () => manual });
  const matters = new Matters(transport, store);
  stops.push(matters.start());
  const settle = async () => {
    for (let i = 0; i < 4; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
    pending.splice(0).forEach((f) => f());
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  await settle();
  return { transport, store, matters, settle };
}

const today = (matters: MatterSummary[]): FakeHost => ({
  query: (query) => {
    if (query === "list_matters") return { matters };
    throw new Error("not scripted");
  },
});

function nextHost(list: () => MatterInfo[], command?: FakeNextHost["command"]): FakeHost {
  return {
    next: {
      query: (query) => {
        if (query === "list_matters") return { matters: list() };
        if (query === "get_settings") return { settings: { ui: DEFAULT_UI, locked: [] } };
        if (query === "get_default_location")
          return { default_location: { display: "~/Documents/Clauseo" } };
        throw new Error(`not scripted: ${JSON.stringify(query)}`);
      },
      ...(command === undefined ? {} : { command }),
    },
  };
}

const headerEntry = (status: ThreadStatus, thread = T1, matter = A.matter): ViewEntry => ({
  header: {
    order: 1,
    thread,
    matter,
    agent: "claude",
    status,
    config_options: [],
    commands: [],
    plan: [],
  },
});

const permission: ViewEntry = {
  permission: {
    order: 2,
    request: uuid(1, 9),
    options: [{ option_id: "allow", kind: "allow_once", agent_label: "Allow" }],
    deliverable: "live",
    state: "pending",
  },
};

/** Plays a reset of a thread's view holding this header (and entries), at cursor `at`. */
function play(
  transport: FakeTransport,
  status: ThreadStatus,
  at: number,
  entries: ViewEntry[] = [],
) {
  transport.deliver(
    event(batch(at, [reset(entries, headerEntry(status))], uuid(1, 3), `thread:${T1}`)),
  );
}

const subscribed = (transport: FakeTransport) =>
  transport.only("subscribe").length - transport.only("unsubscribe").length;

test("without a NextHost: today's list_matters, the newest folder matter opens, the scratch matter never does", async () => {
  const { matters, transport } = await rig(today([A, SCRATCH, B]));
  const view = matters.view();
  expect(view.phase).toBe("ready");
  expect(view.detailed).toBe(false);
  expect(view.open?.matter).toBe(B.matter);
  expect(view.matters.every((m) => !m.pinned && m.folder_state === "found")).toBe(true);
  expect(transport.only("next_query")).toEqual([]);
  // the scratch matter, newest by id, still doesn't reopen
  const only = await rig(today([SCRATCH]));
  expect(only.matters.view().open).toBeNull();
});

test("a failed list says so, once", async () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const { matters } = await rig({
    query: () => {
      throw new Error("the host is gone");
    },
  });
  expect(matters.view().phase).toBe("failed");
  expect(warn).toHaveBeenCalledWith(expect.stringContaining("list_matters failed"));
  warn.mockRestore();
});

test("kept subscriptions: a matter left while its thread runs stays subscribed, header only, until it rests", async () => {
  const { matters, transport, settle } = await rig(today([A, B]));
  matters.open(A.matter);
  matters.onThread(T1);
  await settle();
  expect(subscribed(transport)).toBe(1);
  play(transport, "running", 1);
  await settle();
  expect(matters.view().status.get(A.matter)).toEqual({ state: "working" });
  matters.open(B.matter);
  await settle();
  // left while running: still subscribed, and the row still says so
  expect(transport.only("unsubscribe")).toEqual([]);
  expect(matters.view().status.get(A.matter)).toEqual({ state: "working" });
  // it rests: the subscription goes, and the row with it
  play(transport, "idle", 2);
  await settle();
  expect(transport.only("unsubscribe")).toEqual([{ call: "unsubscribe", stream: `thread:${T1}` }]);
  expect(matters.view().status.has(A.matter)).toBe(false);
});

test("kept subscriptions: a matter left while its thread is idle lets it go", async () => {
  const { matters, transport, settle } = await rig(today([A, B]));
  matters.open(A.matter);
  matters.onThread(T1);
  await settle();
  play(transport, "idle", 1);
  await settle();
  matters.open(B.matter);
  await settle();
  expect(transport.only("unsubscribe")).toEqual([{ call: "unsubscribe", stream: `thread:${T1}` }]);
});

test("without H5, needs_you is a permission when the thread holds a pending one, else a question", async () => {
  const { matters, transport, settle } = await rig(today([A, B]));
  matters.open(A.matter);
  matters.onThread(T1);
  await settle();
  play(transport, "needs_you", 1);
  await settle();
  expect(matters.view().status.get(A.matter)).toEqual({ state: "question" });
  play(transport, "needs_you", 2, [permission]);
  await settle();
  expect(matters.view().status.get(A.matter)).toEqual({ state: "permission" });
  play(transport, { paused_at_limit: { resets_at: 1_000 } }, 3);
  await settle();
  expect(matters.view().status.get(A.matter)).toEqual({ state: "limit", at: 1_000 });
  play(transport, { queued: { behind: uuid(2, 1) } }, 4);
  await settle();
  expect(matters.view().status.get(A.matter)).toEqual({ state: "working" });
});

test("H5's statuses as row states; a moved folder over any of them", () => {
  const row = (over: Partial<MatterInfo>) => rowOf(info(A, over));
  expect(row({})).toBeNull();
  expect(row({ status: "working" })).toEqual({ state: "working" });
  expect(row({ status: "needs_permission" })).toEqual({ state: "permission" });
  expect(row({ status: "needs_answer" })).toEqual({ state: "question" });
  expect(row({ status: "ready" })).toEqual({ state: "ready" });
  expect(row({ status: { stopped: { at: 5 } } })).toEqual({ state: "stopped", at: 5 });
  expect(row({ status: { paused_at_limit: { resets_at: 7 } } })).toEqual({ state: "limit", at: 7 });
  expect(row({ status: { paused_at_limit: {} } })).toEqual({ state: "limit" });
  expect(row({ status: "working", folder_state: "missing" })).toEqual({ state: "moved" });
});

test("with a NextHost: the last opened reopens, opening sends open_matter, the open thread is marked seen, and no header is kept", async () => {
  const list = [info(A, { last_opened: 10 }), info(B, { last_activity: 99 }), info(SCRATCH)];
  const { matters, transport, settle } = await rig(nextHost(() => list));
  expect(matters.view().detailed).toBe(true);
  expect(matters.view().open?.matter).toBe(A.matter);
  matters.open(B.matter);
  matters.onThread(T1);
  await settle();
  const sent = transport.only("next_command").map((c) => c.command);
  expect(sent).toEqual([{ open_matter: { matter: B.matter } }, { mark_seen: { thread: T1 } }]);
  expect(transport.only("subscribe")).toEqual([]);
  expect(transport.only("query")).toEqual([]);
});

test("matters_changed updates a row, and matters_removed takes it off", async () => {
  const { matters, transport } = await rig(nextHost(() => [info(A), info(B)]));
  expect(matters.view().status.has(A.matter)).toBe(false);
  transport.appEvent({
    matters_changed: {
      matters: [info(A, { status: "needs_answer", client: "Asha Rao" }), info(B)],
    },
  });
  expect(matters.view().status.get(A.matter)).toEqual({ state: "question" });
  expect(matters.view().matters.find((m) => m.matter === A.matter)?.client).toBe("Asha Rao");
  transport.appEvent({ matters_removed: { matters: [A.matter] } });
  expect(matters.view().matters.map((m) => m.matter)).toEqual([B.matter]);
});

test("pin, remove and locate send H5's and H3's commands; a cancelled locate sends nothing", async () => {
  let list = [info(A), info(B, { folder_state: "missing" })];
  const commands: NextCommand[] = [];
  const host = nextHost(
    () => list,
    (command) => {
      commands.push(command);
      if ("forget_matter" in command)
        list = list.filter((m) => m.matter !== command.forget_matter.matter);
    },
  );
  const { matters, transport } = await rig(host);
  expect(matters.view().open?.matter).toBe(B.matter);
  expect(matters.view().folderMissing).toBe(true);
  await matters.pin(A.matter, true);
  expect(commands.at(-1)).toEqual({ update_matter: { matter: A.matter, pinned: true } });
  await matters.locate(B.matter);
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(commands.some((c) => "relocate_matter" in c)).toBe(false);
  transport.host.pickFolder = () => ({ folder: "ref-1", display: "~/Documents/Sharma" });
  await matters.locate(B.matter);
  expect(commands.at(-1)).toEqual({ relocate_matter: { matter: B.matter, folder: "ref-1" } });
  // removing the open matter opens the next one
  await matters.forget(B.matter);
  expect(commands.at(-1)).toEqual({ forget_matter: { matter: B.matter } });
  expect(matters.view().matters.map((m) => m.matter)).toEqual([A.matter]);
  expect(matters.view().open?.matter).toBe(A.matter);
});

test("create, without a NextHost: create_matter with the source, then the list again and the new matter open", async () => {
  const made = uuid(3, 2);
  let list = [A];
  const { matters, transport } = await rig({
    ...today([]),
    query: () => ({ matters: list }),
    dispatch: (command) => {
      if (!("create_matter" in command.command)) throw new Error("not scripted");
      list = [A, { matter: made, name: command.command.create_matter.name, kind: "folder" }];
      return { matter_created: { matter: made } };
    },
  });
  await matters.create("New v. Old", { new_folder: { folder: "ref-2" } });
  expect(transport.only("dispatch")[0]?.command.command).toEqual({
    create_matter: { name: "New v. Old", source: { new_folder: { folder: "ref-2" } } },
  });
  expect(matters.view().open?.matter).toBe(made);
  await expect(matters.create("X", "default_location")).rejects.toThrow(/no default location/);
});
