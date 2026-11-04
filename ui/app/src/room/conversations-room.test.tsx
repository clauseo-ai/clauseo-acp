// @vitest-environment happy-dom
// Conversations in the real room (room.tsx over FakeTransport): New conversation's first send in its order, the rows
// under the open matter, moving between conversations, what a row not open says to assistive technology, and Hindi.
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
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
import { detailed, fakeNext } from "./fake-next.ts";
import type { AgentSummary, PaperSummary, ThreadInfo } from "./host-next.ts";
import { RetryFromHere } from "./retry-from-here.tsx";
import { ThreadActionsContext, type ThreadActions } from "./thread/session.ts";
import { clockOf } from "./when.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0); // a fixed instant; fixture times derive from NOW
let h: Harness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const T1 = uuid(1, 1);
const T2 = uuid(2, 1);
const T3 = uuid(3, 1);
const T4 = uuid(4, 1);
const MADE = uuid(9, 1);

const thread = (id: ThreadId, over: Partial<ThreadSummary> = {}): ThreadSummary => ({
  thread: id,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status: "idle",
  ...over,
});

const headerOf = (
  id: ThreadId,
  status: ThreadStatus,
  agent: AgentId = "claude-acp",
): ViewEntry => ({
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

// The file's first test mounts the room cold: 9.6 s on a loaded runner, and the rest of the file failed after it, so 30 s of its own.
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
  expect(h.sent("create_thread")).toEqual([{ matter: THE_MATTER.matter, agent: "claude-acp" }]);
  expect(h.sent("send_prompt")[0]?.thread).toBe(MADE);
  expect(h.sent("rename_thread")).toEqual([{ thread: MADE, title: titleOf(question) }]);
  // the draft became the thread, its row titled (with the next frame: #253), still current
  await act(async () => h!.store.flush());
  const current = rows().find((r) => r.getAttribute("aria-current") === "true");
  expect(current?.textContent).toContain(titleOf(question) ?? "");
  expect(rows()).toHaveLength(2);
}, 30_000);

test("the rows under the open matter: the draft says New conversation, a thread its title, the current one marked, the plan only where it differs", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
  });
  // before New conversation: the newest is current; a row whose plan is the matter's names none
  expect(rows().map((r) => r.textContent)).toEqual(["Indemnity", "Lease notice"]);
  expect(rowOf("Indemnity")?.getAttribute("aria-current")).toBe("true");
  expect(rowOf("Lease notice")?.getAttribute("aria-current")).toBeNull();
  await h.unmount();

  // Claude used most: only the ChatGPT row names its plan, though it is the newest
  h = await mountRoom({
    threads: [
      thread(T1, { title: "Lease notice" }),
      thread(T3, { title: "Reply to notice" }),
      thread(T4, { title: "Indemnity", agent: "codex" }),
    ],
  });
  expect(rowOf("Indemnity")?.querySelector(".room-conv-plan")?.textContent).toBe("ChatGPT");
  expect(rowOf("Lease notice")?.querySelector(".room-conv-plan")).toBeNull();
  expect(rowOf("Reply to notice")?.querySelector(".room-conv-plan")).toBeNull();
  await h.click(h.one("new-conversation"));
  const [draft, ...rest] = rows();
  expect(draft?.querySelector(".room-conv-title")?.textContent).toBe("New conversation");
  expect(draft?.getAttribute("aria-current")).toBe("true");
  expect(rest.map((r) => r.querySelector(".room-conv-title")?.textContent)).toEqual([
    "Indemnity",
    "Reply to notice",
    "Lease notice",
  ]);
  expect(rest.every((r) => r.getAttribute("aria-current") === null)).toBe(true);
  // the draft runs the plan used most, Claude, the matter's own: it names none
  expect(draft?.querySelector(".room-conv-plan")).toBeNull();
  await h.unmount();

  // a tie goes to the plan used most recently: ChatGPT, the newest, so the Claude row names its plan
  h = await mountRoom({
    threads: [
      thread(T1, { title: "Lease notice" }),
      thread(T2, { title: "Indemnity", agent: "codex" }),
    ],
  });
  expect(rowOf("Lease notice")?.querySelector(".room-conv-plan")?.textContent).toBe("Claude");
  expect(rowOf("Indemnity")?.querySelector(".room-conv-plan")).toBeNull();
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
  const time = clockOf(at, NOW, Intl.DateTimeFormat().resolvedOptions().timeZone, "en").time;
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

/* ---------- the composer per conversation ---------- */

const field = () => h!.one("composer") as HTMLTextAreaElement | null;
const newRow = () =>
  rows().find((r) => r.querySelector(".room-conv-title")?.textContent === "New conversation");

test("each conversation keeps its own words in the field", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
  });
  await h.type("About the indemnity");
  await h.click(rowOf("Lease notice"));
  expect(field()?.value).toBe("");
  await h.type("About the notice");
  await h.click(rowOf("Indemnity"));
  expect(field()?.value).toBe("About the indemnity");
  await h.click(rowOf("Lease notice"));
  expect(field()?.value).toBe("About the notice");
});

test("a New conversation with words survives leaving it; an empty one is dropped", async () => {
  h = await mountRoom({ threads: [thread(T1, { title: "Lease notice" })] });
  // empty: gone once left
  await h.click(h.one("new-conversation"));
  expect(newRow()).toBeDefined();
  await h.click(rowOf("Lease notice"));
  expect(newRow()).toBeUndefined();
  // with words: its row stays, and its words come back
  await h.click(h.one("new-conversation"));
  await h.type("Draft a reply to the bank");
  await h.click(rowOf("Lease notice"));
  expect(newRow()).toBeDefined();
  expect(field()?.value).toBe("");
  await h.click(newRow());
  expect(newRow()?.getAttribute("aria-current")).toBe("true");
  expect(field()?.value).toBe("Draft a reply to the bank");
  expect(h.sent("create_thread")).toEqual([]);
});

test("the field takes focus after New conversation and after a row is pressed, not on first load", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
  });
  expect(field()).not.toBeNull();
  expect(document.activeElement).not.toBe(field());
  await h.click(h.one("new-conversation"));
  expect(document.activeElement).toBe(field());
  (document.activeElement as HTMLElement | null)?.blur();
  await h.click(rowOf("Lease notice"));
  expect(document.activeElement).toBe(field());
});

/* ---------- the plan to ask with ---------- */

/** The plan switch, a group named by the visible "Plan to ask with" before it (aria-labelledby). */
const planSwitch = () => {
  const group = h!.host.querySelector<HTMLElement>(".room-plan-seg");
  const by = group?.getAttribute("aria-labelledby");
  return by != null && h!.host.ownerDocument.getElementById(by)?.textContent === "Plan to ask with"
    ? group
    : null;
};
const planButton = (label: string) =>
  [...(planSwitch()?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.includes(label));

const withAgents = (agents: AgentSummary[]) =>
  fakeNext({ matters: () => [], agents, parts: ["agents"] });

const TWO: AgentSummary[] = [
  { id: "claude-acp", kind: "claude" },
  { id: "codex-acp", kind: "codex" },
];

test("a new conversation offers the plan where the host lists two; ChatGPT chosen makes the thread with codex-acp, and the switch goes", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice", agent: "claude-acp" })],
    next: withAgents(TWO),
    dispatch: (command) =>
      command.create_thread !== undefined
        ? { thread_created: { thread: MADE } }
        : defaultOutcome(command),
  });
  // an existing thread: no switch
  expect(planSwitch()).toBeNull();
  await h.click(h.one("new-conversation"));
  expect(planSwitch()).not.toBeNull();
  expect(planButton("Claude")).toBeDefined();
  await h.click(planButton("ChatGPT"));
  await h.type("Summarise the indemnity clause");
  await h.key({ key: "Enter", keyCode: 13 });
  await h.settle();
  expect(h.sent("create_thread")).toEqual([{ matter: THE_MATTER.matter, agent: "codex-acp" }]);
  expect(planSwitch()).toBeNull();
});

test("with one agent listed the switch never shows", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    next: withAgents([{ id: "claude-acp", kind: "claude" }]),
  });
  expect(planSwitch()).toBeNull();
  await h.click(h.one("new-conversation"));
  expect(newRow()).toBeDefined();
  expect(planSwitch()).toBeNull();
});

/* ---------- the folded rail, New conversation's key ---------- */

const rail = () => h!.host.querySelector("nav.room-rail");

test("below 1200 px a row pressed or New conversation folds the raised rail, and the field takes the focus", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" }), thread(T2, { title: "Indemnity" })],
  });
  await h.click(h.one("rail-toggle"));
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  await h.click(rowOf("Lease notice"));
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(field());
  await h.click(h.one("rail-toggle"));
  expect(rail()?.hasAttribute("data-open")).toBe(true);
  await h.click(h.one("new-conversation"));
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(newRow()?.getAttribute("aria-current")).toBe("true");
  expect(document.activeElement).toBe(field());
});

/** Cmd+N on the Mac, Ctrl+N elsewhere, as New conversation's title names it; returns whether it was prevented. */
async function newKey(over: KeyboardEventInit = {}): Promise<boolean> {
  const mac = h!.one("new-conversation")?.title === "⌘N";
  const e = new KeyboardEvent("keydown", {
    key: "n",
    code: "KeyN",
    bubbles: true,
    cancelable: true,
    ...(mac ? { metaKey: true } : { ctrlKey: true }),
    ...over,
  });
  await act(async () => void document.body.dispatchEvent(e));
  await h!.settle();
  return e.defaultPrevented;
}

test("Cmd+N or Ctrl+N starts a new conversation and sends nothing; New conversation names the key", async () => {
  h = await mountRoom({ threads: [thread(T1, { title: "Lease notice" })] });
  expect(["⌘N", "Ctrl+N"]).toContain(h.one("new-conversation")?.title);
  await h.click(h.one("rail-toggle"));
  expect(newRow()).toBeUndefined();
  expect(await newKey()).toBe(true);
  expect(newRow()?.getAttribute("aria-current")).toBe("true");
  expect(rail()?.hasAttribute("data-open")).toBe(false);
  expect(document.activeElement).toBe(field());
  expect(kinds()).toEqual([]);
});

test("the key does nothing while an input method composes, nor with no matter open", async () => {
  h = await mountRoom({ threads: [thread(T1, { title: "Lease notice" })] });
  expect(await newKey({ isComposing: true })).toBe(false);
  expect(await newKey({ keyCode: 229 })).toBe(false);
  expect(newRow()).toBeUndefined();
  await h.unmount();
  const start = vi.spyOn(Conversations.prototype, "startNew");
  h = await mountRoom({ matters: [] });
  const e = new KeyboardEvent("keydown", {
    key: "n",
    code: "KeyN",
    bubbles: true,
    cancelable: true,
    ctrlKey: true,
  });
  const m = new KeyboardEvent("keydown", {
    key: "n",
    code: "KeyN",
    bubbles: true,
    cancelable: true,
    metaKey: true,
  });
  await act(async () => void (document.body.dispatchEvent(e), document.body.dispatchEvent(m)));
  expect(start).not.toHaveBeenCalled();
  expect(e.defaultPrevented || m.defaultPrevented).toBe(false);
});

test("the key goes by the letter the layout gives, and by its place only where the layout gives no Latin letter", async () => {
  h = await mountRoom({ threads: [thread(T1, { title: "Lease notice" })] });
  // Dvorak: the key in N's place is B, so Ctrl+B is not New conversation
  expect(await newKey({ key: "b", code: "KeyN" })).toBe(false);
  expect(newRow()).toBeUndefined();
  // Dvorak's own N, in another place
  expect(await newKey({ key: "n", code: "KeyL" })).toBe(true);
  expect(newRow()).toBeDefined();
  await h.click(rowOf("Lease notice"));
  // a Devanagari layout gives no Latin letter: N's place stands for it
  expect(await newKey({ key: "ल", code: "KeyN" })).toBe(true);
  expect(newRow()).toBeDefined();
});

/* ---------- a draft's line, the status line's plan ---------- */

const paper = (i: number, path: string): PaperSummary =>
  ({ file: uuid(i, 8), path, size: 1000, modified: 1_730_000_000_000 }) as PaperSummary;
const draftLine = () => h!.one("new-conversation-line");

test("a new conversation says what it reads once the papers are counted; nothing without them", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    next: fakeNext({
      matters: () => [],
      papers: [paper(1, "lease.pdf"), paper(2, "notice.pdf"), paper(3, "reply.pdf")],
      parts: ["papers"],
    }),
  });
  expect(draftLine()).toBeNull();
  await h.click(h.one("new-conversation"));
  expect(draftLine()?.textContent).toBe("A new conversation. It reads this matter's 3 papers.");
  await h.click(rowOf("Lease notice"));
  expect(draftLine()).toBeNull();
  await h.unmount();
  // without the papers part the count is unknown: no line
  h = await mountRoom({ threads: [thread(T1, { title: "Lease notice" })] });
  await h.click(h.one("new-conversation"));
  expect(newRow()).toBeDefined();
  expect(draftLine()).toBeNull();
});

test("with one paper the draft's line says so in the singular", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice" })],
    next: fakeNext({ matters: () => [], papers: [paper(1, "lease.pdf")], parts: ["papers"] }),
  });
  await h.click(h.one("new-conversation"));
  expect(draftLine()?.textContent).toBe("A new conversation. It reads this matter's one paper.");
});

test("the status line names the open conversation's plan, and a draft's", async () => {
  h = await mountRoom({
    threads: [
      thread(T1, { title: "Lease notice", agent: "claude-acp" }),
      thread(T2, { title: "Indemnity", agent: "codex-acp" }),
    ],
    next: withAgents(TWO),
  });
  const planWords = () => h!.host.querySelector(".room-status-plan")?.textContent;
  expect(planWords()).toBe("Uses your ChatGPT plan");
  await h.click(rowOf("Lease notice"));
  expect(planWords()).toBe("Uses your Claude plan");
  // a draft with two plans shows the switch, not the words, and the switch is no live region
  await h.click(h.one("new-conversation"));
  expect(planSwitch()).not.toBeNull();
  expect(planSwitch()?.closest('[role="status"]')).toBeNull();
  expect(h.host.querySelector("p .room-plan-seg, p [role=group]")).toBeNull();
});

test("an agent of no kind names no plan on the status line", async () => {
  h = await mountRoom({
    threads: [thread(T1, { title: "Lease notice", agent: "fake-agent" })],
    next: withAgents([{ id: "fake-agent" }]),
  });
  expect(h.host.querySelector(".room-status-plan")?.textContent ?? "").toBe("");
});

test("a stop on another day says nothing on its row; one today says when", async () => {
  const day = 24 * 60 * 60 * 1000;
  const info = (id: ThreadId, title: string, at: number): ThreadInfo => ({
    ...thread(id, { title }),
    seen: false,
    last_stop: { reason: "cancelled", at },
  });
  h = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER)],
      threads: () => [
        info(T1, "Lease notice", NOW - 2 * day),
        info(T2, "Indemnity", NOW - 30 * 60_000),
        { ...thread(T3, { title: "Reply" }), seen: true },
      ],
    }),
  });
  expect(rowOf("Reply")?.getAttribute("aria-current")).toBe("true");
  expect(rowOf("Lease notice")?.getAttribute("data-state")).toBe("idle");
  expect(rowOf("Lease notice")?.title).toBe("Lease notice");
  expect(rowOf("Indemnity")?.getAttribute("data-state")).toBe("stopped");
  expect(rowOf("Indemnity")?.title).toContain("Stopped at");
});
