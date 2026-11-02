// @vitest-environment happy-dom
// U7's matters/other in the real room (room.tsx over FakeTransport): the wait over the composer while another
// conversation of the matter holds it, before sending (another running, needing the lawyer or starting) and after
// (`queued { behind }`); Queue sends the field's words as Ask does; Stop the other cancels the thread the host names;
// a refused cancel shows its code's words; no time ever shows; and a change of the other's status renders no row of
// the open transcript.
import { act, createElement, memo, Profiler, type ProfilerOnRenderCallback } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { agentText, reset, THREAD, upsert, userMessage, uuid } from "../testing/stream-script.ts";
import type {
  CommandOutcome,
  EntryId,
  HostCommand,
  StreamId,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { defaultOutcome, mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

// each transcript row inside its own Profiler, behind a memo like the real row's (stream-commits.test.tsx's way)
const rowCommits = new Map<string, number>();
const countRow: ProfilerOnRenderCallback = (id) =>
  rowCommits.set(id, (rowCommits.get(id) ?? 0) + 1);
vi.mock("./thread/rows.tsx", async (load) => {
  const real = await load<typeof import("./thread/rows.tsx")>();
  return {
    ...real,
    Row: memo(function ProfiledRow(props: { id: EntryId; stream: StreamId }) {
      return createElement(
        Profiler,
        { id: props.id, onRender: countRow },
        createElement(real.Row, props),
      );
    }),
  };
});

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  rowCommits.clear();
});

/** The other conversation: its id sorts below THREAD, so with no activity times THREAD is the newest and opens. */
const OTHER = uuid(0, 1);
const TITLE = "Draft petition";

const row = (thread: string, status: ThreadStatus, title?: string): ThreadSummary => ({
  thread,
  matter: THE_MATTER.matter,
  agent: "claude-acp",
  status,
  ...(title === undefined ? {} : { title }),
});

const headerOf = (thread: string, status: ThreadStatus): ViewEntry => ({
  header: {
    order: 1,
    thread,
    matter: THE_MATTER.matter,
    agent: "claude-acp",
    status,
    config_options: [],
    commands: [],
    plan: [],
  },
});

const mountWith = (
  other: ThreadStatus,
  dispatch?: (c: HostCommand) => CommandOutcome | Promise<CommandOutcome>,
) =>
  mountRoom({
    threads: [row(THREAD, "idle", "Lease notice"), row(OTHER, other, TITLE)],
    ...(dispatch === undefined ? {} : { dispatch }),
  });

const wait = () => h!.one("other-wait");
const head = () =>
  wait()
    ?.querySelector(".queue-wait-head")
    ?.textContent?.replace(/\u00a0/g, " ");
const queueButton = () => h!.one("queue") as HTMLButtonElement | null;
const disabled = (b: HTMLButtonElement | null) =>
  b?.disabled === true || b?.getAttribute("aria-disabled") === "true";

test("queued behind another: the wait names its title with no time, and offers Stop the other, no Queue", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  expect(wait()?.textContent).not.toMatch(/\d|\bby\b|pm|am/);
  expect(h.one("stop-other")).not.toBeNull();
  expect(queueButton()).toBeNull();
});

test("the title is the one the host names in behind, though another conversation also runs", async () => {
  const THIRD = uuid(0x10, 1);
  h = await mountRoom({
    threads: [
      row(THREAD, "idle"),
      row(OTHER, "running", TITLE),
      row(THIRD, { queued: { behind: OTHER } }, "Reply to notice"),
    ],
  });
  // THIRD sorts above THREAD and opens: it waits behind OTHER, as the list says
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
});

/** Words in the field, then Ask's Enter: with another conversation holding the matter, this raises the wait. */
const QUESTION = "Does the lease allow a sublet?";
async function raise(text = QUESTION) {
  await h!.type(text);
  await h!.key({ key: "Enter", keyCode: 13 });
}
const field = () => h!.one("composer") as HTMLTextAreaElement;
const said = () => h!.one("other-wait-said")?.textContent ?? null;
const READY = "One conversation changes a matter's papers at a time. Your message is ready to go.";
/** What the composer's one status region says of its working line. */
const composerSaid = () =>
  h!.host.querySelector('[data-composer] .room-entry > [role="status"]')?.textContent ?? "";

test("before sending: Ask raises the wait while another runs, needs the lawyer or starts, and sends nothing", async () => {
  for (const status of ["running", "needs_you", "starting"] as const) {
    h = await mountWith(status);
    // nothing over an empty field: "Your message is ready to go." is true only once there is one
    expect(wait(), status).toBeNull();
    await raise();
    expect(head(), status).toBe(`Waiting for '${TITLE}' to finish.`);
    expect(queueButton(), status).not.toBeNull();
    expect(h.sent("send_prompt"), status).toEqual([]);
    // the words stay in the field until Queue
    expect(field().value, status).toBe(QUESTION);
    await h.unmount();
    h = null;
  }
});

test("with the other idle, Ask sends at once and no wait shows", async () => {
  h = await mountWith("idle");
  await raise();
  expect(wait()).toBeNull();
  expect(h.sent("send_prompt")).toHaveLength(1);
});

test("an exited agent's conversation, whose next message starts it again, raises the wait too", async () => {
  h = await mountRoom({ threads: [row(THREAD, "agent_exited"), row(OTHER, "running", TITLE)] });
  await raise();
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  expect(h.sent("send_prompt")).toEqual([]);
});

test("emptying the field lowers the raised wait", async () => {
  h = await mountWith("running");
  await raise();
  expect(wait()).not.toBeNull();
  await h.type("");
  expect(wait()).toBeNull();
});

test("Queue sends send_prompt for the open conversation exactly as Ask does, with no delivery (the wire's queue)", async () => {
  h = await mountWith("running");
  await raise();
  expect(disabled(queueButton())).toBe(false);
  await h.click(queueButton());
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }] },
  ]);
  // the words went, as after Ask
  expect(field().value).toBe("");
});

test("after Queue the focus is on Stop the other, never the page", async () => {
  h = await mountWith("running");
  await raise();
  queueButton()!.focus();
  await h.click(queueButton());
  expect(document.activeElement).toBe(h.one("stop-other"));
});

test("the wait stays mounted from Queue through starting to queued behind the other", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, "idle"))]);
  await h.settle();
  await raise();
  const node = wait();
  expect(node).not.toBeNull();
  await h.click(queueButton());
  expect(h.sent("send_prompt")).toHaveLength(1);
  expect(wait()).toBe(node);
  // the reducer's order: the user_message makes it starting, then the host's TurnQueued queued { behind }
  h.deliver([upsert(headerOf(THREAD, "starting"))]);
  await h.settle();
  expect(wait()).toBe(node);
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  expect(queueButton()).toBeNull();
  h.deliver([upsert(headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  expect(wait()).toBe(node);
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
});

test("once sent, the working line says the wait, never Answering.", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  const line = h.one("working")?.textContent?.replace(/\u00a0/g, " ") ?? "";
  expect(line).toContain(`Waiting for '${TITLE}' to finish.`);
  expect(line).not.toContain("Answering.");
  // and through the starting hand-off
  h.deliver([upsert(headerOf(THREAD, "starting"))]);
  await h.settle();
  expect(h.one("working")?.textContent).not.toContain("Answering.");
});

test("starting while the other is only starting too keeps the wait, so it doesn't go and come back", async () => {
  h = await mountWith("starting");
  h.deliver([reset([], headerOf(THREAD, "starting"))]);
  await h.settle();
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  expect(queueButton()).toBeNull();
});

test("starting with no other conversation holding shows no wait", async () => {
  h = await mountWith("idle");
  h.deliver([reset([], headerOf(THREAD, "starting"))]);
  await h.settle();
  expect(wait()).toBeNull();
});

test("the status region mounts empty and is filled as the wait comes; once queued the composer's line says it", async () => {
  h = await mountWith("running");
  await raise();
  expect(said()).toBe(`Waiting for '${TITLE}' to finish. ${READY}`);
  // no name on the section repeating the words
  expect(wait()?.getAttribute("aria-label")).toBeNull();
  await h.click(queueButton());
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  expect(said()).toBe("");
  expect(composerSaid()).toContain(`Waiting for '${TITLE}' to finish.`);
});

test("Stop the other sends cancel for the thread it waits behind, never for the open one", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  await h.click(h.one("stop-other"));
  expect(h.sent("cancel")).toEqual([{ thread: OTHER }]);
  expect(h.sent("send_prompt")).toEqual([]);
});

test("Stop the other before the message went stops the other, then sends the message (design's stopOther)", async () => {
  h = await mountWith("running");
  await raise();
  await h.click(h.one("stop-other"));
  expect(h.sent("cancel")).toEqual([{ thread: OTHER }]);
  // with the next frame's flush (#253)
  await act(async () => h!.store.flush());
  await h.settle();
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: QUESTION } }] },
  ]);
});

test("a second press while the cancel is on its way sends nothing", async () => {
  h = await mountWith("running", (command) =>
    command.cancel !== undefined ? new Promise<never>(() => {}) : defaultOutcome(command),
  );
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  await h.click(h.one("stop-other"));
  await h.click(h.one("stop-other"));
  expect(h.sent("cancel")).toHaveLength(1);
});

test("a refused Stop the other shows its code's words with the next frame's flush, never the host's message", async () => {
  h = await mountWith("running", (command) => {
    if (command.cancel !== undefined)
      throw { code: "command.unknown_thread", message: "raw host text", retryable: false };
    return defaultOutcome(command);
  });
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  await h.click(h.one("stop-other"));
  // the reply has landed, and its set waits for the frame: no commit of its own beside the stream's (#253)
  expect(h.one("error")).toBeNull();
  await act(async () => h!.store.flush());
  expect(h.one("error")?.textContent).toContain(errorWords("en", "command.unknown_thread"));
  expect(h.host.textContent).not.toContain("raw host text");
});

test("the other conversation's status changing renders no row of the open transcript", async () => {
  h = await mountWith("idle");
  h.deliver([
    reset(
      [userMessage(1, 1, "Read clause nine."), agentText(2, 1, "Clause nine says notice.")],
      headerOf(THREAD, "idle"),
    ),
  ]);
  await h.settle();
  // the other's view held, as matters.ts holds a conversation left while it works
  const otherStream = `thread:${OTHER}` as StreamId;
  let release = () => {};
  act(() => {
    release = h!.store.watch(otherStream);
  });
  expect(rowCommits.size).toBeGreaterThan(0);
  rowCommits.clear();
  h.deliverOn(otherStream, [reset([], headerOf(OTHER, "running"))]);
  await h.settle();
  await raise();
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  h.deliverOn(otherStream, [reset([], headerOf(OTHER, "idle"))]);
  await h.settle();
  expect(wait()).toBeNull();
  expect([...rowCommits.entries()]).toEqual([]);
  act(() => release());
});
