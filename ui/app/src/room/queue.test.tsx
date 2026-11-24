// @vitest-environment happy-dom
// U7's matters/other in the real room (room.tsx over FakeTransport), as reworked for parallel answers (issue 15, 24
// Nov): the conversations of a matter answer at the same time, so another conversation running, starting or waiting
// on the lawyer never holds a message back: Ask sends at once, and no wait, Stop the other or Queue shows. The wait
// over the composer shows only while the host has queued the turn (`queued { behind }`), which it does while an undo,
// a restore or a relocate holds the matter: it names no conversation and offers nothing to press. A change of the
// other's status renders no row of the open transcript.
import { act, createElement, memo, Profiler, type ProfilerOnRenderCallback } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  reset,
  step,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type {
  EntryId,
  MatterHold,
  StreamId,
  ThreadStatus,
  ThreadSummary,
  ViewEntry,
} from "../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";
import { forgetSteerRestores } from "./thread/steer-restore.ts";
import { forgetUnsent } from "./thread/unsent-restore.ts";
import { fakeNext } from "./fake-next.ts";

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
  forgetSteerRestores();
  forgetUnsent();
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

/** The host's app events (ThreadsChanged), alone of the rows today's host lacks. */
const APP = fakeNext({ matters: () => [], parts: ["app"] });
const mountWith = (other: ThreadStatus, own: ThreadStatus = "idle") =>
  mountRoom({ threads: [row(THREAD, own, "Lease notice"), row(OTHER, other, TITLE)], next: APP });

/** The wait over the composer, while the host holds the open conversation's turn for the matter. */
const wait = () => h!.one("matter-hold");
/** The composer's working line's words. */
const working = () =>
  h!
    .one("working")
    ?.textContent?.replace(/\u00a0/g, " ")
    .trim();
const QUESTION = "Does the lease allow a sublet?";
const field = () => h!.one("composer") as HTMLTextAreaElement;
async function ask(text = QUESTION) {
  await h!.type(text);
  await h!.key({ key: "Enter", keyCode: 13 });
}
/** The card's words for each hold the host names (TurnQueued.hold, folded into HeaderEntry.hold). */
const CARDS = {
  undo: "An undo is changing this matter's papers. Your message is queued and sends once it's done.",
  restore:
    "A restore is changing this matter's papers. Your message is queued and sends once it's done.",
  relocate:
    "This matter's folder is being moved. Your message is queued and sends once that's done.",
} as const;
const OTHER_ANSWER = "Waiting for another answer in this matter.";
/** THREAD's header queued with the host's hold (HeaderEntry.hold), behind its own thread as the host logs it: the
 * message's own status stays sent. */
const heldHeader = (hold: MatterHold): ViewEntry => {
  const e = headerOf(THREAD, { queued: { behind: THREAD } });
  return { header: { ...e.header!, hold } };
};
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

test("queued with the host's hold: the card names that hold; the working line says Waiting to answer.; nothing to press", async () => {
  for (const hold of ["undo", "restore", "relocate"] as const) {
    h = await mountWith("idle");
    h.deliver([reset([], heldHeader(hold))]);
    await h.settle();
    expect(working(), hold).toBe("Waiting to answer.");
    expect(wait()?.textContent, hold).toBe(CARDS[hold]);
    expect(wait()?.querySelector("button"), hold).toBeNull();
    // the composer's own region says the wait; the card holds none of its own
    expect(composerSaid(), hold).toContain("Waiting to answer.");
    expect(wait()?.querySelector('[role="status"]'), hold).toBeNull();
    // the waiting mark is on the waiting (open) row, and the other row's says nothing of a wait
    expect(rowTitled("Lease notice")?.dataset["state"], hold).toBe("waiting");
    expect(rowSays("Lease notice"), hold).toBe("Waiting to answer.");
    expect(rowSays(TITLE), hold).not.toContain("Waiting");
    await h.unmount();
    h = null;
  }
});

test("an older host's log: queued behind another conversation with no hold says another answer, never an undo, restore or move", async () => {
  h = await mountWith("running");
  h.deliver([reset([], headerOf(THREAD, { queued: { behind: OTHER } }))]);
  await h.settle();
  expect(working()).toBe("Waiting to answer.");
  expect(wait()?.textContent).toBe(OTHER_ANSWER);
  expect(h.host.textContent).not.toContain("changing this matter's papers");
  expect(h.host.textContent).not.toContain("being moved");
  expect(h.host.textContent).not.toContain(`'${TITLE}'`);
});

test("a missing or unknown hold behind its own thread: the working line alone, no card that names a reason", async () => {
  for (const head of [headerOf(THREAD, { queued: { behind: THREAD } }), heldHeader("unknown")]) {
    h = await mountWith("idle");
    h.deliver([reset([], head)]);
    await h.settle();
    expect(working()).toBe("Waiting to answer.");
    expect(wait()).toBeNull();
    await h.unmount();
    h = null;
  }
});

test("an unknown hold behind another conversation: the working line alone, never another answer in the matter", async () => {
  h = await mountWith("idle");
  const e = headerOf(THREAD, { queued: { behind: OTHER } });
  h.deliver([reset([], { header: { ...e.header!, hold: "unknown" } })]);
  await h.settle();
  expect(working()).toBe("Waiting to answer.");
  expect(wait()).toBeNull();
  expect(composerSaid()).not.toContain(OTHER_ANSWER);
});

test("a row queued for the matter says Waiting to answer. on the rail, naming no conversation", async () => {
  h = await mountWith({ queued: { behind: OTHER } });
  expect(rowTitled(TITLE)?.dataset["state"]).toBe("waiting");
  expect(rowSays(TITLE)).toBe("Waiting to answer.");
});

test("the wait goes once the turn starts", async () => {
  h = await mountWith("idle");
  h.deliver([reset([], heldHeader("restore"))]);
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

// Stop on a message that waits (Windows test, 24 Nov; #1302 review): the host ends its turn cancelled before it
// started, and the reducer marks the message removed. Its words never vanish, and the line says only what is true.
const STOPPED =
  "Not sent, because you stopped it before it started. Your message is back in the box below.";
const NEUTRAL = "This message wasn't sent. It's back in the box below.";
type Status = "sent" | "removed" | "queued";
const asked = (order: number, n: number, text: string, status: Status): ViewEntry => {
  const e = userMessage(order, n, text);
  return { user_message: { ...e.user_message!, status, turn: uuid(n, 4) } };
};
const LATER = "And the sublet clause in the renewal?";
const ANSWERED = [
  userMessage(1, 1, "Read clause nine."),
  agentText(2, 1, "Clause nine says notice."),
];
const line = () =>
  h!
    .one("unsent")
    ?.textContent?.replace(/\u00a0/g, " ")
    .trim();
/** The host's ThreadsChanged after it (#1282): the emptied conversations are no longer listed. */
const leaves = (...removed: string[]) =>
  act(async () =>
    h!.transport.appEvent({
      threads_changed: { matter: THE_MATTER.matter, threads: [], removed, all: false },
    }),
  );
const openTitle = () =>
  h!
    .all("conversation")
    .find((b) => b.getAttribute("aria-current") === "true")
    ?.querySelector(".room-conv-title")?.textContent;

test("Stop on a later message that waits: its words come back into the same conversation's field, because you stopped it", async () => {
  h = await mountWith("idle");
  h.deliver([reset([...ANSWERED, asked(3, 2, LATER, "sent")], heldHeader("undo"))]);
  await h.settle();
  await h.click(h.one("cancel"));
  expect(h.sent("cancel")).toEqual([{ thread: THREAD }]);
  h.deliver([upsert(asked(3, 2, LATER, "removed")), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  expect(field().value).toBe(LATER);
  expect(line()).toBe(STOPPED);
  expect(rowTitled("Lease notice")?.getAttribute("aria-current")).toBe("true");
  expect(h.host.textContent).toContain("Clause nine says notice.");
  // the next send clears the line
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt")).toHaveLength(1);
  expect(h.one("unsent")).toBeNull();
});

test("a message ended before it started with no Stop in this window: the words come back, with neutral words", async () => {
  h = await mountWith("idle");
  h.deliver([reset([...ANSWERED, asked(3, 2, LATER, "sent")], heldHeader("restore"))]);
  await h.settle();
  h.deliver([upsert(asked(3, 2, LATER, "removed")), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  expect(field().value).toBe(LATER);
  expect(line()).toBe(NEUTRAL);
});

test("words typed meanwhile keep their place: the stopped message comes after them", async () => {
  h = await mountWith("idle");
  h.deliver([reset([...ANSWERED, asked(3, 2, LATER, "sent")], heldHeader("undo"))]);
  await h.settle();
  await h.type("Also the deposit.");
  h.deliver([upsert(asked(3, 2, LATER, "removed"))]);
  await h.settle();
  expect(field().value).toBe(`Also the deposit.\n\n${LATER}`);
});

test("Stop on one of two waiting messages: only the stopped one comes back; the other stays queued", async () => {
  const NEXT = "Then list the annexures.";
  h = await mountWith("idle");
  h.deliver([
    reset(
      [...ANSWERED, asked(3, 2, LATER, "sent"), asked(4, 3, NEXT, "queued")],
      heldHeader("undo"),
    ),
  ]);
  await h.settle();
  await h.click(h.one("cancel"));
  h.deliver([upsert(asked(3, 2, LATER, "removed"))]);
  await h.settle();
  expect(field().value).toBe(LATER);
  expect(field().value).not.toContain(NEXT);
  expect(line()).toBe(STOPPED);
});

test("the only message removed, with no removal from the host: no new draft opens, the conversation stays open", async () => {
  h = await mountWith("idle");
  h.deliver([reset([asked(1, 2, QUESTION, "sent")], heldHeader("undo"))]);
  await h.settle();
  h.deliver([upsert(asked(1, 2, QUESTION, "removed")), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  expect(openTitle()).toBe("Lease notice");
  expect(h.all("conversation").some((b) => b.textContent?.includes("Not started yet"))).toBe(false);
});

test("the only message stopped, then the host's removal: a new draft holds its words, with the focus and the line, and its row goes", async () => {
  h = await mountWith("idle");
  h.deliver([reset([asked(1, 2, QUESTION, "sent")], heldHeader("undo"))]);
  await h.settle();
  await h.click(h.one("cancel"));
  h.deliver([upsert(asked(1, 2, QUESTION, "removed")), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  await leaves(THREAD);
  await h.settle();
  expect(openTitle()).toBe("Not started yet");
  expect(field().value).toBe(QUESTION);
  expect(document.activeElement).toBe(field());
  expect(line()).toBe(STOPPED);
  expect(rowTitled("Lease notice")).toBeUndefined();
  expect(rowTitled(TITLE)).not.toBeUndefined();
});

test("the host's removal before the stream says the message was removed: the words go to a new draft, once", async () => {
  h = await mountWith("idle");
  h.deliver([reset([asked(1, 2, QUESTION, "sent")], heldHeader("undo"))]);
  await h.settle();
  await h.click(h.one("cancel"));
  await leaves(THREAD);
  await h.settle();
  expect(openTitle()).toBe("Not started yet");
  expect(field().value).toBe(QUESTION);
  expect(line()).toBe(STOPPED);
});

test("Stop, then switching to another conversation before the host's removal: the words wait in the matter's draft", async () => {
  h = await mountWith("idle");
  h.deliver([reset([asked(1, 2, QUESTION, "sent")], heldHeader("undo"))]);
  await h.settle();
  await h.click(h.one("cancel"));
  await h.click(rowTitled(TITLE));
  expect(openTitle()).toBe(TITLE);
  await leaves(THREAD);
  await h.settle();
  // the lawyer stays where they went; the draft row holds the words
  expect(openTitle()).toBe(TITLE);
  const draft = h.all("conversation").find((b) => b.textContent?.includes("Not started yet"));
  expect(draft).not.toBeUndefined();
  await h.click(draft);
  expect(field().value).toBe(QUESTION);
  expect(line()).toBe(STOPPED);
});

test("two conversations answering, stopped one after the other: each Stop cancels its own turn only", async () => {
  h = await mountWith("running", "running");
  h.deliver([reset([userMessage(1, 1, "Read clause nine.")], headerOf(THREAD, "running"))]);
  await h.settle();
  await h.click(h.one("cancel"));
  expect(h.sent("cancel")).toEqual([{ thread: THREAD }]);
  expect(rowSays(TITLE)).toBe("Answering.");
  await h.click(rowTitled(TITLE));
  h.deliverOn(`thread:${OTHER}` as StreamId, [
    reset([userMessage(1, 2, "Draft it.")], headerOf(OTHER, "running")),
  ]);
  await h.settle();
  await h.click(h.one("cancel"));
  expect(h.sent("cancel")).toEqual([{ thread: THREAD }, { thread: OTHER }]);
});

test("a message already removed when the conversation opens stays as it is: nothing comes back", async () => {
  h = await mountWith("idle");
  h.deliver([reset([...ANSWERED, asked(3, 2, LATER, "removed")], headerOf(THREAD, "idle"))]);
  await h.settle();
  expect(field().value).toBe("");
  expect(h.one("unsent")).toBeNull();
});

/** The rail's New conversation: the matter's draft opens. */
const newDraft = () =>
  h!.click(h!.host.querySelector<HTMLButtonElement>('[data-testid="new-conversation"]'));
const draftRow = () =>
  h!.all("conversation").find((b) => b.textContent?.includes("Not started yet"));
/** The only message of THREAD waiting for an undo, and this window's Stop pressed on it. */
async function stoppedOnly() {
  h!.deliver([reset([asked(1, 2, QUESTION, "sent")], heldHeader("undo"))]);
  await h!.settle();
  await h!.click(h!.one("cancel"));
}

test("typing in the matter's draft as the host removes the stopped conversation: the words join the field, after the typed ones", async () => {
  h = await mountWith("idle");
  await stoppedOnly();
  await newDraft();
  expect(openTitle()).toBe("Not started yet");
  await h.type("My own note.");
  await leaves(THREAD);
  await h.settle();
  expect(field().value).toBe(`My own note.\n\n${QUESTION}`);
  expect(line()).toBe(STOPPED);
  // the next keystroke keeps them: the field holds them, and the draft follows the field
  await h.type(`${field().value} Thanks.`);
  await h.click(rowTitled(TITLE));
  await h.click(draftRow());
  expect(field().value).toBe(`My own note.\n\n${QUESTION} Thanks.`);
});

test("a draft with typed words, not open as the removal comes: the words merge after them", async () => {
  h = await mountWith("idle");
  await stoppedOnly();
  await newDraft();
  await h.type("Typed first.");
  await h.click(rowTitled(TITLE));
  await leaves(THREAD);
  await h.settle();
  expect(openTitle()).toBe(TITLE);
  await h.click(draftRow());
  expect(field().value).toBe(`Typed first.\n\n${QUESTION}`);
  expect(line()).toBe(STOPPED);
});

test("a conversation removed in another matter: its words land in that matter's draft, not the open matter's", async () => {
  const B = { matter: uuid(9, 2), name: "Mehta v. Union", kind: "folder" as const };
  const THIRD = uuid(9, 1);
  h = await mountRoom({
    matters: [THE_MATTER, B],
    threads: (matter) =>
      matter === B.matter
        ? [
            {
              thread: THIRD,
              matter: B.matter,
              agent: "claude-acp",
              status: "idle",
              title: "Bail note",
            },
          ]
        : [row(THREAD, "idle", "Lease notice"), row(OTHER, "idle", TITLE)],
    next: APP,
  });
  const matterRow = (name: string) =>
    h!.all("matter-row").find((r) => r.textContent?.includes(name));
  if (h.one("matter")?.textContent !== THE_MATTER.name) await h.click(matterRow(THE_MATTER.name));
  await stoppedOnly();
  await h.click(matterRow(B.name));
  expect(h.one("matter")?.textContent).toBe(B.name);
  await leaves(THREAD);
  await h.settle();
  // nothing in the open matter: its field and rows are as they were
  expect(field().value).toBe("");
  expect(draftRow()).toBeUndefined();
  await h.click(matterRow(THE_MATTER.name));
  await h.click(draftRow());
  expect(field().value).toBe(QUESTION);
  expect(line()).toBe(STOPPED);
});

test("Stop of a running answer, then a later message ended before it started with no Stop: the neutral line", async () => {
  h = await mountWith("idle");
  h.deliver([reset([userMessage(1, 1, "Read clause nine.")], headerOf(THREAD, "running"))]);
  await h.settle();
  await h.click(h.one("cancel"));
  // the answer ends: the Stop has nothing left to say of
  h.deliver([upsert(step(3, { stop: "cancelled" })), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  h.deliver([upsert(asked(4, 2, LATER, "sent")), upsert(heldHeader("restore"))]);
  await h.settle();
  h.deliver([upsert(asked(4, 2, LATER, "removed")), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  expect(field().value).toBe(LATER);
  expect(line()).toBe(NEUTRAL);
});

test("the store dropped the conversation's view before the host's removal: the words seen while it was open come back", async () => {
  h = await mountWith("idle");
  h.deliver([reset([asked(1, 2, QUESTION, "sent")], heldHeader("undo"))]);
  await h.settle();
  h.deliver([upsert(asked(1, 2, QUESTION, "removed")), upsert(headerOf(THREAD, "idle"))]);
  await h.settle();
  // the lawyer leaves the resting conversation: its view goes from the store
  await h.click(rowTitled(TITLE));
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  expect(h.store.view(`thread:${THREAD}` as StreamId)).toBeUndefined();
  await leaves(THREAD);
  await h.settle();
  await h.click(draftRow());
  expect(field().value).toBe(QUESTION);
  expect(line()).toBe(NEUTRAL);
});
