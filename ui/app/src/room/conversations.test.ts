// conversations.ts over FakeTransport, with and without a NextHost: the rows and the one that opens, the draft New
// conversation makes, create_thread on the first send and the rename after it, the headers and H4's events that keep the
// rows current, H6's seen marks, and following the open matter. A row's words are in conversation-status.test.ts; retry
// from here is in conversations-retry.test.ts.
import { expect, test, vi } from "vitest";
import type { MatterId, ThreadSummary } from "../transport/protocol.ts";
import { DEFAULT_AGENT } from "./agent.ts";
import { rowWords } from "./conversation-status.ts";
import { titleOf } from "./conversation-title.ts";
import {
  A,
  B,
  info,
  MADE,
  next,
  play,
  rig,
  stops,
  T1,
  T2,
  T3,
  thread,
  threadsOf,
  today,
} from "./conversations-rig.ts";
import { detailed } from "./fake-next.ts";
import type { ThreadInfo } from "./host-next.ts";

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
  const made = await talks.create(talks.view().open ?? "");
  expect(made).toBe(MADE);
  expect(sent("create_thread")).toEqual([{ matter: A.matter, agent: "claude-acp" }]);
  const view = talks.view();
  expect(view.open).toBe(key);
  expect(view.thread).toBe(MADE);
  expect(view.drafting).toBe(false);
  expect(view.rows[0]).toMatchObject({ key, thread: MADE, agent: "claude-acp" });
  expect(view.rows).toHaveLength(2);
  // the open conversation now has its thread: create doesn't make another
  expect(await talks.create(talks.view().open ?? "")).toBe(MADE);
  expect(sent("create_thread")).toHaveLength(1);
});

test("a matter with no threads: the draft it opens keeps its key once create makes the thread", async () => {
  const { talks } = await rig(today(() => []));
  const key = talks.view().open;
  await talks.create(talks.view().open ?? "");
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
  const made = talks.create(talks.view().open ?? "");
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
  expect(await talks.create(talks.view().open ?? "")).toBe(MADE);
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
  await expect(talks.create(talks.view().open ?? "")).rejects.toBeDefined();
  expect(sent("create_thread")).toEqual([]);
});

test("the agent create_thread names: the plan chosen, else the one the matter uses most (archived left out), else DEFAULT_AGENT", async () => {
  // nothing to go by
  const none = await rig(today(() => []));
  await none.talks.create(none.talks.view().open ?? "");
  expect(none.sent("create_thread")).toEqual([{ matter: A.matter, agent: DEFAULT_AGENT }]);
  // the plan used most, over the newest thread's; an archived thread doesn't count
  const newest = await rig(
    today(() => [
      thread(T1, { agent: "codex" }),
      thread(T2, { agent: "codex" }),
      thread(T3, { agent: "claude-acp" }),
      thread(MADE, { agent: "claude-acp", status: "archived" }),
    ]),
  );
  newest.talks.startNew();
  expect(newest.talks.view().agent).toBe("codex");
  expect(newest.talks.view().plan).toBe("ChatGPT");
  await newest.talks.create(newest.talks.view().open ?? "");
  expect(newest.sent("create_thread")).toEqual([{ matter: A.matter, agent: "codex" }]);
  // the lawyer's choice over both
  const chosen = await rig(today(() => [thread(T1, { agent: "codex" })]));
  chosen.talks.startNew();
  chosen.talks.choose("claude-acp");
  expect(chosen.talks.view().agent).toBe("claude-acp");
  await chosen.talks.create(chosen.talks.view().open ?? "");
  expect(chosen.sent("create_thread")).toEqual([{ matter: A.matter, agent: "claude-acp" }]);
});

/* ---------- the title after the first send ---------- */

test("sent: rename_thread with the title the message gives, marked a placeholder, once per thread", async () => {
  const { talks, sent, settle } = await rig(today(() => []));
  const made = await talks.create(talks.view().open ?? "");
  const text = "What does   clause nine of the lease say about notice?";
  talks.sent(made, text);
  talks.sent(made, "And clause ten?");
  await settle();
  expect(sent("rename_thread")).toEqual([
    { thread: made, title: titleOf(text), placeholder: true },
  ]);
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
  const made = await talks.create(talks.view().open ?? "");
  talks.sent(made, "\u200b\u200b\u200b");
  await settle();
  expect(sent("rename_thread")).toEqual([]);
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

/* ---------- review round 1 ---------- */

test("S1: back in a matter, its open conversation is marked seen again (focused, with the matters part)", async () => {
  const lists: Record<MatterId, ThreadInfo[]> = {
    [A.matter]: [info(T1)],
    [B.matter]: [info(T2, {}, B.matter)],
  };
  const { matters, settle, marks } = await rig(
    next((m) => lists[m] ?? [], [detailed(A, { last_opened: 10 }), detailed(B)]),
    () => true,
  );
  expect(marks()).toEqual([{ mark_seen: { thread: T1 } }]);
  matters.open(B.matter);
  await settle();
  expect(marks().at(-1)).toEqual({ mark_seen: { thread: T2 } });
  matters.open(A.matter);
  await settle();
  expect(marks().at(-1)).toEqual({ mark_seen: { thread: T1 } });
  expect(marks().filter((c) => "mark_seen" in c && c.mark_seen.thread === T1)).toHaveLength(2);
});

/** A list whose first read fails and whose second waits for the test's answer. */
function failThenHold() {
  let answer: (threads: ThreadSummary[]) => void = () => {};
  let reads = 0;
  const host = today(() => {
    reads += 1;
    if (reads === 1) throw new Error("the host is busy");
    return new Promise<ThreadSummary[]>((resolve) => (answer = resolve));
  });
  return { host, answer: (threads: ThreadSummary[]) => answer(threads) };
}

test("S2: create(key) makes the thread for that key, even when New conversation opened meanwhile", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const { host, answer } = failThenHold();
  const { talks, sent, settle } = await rig(host);
  expect(talks.view().phase).toBe("failed");
  const keyA = talks.view().open as string;
  const made = talks.create(keyA);
  talks.startNew();
  const draft = talks.view().open as string;
  expect(draft).not.toBe(keyA);
  // the draft's plan is its own: the thread for A doesn't take it
  talks.choose("codex-acp");
  answer([]);
  expect(await made).toBe(MADE);
  await settle();
  expect(sent("create_thread")).toEqual([{ matter: A.matter, agent: DEFAULT_AGENT }]);
  const view = talks.view();
  expect(view.open).toBe(draft);
  expect(view.drafting).toBe(true);
  expect(view.rows.map((r) => [r.key, r.thread])).toEqual([
    [draft, null],
    [keyA, MADE],
  ]);
  expect(view.agent).toBe("codex-acp");
});

test("S3: a list read that predates a thread the room made keeps it listed and open", async () => {
  let answer: ((threads: ThreadSummary[]) => void) | null = null;
  let reads = 0;
  const { talks, matters, settle } = await rig(
    today(
      (m) => {
        if (m !== B.matter) return [];
        reads += 1;
        if (reads === 1) return [];
        return new Promise<ThreadSummary[]>((resolve) => (answer = resolve));
      },
      [A, B],
    ),
  );
  expect(talks.view().matter).toBe(B.matter);
  matters.open(A.matter);
  await settle();
  matters.open(B.matter);
  await settle();
  // B's second read is on its way; the first send makes the thread meanwhile
  expect(answer).not.toBeNull();
  const key = talks.view().open as string;
  expect(await talks.create(key)).toBe(MADE);
  answer!([]);
  await settle();
  const view = talks.view();
  expect(threadsOf(view.rows)).toEqual([MADE]);
  expect(view.thread).toBe(MADE);
  expect(view.open).toBe(key);
});

test("S5: a thread made whose question was never accepted is archived when left with nothing typed", async () => {
  const { talks, sent, settle } = await rig(today(() => [thread(T1)]));
  const t1 = talks.view().open as string;
  talks.startNew();
  const key = talks.view().open as string;
  await talks.create(key);
  talks.open(t1);
  await settle();
  expect(sent("archive_thread")).toEqual([{ thread: MADE }]);
  expect(threadsOf(talks.view().rows)).toEqual([T1]);
});

test("S5: with words typed it stays, and once a question is accepted it is never archived", async () => {
  const { talks, drafts, sent, settle } = await rig(today(() => [thread(T1)]));
  const t1 = talks.view().open as string;
  talks.startNew();
  const key = talks.view().open as string;
  await talks.create(key);
  drafts.set(key, "Summarise the lease");
  talks.open(t1);
  await settle();
  expect(sent("archive_thread")).toEqual([]);
  expect(threadsOf(talks.view().rows)).toEqual([MADE, T1]);
  // accepted, then left with nothing typed: kept
  talks.open(key);
  talks.sent(MADE, "Summarise the lease");
  drafts.delete(key);
  talks.open(t1);
  await settle();
  expect(sent("archive_thread")).toEqual([]);
  expect(threadsOf(talks.view().rows)).toEqual([MADE, T1]);
});

test("N1: a plan chosen while the list failed is the one create_thread names after the re-read", async () => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const { host, answer } = failThenHold();
  const { talks, sent } = await rig(host);
  expect(talks.view().phase).toBe("failed");
  const key = talks.view().open as string;
  talks.choose("codex-acp");
  const made = talks.create(key);
  answer([]);
  expect(await made).toBe(MADE);
  expect(sent("create_thread")).toEqual([{ matter: A.matter, agent: "codex-acp" }]);
});

test("N2: words of whitespace only leave the drafts map with their draft", async () => {
  const { talks, drafts } = await rig(today(() => [thread(T1)]));
  const t1 = talks.view().open as string;
  talks.startNew();
  const key = talks.view().open as string;
  drafts.set(key, "   ");
  talks.open(t1);
  expect(drafts.has(key)).toBe(false);
  expect(threadsOf(talks.view().rows)).toEqual([T1]);
});

test("opening the conversation already open asks for the field's focus again", async () => {
  const { talks } = await rig(today(() => [thread(T1)]));
  const key = talks.view().open as string;
  const before = talks.view().focus?.n ?? 0;
  talks.open(key);
  expect(talks.view().focus).toEqual({ key, n: before + 1 });
  talks.open(key);
  expect(talks.view().focus?.n).toBe(before + 2);
});
