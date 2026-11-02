// conversations.ts over FakeTransport, with and without a NextHost: the rows and the one that opens, the draft New
// conversation makes, create_thread on the first send and the rename after it, each row's words, the headers and H4's
// events that keep the rows current, H6's seen marks, and following the open matter.
import { afterEach, expect, test, vi } from "vitest";
import type { Batch } from "../store/batch.ts";
import { createViewStore } from "../store/store.ts";
import { batch, event, reset, uuid } from "../testing/stream-script.ts";
import { FakeTransport, type FakeHost, type FakeNextHost } from "../transport/fake-transport.ts";
import type {
  AgentId,
  CommandOutcome,
  HostCommand,
  MatterId,
  MatterSummary,
  ThreadId,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { DEFAULT_AGENT } from "./agent.ts";
import { titleOf } from "./conversation-title.ts";
import { Conversations, rowWords, type ConversationRow } from "./conversations.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { ThreadInfo } from "./host-next.ts";
import { Matters } from "./matters.ts";

const A: MatterSummary = { matter: uuid(1, 2), name: "Acme Ltd v. State", kind: "folder" };
const B: MatterSummary = { matter: uuid(2, 2), name: "Sharma v. State of Delhi", kind: "folder" };
const T1 = uuid(1, 1);
const T2 = uuid(2, 1);
const T3 = uuid(3, 1);
const MADE = uuid(9, 1);

const thread = (
  id: ThreadId,
  over: Partial<ThreadSummary> = {},
  matter: MatterId = A.matter,
): ThreadSummary => ({ thread: id, matter, agent: "claude", status: "idle", ...over });

const info = (
  id: ThreadId,
  over: Partial<ThreadInfo> = {},
  matter: MatterId = A.matter,
): ThreadInfo => ({ ...thread(id, {}, matter), seen: true, ...over });

let stops: (() => void)[] = [];
afterEach(() => {
  for (const stop of stops.splice(0)) stop();
  vi.restoreAllMocks();
});

/** A started Matters and Conversations over one fake host, with a store whose frames run by hand. */
async function rig(host: FakeHost, focused: () => boolean = () => false) {
  const transport = new FakeTransport(host);
  const pending: (() => void)[] = [];
  const manual: Batch = (flush) => {
    pending.push(flush);
    return () => {};
  };
  const store = createViewStore(transport, { batch: () => manual });
  const matters = new Matters(transport, store, manual);
  stops.push(matters.start());
  const drafts = new Map<string, string>();
  const talks = new Conversations(transport, store, matters, drafts, focused);
  stops.push(talks.start());
  const settle = async () => {
    for (let i = 0; i < 4; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
    pending.splice(0).forEach((f) => f());
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  await settle();
  /** The commands dispatched, by kind. */
  const sent = <K extends keyof HostCommand>(kind: K) =>
    transport
      .only("dispatch")
      .map((c) => c.command.command[kind])
      .filter((c) => c !== undefined) as NonNullable<HostCommand[K]>[];
  const listReads = () =>
    transport.only("query").filter((c) => typeof c.query === "object" && "list_threads" in c.query)
      .length +
    transport
      .only("next_query")
      .filter((c) => typeof c.query === "object" && "list_threads" in c.query).length;
  const marks = () =>
    transport
      .only("next_command")
      .map((c) => c.command)
      .filter((c) => "mark_seen" in c);
  return { transport, store, matters, talks, drafts, settle, sent, listReads, marks };
}

/** Today's host: list_matters, list_threads per matter, create_thread making MADE, every other command done. */
function today(
  threads: (matter: MatterId) => ThreadSummary[] | Promise<ThreadSummary[]>,
  matters: MatterSummary[] = [A],
  dispatch: (command: HostCommand) => CommandOutcome = (command) =>
    command.create_thread !== undefined ? { thread_created: { thread: MADE } } : "done",
): FakeHost {
  return {
    query: async (query) => {
      if (query === "list_matters") return { matters };
      if (typeof query === "object" && query.list_threads !== undefined)
        return { threads: await threads(query.list_threads.matter) };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope) => dispatch(envelope.command),
  };
}

/** A NextHost with the matters, app and agents parts: A, opened last, with these threads. */
function next(threads: (matter: MatterId) => ThreadInfo[], matters = [detailed(A)]): FakeHost {
  return {
    next: fakeNext({ matters: () => matters, threads }),
    dispatch: (envelope) =>
      envelope.command.create_thread !== undefined ? { thread_created: { thread: MADE } } : "done",
  };
}

const headerOf = (
  id: ThreadId,
  status: ThreadStatus,
  over: { title?: string; agent?: AgentId } = {},
): ViewEntry => ({
  header: {
    order: 1,
    thread: id,
    matter: A.matter,
    agent: over.agent ?? "claude",
    status,
    ...(over.title === undefined ? {} : { title: over.title }),
    config_options: [],
    commands: [],
    plan: [],
  },
});

/** Plays a reset of a thread's view holding only this header, at cursor `at`. */
function play(
  transport: FakeTransport,
  id: ThreadId,
  status: ThreadStatus,
  at: number,
  over: { title?: string; agent?: AgentId } = {},
) {
  transport.deliver(
    event(batch(at, [reset([], headerOf(id, status, over))], uuid(1, 3), `thread:${id}`)),
  );
}

const threadsOf = (rows: readonly ConversationRow[]) => rows.map((r) => r.thread);

/* ---------- the list ---------- */

test("archived threads are hidden, rows newest first by id, and the newest not archived opens", async () => {
  const { talks } = await rig(
    today(() => [thread(T1), thread(T3, { status: "archived" }), thread(T2)]),
  );
  const view = talks.view();
  expect(view.phase).toBe("ready");
  expect(threadsOf(view.rows)).toEqual([T2, T1]);
  expect(view.thread).toBe(T2);
  expect(view.drafting).toBe(false);
});

test("with H6's last_activity the rows go by it, newest activity first", async () => {
  const { talks } = await rig(
    next(() => [
      info(T1, { last_activity: 300 }),
      info(T2, { last_activity: 100 }),
      info(T3, { last_activity: 200 }),
    ]),
  );
  expect(threadsOf(talks.view().rows)).toEqual([T1, T3, T2]);
});

test("a matter with no threads opens a draft, with no row shown", async () => {
  const { talks, sent } = await rig(today(() => []));
  const view = talks.view();
  expect(view.drafting).toBe(true);
  expect(view.rows).toEqual([]);
  expect(view.thread).toBeNull();
  expect(view.open).not.toBeNull();
  expect(sent("create_thread")).toEqual([]);
});

/* ---------- New conversation ---------- */

test("startNew: a draft row first, untitled and with no agent; nothing is dispatched", async () => {
  const { talks, transport } = await rig(today(() => [thread(T1)]));
  const before = transport.calls.length;
  talks.startNew();
  const view = talks.view();
  const [draft, ...rest] = view.rows;
  expect(draft).toEqual({ key: view.open, thread: null, agent: null, status: "idle" });
  expect(draft && "title" in draft).toBe(false);
  expect(threadsOf(rest)).toEqual([T1]);
  expect(view.drafting).toBe(true);
  expect(view.thread).toBeNull();
  expect(transport.calls.slice(before)).toEqual([]);
});

test("leaving a draft with nothing typed drops it; with words it stays", async () => {
  const { talks, drafts } = await rig(today(() => [thread(T1)]));
  const t1 = talks.view().open as string;
  talks.startNew();
  talks.open(t1);
  expect(threadsOf(talks.view().rows)).toEqual([T1]);
  talks.startNew();
  const draft = talks.view().open as string;
  drafts.set(draft, "  ");
  talks.open(t1);
  expect(threadsOf(talks.view().rows)).toEqual([T1]);
  talks.startNew();
  const kept = talks.view().open as string;
  drafts.set(kept, "What does clause nine say?");
  talks.open(t1);
  expect(talks.view().rows.map((r) => r.key)).toEqual([kept, t1]);
  // and opening it again finds the same draft
  talks.open(kept);
  expect(talks.view().drafting).toBe(true);
  expect(talks.view().open).toBe(kept);
});

/* ---------- create ---------- */

test("create with the draft open: create_thread once, and the thread keeps the draft's key", async () => {
  const { talks, sent } = await rig(today(() => [thread(T1)]));
  talks.startNew();
  const key = talks.view().open;
  const made = await talks.create();
  expect(made).toBe(MADE);
  expect(sent("create_thread")).toEqual([{ matter: A.matter, agent: "claude" }]);
  const view = talks.view();
  expect(view.open).toBe(key);
  expect(view.thread).toBe(MADE);
  expect(view.drafting).toBe(false);
  expect(view.rows[0]).toMatchObject({ key, thread: MADE, agent: "claude" });
  expect(view.rows).toHaveLength(2);
  // the open conversation now has its thread: create doesn't make another
  expect(await talks.create()).toBe(MADE);
  expect(sent("create_thread")).toHaveLength(1);
});

test("a matter with no threads: the draft it opens keeps its key once create makes the thread", async () => {
  const { talks } = await rig(today(() => []));
  const key = talks.view().open;
  await talks.create();
  expect(talks.view().open).toBe(key);
  expect(talks.view().thread).toBe(MADE);
  expect(talks.view().rows.map((r) => r.key)).toEqual([key]);
});

test("create while the list loads waits for it, and returns the thread it reopens instead of making one", async () => {
  let answer: (threads: ThreadSummary[]) => void = () => {};
  const held = new Promise<ThreadSummary[]>((resolve) => (answer = resolve));
  const { talks, sent, settle } = await rig(today(() => held));
  expect(talks.view().phase).toBe("loading");
  const key = talks.view().open;
  const made = talks.create();
  answer([thread(T1)]);
  expect(await made).toBe(T1);
  await settle();
  expect(sent("create_thread")).toEqual([]);
  expect(talks.view().open).toBe(key);
  expect(talks.view().thread).toBe(T1);
});

test("a failed list is read again by create, which then makes the thread", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  let reads = 0;
  const { talks, sent, listReads } = await rig(
    today(() => {
      reads += 1;
      if (reads === 1) throw new Error("the host is busy");
      return [];
    }),
  );
  expect(talks.view().phase).toBe("failed");
  expect(talks.view().refused).not.toBeNull();
  expect(await talks.create()).toBe(MADE);
  expect(listReads()).toBe(2);
  expect(sent("create_thread")).toHaveLength(1);
  expect(talks.view().phase).toBe("ready");
  expect(talks.view().refused).toBeNull();
});

test("a list that fails again: create rejects and makes nothing", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const { talks, sent } = await rig(
    today(() => {
      throw new Error("the host is busy");
    }),
  );
  await expect(talks.create()).rejects.toBeDefined();
  expect(sent("create_thread")).toEqual([]);
});

test("the agent create_thread names: the plan chosen, else the newest thread's, else DEFAULT_AGENT", async () => {
  // nothing to go by
  const none = await rig(today(() => []));
  await none.talks.create();
  expect(none.sent("create_thread")).toEqual([{ matter: A.matter, agent: DEFAULT_AGENT }]);
  // the newest thread's, archived or not
  const newest = await rig(
    today(() => [thread(T1), thread(T2, { agent: "codex", status: "archived" })]),
  );
  newest.talks.startNew();
  expect(newest.talks.view().agent).toBe("codex");
  await newest.talks.create();
  expect(newest.sent("create_thread")).toEqual([{ matter: A.matter, agent: "codex" }]);
  // the lawyer's choice over both
  const chosen = await rig(today(() => [thread(T1, { agent: "codex" })]));
  chosen.talks.startNew();
  chosen.talks.choose("claude-acp");
  expect(chosen.talks.view().agent).toBe("claude-acp");
  await chosen.talks.create();
  expect(chosen.sent("create_thread")).toEqual([{ matter: A.matter, agent: "claude-acp" }]);
});

/* ---------- the title after the first send ---------- */

test("sent: rename_thread with the title the message gives, once per thread", async () => {
  const { talks, sent, settle } = await rig(today(() => []));
  const made = await talks.create();
  const text = "What does   clause nine of the lease say about notice?";
  talks.sent(made, text);
  talks.sent(made, "And clause ten?");
  await settle();
  expect(sent("rename_thread")).toEqual([{ thread: made, title: titleOf(text) }]);
  expect(talks.view().rows[0]?.title).toBe(titleOf(text));
});

test("sent: no rename for a thread the list already titles", async () => {
  const { talks, sent, settle } = await rig(today(() => [thread(T1, { title: "Notice" })]));
  talks.sent(T1, "What does clause nine say?");
  await settle();
  expect(sent("rename_thread")).toEqual([]);
});

test("sent: no rename when the header already has a title (an agent's rename stands)", async () => {
  const { talks, store, transport, sent, settle } = await rig(today(() => [thread(T1)]));
  stops.push(store.watch(`thread:${T1}`));
  play(transport, T1, "running", 1, { title: "Indemnity" });
  await settle();
  talks.sent(T1, "What does clause nine say?");
  await settle();
  expect(sent("rename_thread")).toEqual([]);
});

test("sent: nothing when the message gives no title", async () => {
  const { talks, sent, settle } = await rig(today(() => []));
  const made = await talks.create();
  talks.sent(made, "\u200b\u200b\u200b");
  await settle();
  expect(sent("rename_thread")).toEqual([]);
});

/* ---------- a row's words ---------- */

test("rowWords for every status, and H6's marks over idle", () => {
  const words = (status: ThreadStatus, over: Partial<ConversationRow> = {}) =>
    rowWords({ status, ...over });
  expect(words("starting")).toEqual({ state: "working" });
  expect(words("running")).toEqual({ state: "working" });
  expect(words("needs_you")).toEqual({ state: "question" });
  expect(words({ queued: { behind: T2 } })).toEqual({ state: "waiting", behind: T2 });
  expect(words("idle")).toEqual({ state: "idle" });
  expect(words({ paused_at_limit: { resets_at: 5 } })).toEqual({ state: "idle" });
  expect(words("agent_exited")).toEqual({ state: "idle" });
  expect(words("idle_stopped")).toEqual({ state: "idle" });
  const unseen = (reason: NonNullable<ConversationRow["last_stop"]>["reason"]) =>
    words("idle", { seen: false, last_stop: { reason, at: 42 } });
  expect(unseen("end_turn")).toEqual({ state: "ready" });
  expect(unseen("cancelled")).toEqual({ state: "stopped", at: 42 });
  expect(unseen("failed")).toEqual({ state: "idle" });
  expect(words("idle", { seen: true, last_stop: { reason: "end_turn", at: 42 } })).toEqual({
    state: "idle",
  });
  expect(words("idle", { seen: true, last_stop: { reason: "cancelled", at: 42 } })).toEqual({
    state: "idle",
  });
});

/* ---------- live headers ---------- */

test("a listed thread's header updates its row's title and status", async () => {
  const { talks, store, transport, settle } = await rig(today(() => [thread(T1), thread(T2)]));
  stops.push(store.watch(`thread:${T1}`));
  play(transport, T1, "running", 1, { title: "Clause nine" });
  await settle();
  expect(talks.view().rows.find((r) => r.thread === T1)).toMatchObject({
    title: "Clause nine",
    status: "running",
  });
  play(transport, T1, "needs_you", 2, { title: "Clause nine" });
  await settle();
  expect(talks.view().rows.find((r) => r.thread === T1)?.status).toBe("needs_you");
});

test("a held view that goes while its thread was running reads the list again", async () => {
  let list = [thread(T1), thread(T2)];
  const { talks, store, transport, settle, listReads } = await rig(today(() => list));
  const release = store.watch(`thread:${T1}`);
  stops.push(store.watch(`thread:${T2}`));
  play(transport, T1, "running", 1);
  await settle();
  expect(talks.view().rows.find((r) => r.thread === T1)?.status).toBe("running");
  expect(listReads()).toBe(1);
  // the turn's end slips past: the view goes while it still says running
  list = [thread(T1, { title: "Clause nine" }), thread(T2)];
  release();
  await settle();
  // the store tells its listeners on its next frame
  play(transport, T2, "idle", 1);
  await settle();
  expect(listReads()).toBe(2);
  expect(talks.view().rows.find((r) => r.thread === T1)).toMatchObject({
    status: "idle",
    title: "Clause nine",
  });
});

test("a held view that goes while its thread rests reads nothing again", async () => {
  const { store, transport, settle, listReads } = await rig(today(() => [thread(T1), thread(T2)]));
  const release = store.watch(`thread:${T1}`);
  stops.push(store.watch(`thread:${T2}`));
  play(transport, T1, "idle", 1);
  await settle();
  release();
  await settle();
  play(transport, T2, "idle", 1);
  await settle();
  expect(listReads()).toBe(1);
});

/* ---------- H4 ---------- */

test("threads_changed: a whole list replaces the rows; a change upserts and removes", async () => {
  const { talks, transport, settle } = await rig(next(() => [info(T1), info(T2)]));
  expect(threadsOf(talks.view().rows)).toEqual([T2, T1]);
  transport.appEvent({
    threads_changed: { matter: A.matter, threads: [info(T3), info(T1)], removed: [], all: true },
  });
  await settle();
  expect(threadsOf(talks.view().rows)).toEqual([T3, T1]);
  transport.appEvent({
    threads_changed: {
      matter: A.matter,
      threads: [info(T2, { title: "Back" }), info(T1, { status: "running" })],
      removed: [T3],
      all: false,
    },
  });
  await settle();
  const rows = talks.view().rows;
  expect(threadsOf(rows)).toEqual([T2, T1]);
  expect(rows[0]?.title).toBe("Back");
  expect(rows[1]?.status).toBe("running");
});

test("threads_changed for a matter never visited is ignored; its list is read when it opens", async () => {
  const lists: Record<MatterId, ThreadInfo[]> = {
    [A.matter]: [info(T1)],
    [B.matter]: [info(T2, {}, B.matter)],
  };
  const { talks, matters, transport, settle } = await rig(
    next((m) => lists[m] ?? [], [detailed(A, { last_opened: 10 }), detailed(B)]),
  );
  expect(talks.view().matter).toBe(A.matter);
  transport.appEvent({
    threads_changed: {
      matter: B.matter,
      threads: [info(T3, {}, B.matter)],
      removed: [],
      all: true,
    },
  });
  await settle();
  expect(threadsOf(talks.view().rows)).toEqual([T1]);
  matters.open(B.matter);
  await settle();
  expect(talks.view().matter).toBe(B.matter);
  expect(threadsOf(talks.view().rows)).toEqual([T2]);
});

/* ---------- H6 ---------- */

test("mark_seen when a thread opens with the window focused", async () => {
  const { talks, marks } = await rig(
    next(() => [info(T1), info(T2, { seen: false, last_stop: { reason: "end_turn", at: 5 } })]),
    () => true,
  );
  // the newest opens: marked seen
  expect(marks()).toEqual([{ mark_seen: { thread: T2 } }]);
  expect(talks.view().rows.find((r) => r.thread === T2)?.seen).toBe(true);
  talks.open(T1);
  expect(marks()).toEqual([{ mark_seen: { thread: T2 } }, { mark_seen: { thread: T1 } }]);
});

test("no mark_seen without focus", async () => {
  const { talks, marks } = await rig(
    next(() => [info(T1), info(T2, { seen: false, last_stop: { reason: "end_turn", at: 5 } })]),
    () => false,
  );
  talks.open(T1);
  expect(marks()).toEqual([]);
  const row = talks.view().rows.find((r) => r.thread === T2);
  expect(row?.seen).toBe(false);
  expect(row && rowWords(row)).toEqual({ state: "ready" });
});

test("mark_seen when the open thread's turn ends (running to idle) with the window focused", async () => {
  let focused = false;
  const { store, transport, settle, marks } = await rig(
    next(() => [info(T1)]),
    () => focused,
  );
  expect(marks()).toEqual([]);
  focused = true;
  stops.push(store.watch(`thread:${T1}`));
  play(transport, T1, "running", 1);
  await settle();
  expect(marks()).toEqual([]);
  play(transport, T1, "idle", 2);
  await settle();
  expect(marks()).toEqual([{ mark_seen: { thread: T1 } }]);
});

test("a turn that ends with the window unfocused marks nothing", async () => {
  const { store, transport, settle, marks } = await rig(
    next(() => [info(T1)]),
    () => false,
  );
  stops.push(store.watch(`thread:${T1}`));
  play(transport, T1, "running", 1);
  await settle();
  play(transport, T1, "idle", 2);
  await settle();
  expect(marks()).toEqual([]);
});

/* ---------- following the matter ---------- */

test("switching matters follows the open one, and going back reopens the conversation open there", async () => {
  const lists: Record<MatterId, ThreadSummary[]> = {
    [A.matter]: [thread(T1, {}, A.matter)],
    [B.matter]: [thread(T2, {}, B.matter), thread(T3, {}, B.matter)],
  };
  const { talks, matters, settle } = await rig(today((m) => lists[m] ?? [], [A, B]));
  // the newest matter opens, and in it the newest thread
  expect(matters.view().open?.matter).toBe(B.matter);
  expect(talks.view().matter).toBe(B.matter);
  expect(talks.view().thread).toBe(T3);
  talks.open(T2);
  expect(talks.view().thread).toBe(T2);
  matters.open(A.matter);
  await settle();
  expect(talks.view().matter).toBe(A.matter);
  expect(talks.view().thread).toBe(T1);
  expect(threadsOf(talks.view().rows)).toEqual([T1]);
  matters.open(B.matter);
  await settle();
  expect(talks.view().matter).toBe(B.matter);
  expect(talks.view().open).toBe(T2);
  expect(talks.view().thread).toBe(T2);
});

/* ---------- H13: Retry from here ---------- */

const QUESTION = uuid(1, 5);

/** Today's threads, with H13's stand-in alone: retry_from makes RETRIED, or is refused. */
function retrying(
  threads: ThreadSummary[],
  answer: NonNullable<FakeNextHost["command"]>,
): FakeHost {
  return {
    ...today(() => threads),
    next: {
      parts: ["retry"],
      query: () => {
        throw new Error("no next query here");
      },
      command: answer,
    },
  };
}

const RETRIED = uuid(8, 1);

test("retry: retry_from with the thread and question; the new thread is listed with the source's agent and opens, with the line's facts", async () => {
  const { talks, transport, settle } = await rig(
    retrying(
      [thread(T1, { title: "Lease notice", agent: "codex" }), thread(T2, { title: "Indemnity" })],
      (command) => ("retry_from" in command ? { thread_created: { thread: RETRIED } } : undefined),
    ),
  );
  expect(talks.view().canRetry).toBe(true);
  expect(talks.view().retried).toBeNull();
  talks.open(T1);
  await talks.retry(T1, QUESTION, 1_000);
  await settle();
  expect(transport.only("next_command").map((c) => c.command)).toEqual([
    { retry_from: { thread: T1, message: QUESTION } },
  ]);
  const view = talks.view();
  expect(view.thread).toBe(RETRIED);
  expect(view.open).toBe(RETRIED);
  expect(view.rows.find((r) => r.thread === RETRIED)).toMatchObject({
    key: RETRIED,
    agent: "codex",
  });
  expect(view.retried).toEqual({ from: T1, title: "Lease notice", at: 1_000 });
  // back to the first: no line there
  talks.open(T1);
  expect(talks.view().retried).toBeNull();
});

test("retry from the thread that opened first: the line names that conversation by the key the room gave it", async () => {
  const { talks } = await rig(
    retrying([thread(T1, { title: "Lease notice" })], () => ({
      thread_created: { thread: RETRIED },
    })),
  );
  const first = talks.view().open;
  expect(first).not.toBe(T1);
  await talks.retry(T1, QUESTION, 7);
  expect(talks.view().retried).toEqual({ from: first, title: "Lease notice", at: 7 });
});

test("without H13's part canRetry is false and retry rejects, sending nothing", async () => {
  const { talks, transport } = await rig(today(() => [thread(T1)]));
  expect(talks.view().canRetry).toBe(false);
  await expect(talks.retry(T1, QUESTION, 1)).rejects.toBeDefined();
  expect(transport.only("next_command")).toEqual([]);
});

test("a refused retry rejects and opens nothing", async () => {
  const { talks, settle } = await rig(
    retrying([thread(T1), thread(T2)], () => {
      throw new Error("the host refused");
    }),
  );
  const before = talks.view();
  await expect(talks.retry(T2, QUESTION, 1)).rejects.toBeDefined();
  await settle();
  const after = talks.view();
  expect(after.open).toBe(before.open);
  expect(after.thread).toBe(T2);
  expect(threadsOf(after.rows)).toEqual([T2, T1]);
  expect(after.retried).toBeNull();
});
