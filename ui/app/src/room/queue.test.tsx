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
  dispatch?: (c: HostCommand) => ReturnType<typeof defaultOutcome>,
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

test("before sending: shown while another runs, needs the lawyer or starts, and not while it is idle", async () => {
  for (const status of ["running", "needs_you", "starting"] as const) {
    h = await mountWith(status);
    expect(head(), status).toBe(`Waiting for '${TITLE}' to finish.`);
    expect(queueButton(), status).not.toBeNull();
    await h.unmount();
    h = null;
  }
  h = await mountWith("idle");
  expect(wait()).toBeNull();
  expect(queueButton()).toBeNull();
});

test("Queue waits for words, then sends send_prompt for the open conversation exactly as Ask does", async () => {
  h = await mountWith("running");
  expect(disabled(queueButton())).toBe(true);
  await h.click(queueButton());
  expect(h.sent("send_prompt")).toEqual([]);
  await h.type("Does the lease allow a sublet?");
  expect(disabled(queueButton())).toBe(false);
  await h.click(queueButton());
  expect(h.sent("send_prompt")).toEqual([
    { thread: THREAD, content: [{ text: { text: "Does the lease allow a sublet?" } }] },
  ]);
  // the words went, as after Ask: the field is empty and Queue waits for words again
  expect((h.one("composer") as HTMLTextAreaElement).value).toBe("");
});

test("the wait stays mounted from Queue through starting to queued behind the other", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, "idle"))]);
  await h.settle();
  const node = wait();
  expect(node).not.toBeNull();
  await h.type("Does the lease allow a sublet?");
  await h.click(queueButton());
  expect(h.sent("send_prompt")).toHaveLength(1);
  expect(wait()).toBe(node);
  // the reducer's order: the user_message makes it starting, then the host's TurnQueued queued { behind }
  h.deliver([upsert(headerOf(THREAD, "starting"))]);
  await h.settle();
  expect(wait()).toBe(node);
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  expect(queueButton()).toBeNull();
  await h.click(h.one("stop-other"));
  expect(h.sent("cancel")).toEqual([{ thread: OTHER }]);
  h.deliver([upsert(headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  expect(wait()).toBe(node);
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
});

test("starting with no other conversation running shows no wait", async () => {
  h = await mountWith("idle");
  h.deliver([reset([], headerOf(THREAD, "starting"))]);
  await h.settle();
  expect(wait()).toBeNull();
});

test("Stop the other sends cancel for the thread it waits behind, never for the open one", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  await h.click(h.one("stop-other"));
  expect(h.sent("cancel")).toEqual([{ thread: OTHER }]);
});

test("a refused Stop the other shows its code's words, never the host's message", async () => {
  h = await mountWith("running", (command) => {
    if (command.cancel !== undefined)
      throw { code: "command.unknown_thread", message: "raw host text", retryable: false };
    return defaultOutcome(command);
  });
  await h.click(h.one("stop-other"));
  // the reply's set waits for the next frame's flush (#253)
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
  expect(head()).toBe(`Waiting for '${TITLE}' to finish.`);
  h.deliverOn(otherStream, [reset([], headerOf(OTHER, "idle"))]);
  await h.settle();
  expect(wait()).toBeNull();
  expect([...rowCommits.entries()]).toEqual([]);
  act(() => release());
});
