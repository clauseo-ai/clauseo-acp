// @vitest-environment happy-dom
// Conversations in the real room (room.tsx over FakeTransport): New conversation's first send in its order, the rows
// under the open matter, moving between conversations, what a row not open says to assistive technology, and Hindi.
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { reset, userMessage, uuid } from "../testing/stream-script.ts";
import type {
  AgentId,
  HostCommand,
  ThreadId,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { titleOf } from "./conversation-title.ts";
import { Conversations } from "./conversations.ts";
import { RetryFromHere } from "./retry-from-here.tsx";
import { ThreadActionsContext, type ThreadActions } from "./thread/session.ts";
import { clockOf } from "./when.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.restoreAllMocks();
});

const T1 = uuid(1, 1);
const T2 = uuid(2, 1);
const T3 = uuid(3, 1);
const T4 = uuid(4, 1);
const MADE = uuid(9, 1);

const thread = (id: ThreadId, over: Partial<ThreadSummary> = {}): ThreadSummary => ({
  thread: id,
  matter: THE_MATTER.matter,
  agent: "claude",
  status: "idle",
  ...over,
});

const headerOf = (id: ThreadId, status: ThreadStatus, agent: AgentId = "claude"): ViewEntry => ({
  header: {
    order: 1,
    thread: id,
    matter: THE_MATTER.matter,
    agent,
    status,
    config_options: [],
    commands: [],
    plan: [],
  },
});

const rows = () => h!.all("conversation");
/** A row by its title: the thread that opens first keeps the key the room gave it before the list was in. */
const rowOf = (title: string) =>
  rows().find((r) => r.querySelector(".room-conv-title")?.textContent === title);
const kinds = () =>
  h!.transport.only("dispatch").map((c) => Object.keys(c.command.command)[0] as keyof HostCommand);
const streams = (call: "subscribe" | "unsubscribe") => h!.transport.only(call).map((c) => c.stream);

test("New conversation sends nothing; its first Enter runs create_thread, send_prompt, rename_thread, in that order", async () => {
  h = await mountRoom({
    threads: [thread(T1)],
    dispatch: (command) =>
      command.create_thread !== undefined
        ? { thread_created: { thread: MADE } }
        : defaultOutcome(command),
  });
  const before = h.transport.calls.length;
  await h.click(h.one("new-conversation"));
  expect(h.transport.calls.slice(before).filter((c) => c.call === "dispatch")).toEqual([]);
  expect(kinds()).toEqual([]);
  const question = "What does clause nine of the lease say about notice to the bank?";
  await h.type(question);
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(kinds()).toEqual(["create_thread", "send_prompt", "rename_thread"]);
  expect(h.sent("create_thread")).toEqual([{ matter: THE_MATTER.matter, agent: "claude" }]);
  expect(h.sent("send_prompt")[0]?.thread).toBe(MADE);
  expect(h.sent("rename_thread")).toEqual([{ thread: MADE, title: titleOf(question) }]);
  // the draft became the thread, its row titled, still current
  const current = rows().find((r) => r.getAttribute("aria-current") === "true");
  expect(current?.textContent).toContain(titleOf(question) ?? "");
  expect(rows()).toHaveLength(2);
});

test("the rows under the open matter: the draft says New conversation, a thread its title, the current one marked, the plan only where it differs", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
  });
  // before New conversation: the newest is current; the one with the same plan as the oldest names none
  expect(rows().map((r) => r.textContent)).toEqual(["Indemnity", "Lease notice"]);
  expect(rowOf("Indemnity")?.getAttribute("aria-current")).toBe("true");
  expect(rowOf("Lease notice")?.getAttribute("aria-current")).toBeNull();
  await h.unmount();

  h = await mountRoom({
    threads: [
      thread(T1, { title: "Lease notice" }),
      thread(T2, { title: "Indemnity", agent: "codex" }),
    ],
  });
  expect(rowOf("Indemnity")?.querySelector(".room-conv-plan")?.textContent).toBe("ChatGPT");
  expect(rowOf("Lease notice")?.querySelector(".room-conv-plan")).toBeNull();
  await h.click(h.one("new-conversation"));
  const [draft, ...rest] = rows();
  expect(draft?.querySelector(".room-conv-title")?.textContent).toBe("New conversation");
  expect(draft?.getAttribute("aria-current")).toBe("true");
  expect(rest.map((r) => r.querySelector(".room-conv-title")?.textContent)).toEqual([
    "Indemnity",
    "Lease notice",
  ]);
  expect(rest.every((r) => r.getAttribute("aria-current") === null)).toBe(true);
  // the draft names the plan it would run: the newest thread's, ChatGPT, unlike the oldest's
  expect(draft?.querySelector(".room-conv-plan")?.textContent).toBe("ChatGPT");
  expect(rowOf("Lease notice")?.querySelector(".room-conv-plan")).toBeNull();
});

test("switching: the other row's thread is watched and shown; the first's header stays held while it runs, and goes once idle", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
  });
  expect(streams("subscribe")).toEqual([`thread:${T2}`]);
  h.deliverOn(`thread:${T2}`, [
    reset([userMessage(5, 2, "Summarise the indemnity clause")], headerOf(T2, "running")),
  ]);
  expect(h.one("message-user")?.textContent).toContain("Summarise the indemnity clause");
  await h.click(rowOf("Lease notice"));
  expect(streams("subscribe")).toEqual([`thread:${T2}`, `thread:${T1}`]);
  h.deliverOn(`thread:${T1}`, [
    reset([userMessage(5, 1, "What does clause nine say?")], headerOf(T1, "idle")),
  ]);
  await h.settle();
  expect(h.all("message-user").map((m) => m.textContent)).toEqual([
    expect.stringContaining("What does clause nine say?"),
  ]);
  expect(rowOf("Lease notice")?.getAttribute("aria-current")).toBe("true");
  // left while running: its header stays subscribed, and its row says so
  expect(streams("unsubscribe")).toEqual([]);
  expect(rowOf("Indemnity")?.dataset["state"]).toBe("working");
  h.deliverOn(`thread:${T2}`, [reset([], headerOf(T2, "idle"))]);
  await h.settle();
  expect(streams("unsubscribe")).toEqual([`thread:${T2}`]);
  expect(rowOf("Indemnity")?.dataset["state"]).toBe("idle");
});

test("a row not open says what it's doing: Answering., a question, waiting for another", async () => {
  h = await mountRoom({
    threads: [
      thread(T1, { title: "Lease notice", status: "running" }),
      thread(T2, { title: "Bank letters", status: "needs_you" }),
      thread(T3, { title: "Indemnity", status: { queued: { behind: T1 } } }),
      thread(T4, { title: "Open one" }),
    ],
  });
  expect(rowOf("Open one")?.getAttribute("aria-current")).toBe("true");
  expect(rowOf("Open one")?.querySelector(".sr-only")).toBeNull();
  expect(rowOf("Lease notice")?.querySelector(".sr-only")?.textContent).toContain("Answering.");
  expect(rowOf("Bank letters")?.textContent).toContain("A question for you");
  expect(rowOf("Indemnity")?.querySelector(".sr-only")?.textContent).toContain(
    "Waiting for 'Lease notice'",
  );
});

test("in Hindi the draft says नई बातचीत", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    display: { language: "hi" },
  });
  await h.click(h.one("new-conversation"));
  expect(rows()[0]?.querySelector(".room-conv-title")?.textContent).toBe("नई बातचीत");
});

/* ---------- H13: Retry from here ---------- */

test("after a retry the new conversation opens with its line: the question's time, the first's title, and Open it goes back", async () => {
  // the room makes its Conversations itself: catch it as it starts (the spy calls through)
  const start = vi.spyOn(Conversations.prototype, "start");
  const RETRIED = uuid(8, 1);
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
    next: {
      parts: ["retry"],
      query: () => {
        throw new Error("no next query here");
      },
      command: () => ({ thread_created: { thread: RETRIED } }),
    },
  });
  const talks = start.mock.contexts[0] as Conversations | undefined;
  expect(talks).toBeDefined();
  expect(h.one("retried-open")).toBeNull();
  const at = Date.UTC(2026, 10, 2, 11, 1);
  await act(async () => talks!.retry(T2, uuid(1, 5), at));
  // the line is a late chunk: wait for it, inside an act React can retry the Suspense in
  await act(async () => void (await import("./retried-line.tsx")));
  await h.settle();
  expect(h.transport.only("next_command").map((c) => c.command)).toEqual([
    { retry_from: { thread: T2, message: uuid(1, 5) } },
  ]);
  expect(h.transport.only("subscribe").map((c) => c.stream)).toContain(`thread:${RETRIED}`);
  const open = h.one("retried-open");
  expect(open).not.toBeNull();
  const line = open?.closest("p")?.textContent ?? "";
  const time = clockOf(at, Date.now(), Intl.DateTimeFormat().resolvedOptions().timeZone, "en").time;
  expect(line).toContain(time);
  expect(line).toContain("Retried from your question at");
  expect(line).toContain("Indemnity");
  // the new conversation is current; the first isn't
  expect(rowOf("Indemnity")?.getAttribute("aria-current")).toBeNull();
  await h.click(open);
  expect(rowOf("Indemnity")?.getAttribute("aria-current")).toBe("true");
  expect(h.one("retried-open")).toBeNull();
});

/** RetryFromHere alone, under the actions given. */
async function retryButton(retry: ThreadActions["retry"]) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const actions: ThreadActions = {
    stream: null,
    sentTurn: null,
    undo: async () => {},
    answer: async () => {},
    stop: () => {},
    ...(retry === undefined ? {} : { retry }),
  };
  await act(async () =>
    root.render(
      createElement(
        ThreadActionsContext,
        { value: actions },
        createElement(RetryFromHere, { message: uuid(1, 5), at: 42 }),
      ),
    ),
  );
  return {
    button: () => host.querySelector<HTMLElement>('[data-testid="retry-from-here"]'),
    done: async () => {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

test("RetryFromHere draws nothing where the host can't retry", async () => {
  const r = await retryButton(undefined);
  expect(r.button()).toBeNull();
  await r.done();
});

test("RetryFromHere: a press calls retry(message, at) once, even pressed twice before it answers", async () => {
  let answer: () => void = () => {};
  const retry = vi.fn(() => new Promise<void>((resolve) => (answer = resolve)));
  const r = await retryButton(retry);
  // two presses, each its own event, the first still on its way
  await act(async () => r.button()?.click());
  await act(async () => r.button()?.click());
  expect(r.button()?.getAttribute("aria-disabled")).toBe("true");
  expect(retry).toHaveBeenCalledTimes(1);
  expect(retry).toHaveBeenCalledWith(uuid(1, 5), 42);
  // once it answers, a press goes again
  await act(async () => answer());
  await act(async () => r.button()?.click());
  expect(retry).toHaveBeenCalledTimes(2);
  await r.done();
});
