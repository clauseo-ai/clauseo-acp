// @vitest-environment happy-dom
// U7's matters/other in the real room (room.tsx over FakeTransport), as reworked for parallel answers (issue 15, 24
// Nov): the conversations of a matter answer at the same time, so another conversation running, starting or waiting
// on the lawyer never holds a message back: Ask sends at once, and no wait, Stop the other or Queue shows. The wait
// over the composer shows only while the host has queued the turn (`queued { behind }`), which it does while an undo,
// a restore or a relocate holds the matter: it names no conversation and offers nothing to press. A change of the
// other's status renders no row of the open transcript.
import { act, createElement, memo, Profiler, type ProfilerOnRenderCallback } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { agentText, reset, THREAD, upsert, userMessage, uuid } from "../testing/stream-script.ts";
import type {
  EntryId,
  StreamId,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

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

const mountWith = (other: ThreadStatus, own: ThreadStatus = "idle") =>
  mountRoom({ threads: [row(THREAD, own, "Lease notice"), row(OTHER, other, TITLE)] });

/** The wait over the composer, while the host holds the open conversation's turn for the matter. */
const wait = () => h!.one("matter-hold");
/** The composer's working line's words. */
const working = () =>
  h!
    .one("working")
    ?.textContent?.replace(/\u00a0/g, " ")
    .trim();
const QUESTION = "Does the lease allow a sublet?";
async function ask(text = QUESTION) {
  await h!.type(text);
  await h!.key({ key: "Enter", keyCode: 13 });
}
const HOLD =
  "An undo, restore or move is changing this matter's papers. Your message is queued and sends once that's done.";
/** What the composer's one status region says of its working line. */
const composerSaid = () =>
  h!.host.querySelector('[data-composer] .room-entry > [role="status"]')?.textContent ?? "";
const rowTitled = (title: string) =>
  h!.all("conversation").find((b) => b.querySelector(".room-conv-title")?.textContent === title);
/** The words a row says to assistive technology, or under its title. */
const rowSays = (title: string) =>
  (rowTitled(title)?.querySelector(".sr-only, .room-conv-said")?.textContent ?? "").trim();

test("while another conversation answers, needs the lawyer or starts, Ask sends at once: no wait, Stop the other or Queue", async () => {
  for (const status of ["running", "needs_you", "starting"] as const) {
    h = await mountWith(status);
    await ask();
    expect(h.sent("send_prompt"), status).toEqual([
      { thread: THREAD, content: [{ text: { text: QUESTION } }] },
    ]);
    expect(wait(), status).toBeNull();
    expect(h.host.textContent, status).not.toMatch(
      /Waiting for|Stop the other|One conversation changes/,
    );
    expect(
      h.host.querySelector('[data-testid="queue"], [data-testid="stop-other"]'),
      status,
    ).toBeNull();
    await h.unmount();
    h = null;
  }
});

test("an exited agent's conversation sends at once too, while another answers", async () => {
  h = await mountWith("running", "agent_exited");
  await ask();
  expect(h.sent("send_prompt")).toHaveLength(1);
  expect(wait()).toBeNull();
});

test("two conversations answering at once: the open one's working line says Answering., the other's row says it too", async () => {
  h = await mountWith("running", "running");
  h.deliver([reset([userMessage(1, 1, "Read clause nine.")], headerOf(THREAD, "running"))]);
  await h.settle();
  expect(working()).toContain("Answering.");
  expect(wait()).toBeNull();
  // each row carries its own conversation's state: the open one none (its state is in the room), the other answering
  expect(rowTitled("Lease notice")?.getAttribute("aria-current")).toBe("true");
  expect(rowSays(TITLE)).toBe("Answering.");
  await h.click(rowTitled(TITLE));
  expect(rowSays("Lease notice")).toBe("Answering.");
});

test("queued by the host for the matter: the working line says Waiting to answer., the card says why, nothing to press, no name and no time", async () => {
  for (const behind of [THREAD, OTHER]) {
    h = await mountWith("idle");
    h.deliver([reset([], headerOf(THREAD, { queued: { behind } }))]);
    await h.settle();
    expect(working()).toBe("Waiting to answer.");
    expect(working()).not.toContain("Answering.");
    expect(wait()?.textContent).toBe(HOLD);
    expect(wait()?.querySelector("button")).toBeNull();
    expect(h.host.textContent).not.toContain(TITLE + "'");
    expect(wait()?.textContent).not.toMatch(/\d/);
    // the composer's own region says the wait; the card holds none of its own
    expect(composerSaid()).toContain("Waiting to answer.");
    expect(wait()?.querySelector('[role="status"]')).toBeNull();
    // the waiting mark is on the waiting (open) row, and the other row's says nothing of a wait
    expect(rowTitled("Lease notice")?.dataset["state"]).toBe("waiting");
    expect(rowSays("Lease notice")).toBe("Waiting to answer.");
    expect(rowSays(TITLE)).not.toContain("Waiting");
    await h.unmount();
    h = null;
  }
});

test("a row queued for the matter says Waiting to answer. on the rail, naming no conversation", async () => {
  h = await mountWith({ queued: { behind: OTHER } });
  expect(rowTitled(TITLE)?.dataset["state"]).toBe("waiting");
  expect(rowSays(TITLE)).toBe("Waiting to answer.");
});

test("the wait goes once the turn starts", async () => {
  h = await mountWith("idle");
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: THREAD } }))]);
  await h.settle();
  expect(wait()).not.toBeNull();
  h.deliver([upsert(headerOf(THREAD, "running"))]);
  await h.settle();
  expect(wait()).toBeNull();
  expect(working()).toContain("Answering.");
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
  expect(wait()).toBeNull();
  h.deliverOn(otherStream, [reset([], headerOf(OTHER, "idle"))]);
  await h.settle();
  expect([...rowCommits.entries()]).toEqual([]);
  act(() => release());
});
