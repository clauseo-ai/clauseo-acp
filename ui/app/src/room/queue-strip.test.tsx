// @vitest-environment happy-dom
// U7's queue over the composer (queue-slot.tsx, queue-strip.tsx) over FakeTransport: the real room (harness.ts) for
// the store, the stream and the transcript, and QueueSlot mounted beside it in a root of its own inside the same store
// (the composer mounts the slot on U7b-composer). Queued entries fill the strip in order; Edit, Remove, Send now and
// Send next send their H9 commands for the right turn, one at a time per row; a refusal's words show with the frame's
// flush and go once the queue changes; the held heads; the cap line; nothing over an empty, unheld queue; focus after a
// row goes; and a strip change renders no row of the transcript beyond the queued entries' own, one commit per frame.
import { act, createElement, memo, Profiler, type ProfilerOnRenderCallback } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { StoreContext } from "../store/hooks.ts";
import {
  agentText,
  header,
  reset,
  step,
  STREAM,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type {
  CommandOutcome,
  EntryId,
  HostCommand,
  StreamId,
  ViewEntry,
} from "../transport/protocol.ts";
import { loadQueueStrip } from "./queue-chunk.ts";
import { QueueSlot } from "./queue-slot.tsx";
import { clockTime } from "./thread/clock.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

// each transcript row inside its own Profiler, behind a memo like the real row's (queue.test.tsx's way)
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
let host: HTMLDivElement | null = null;
let root: Root | null = null;
let stripCommits = 0;
let fieldFocused = 0;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  await h?.unmount();
  h = null;
  rowCommits.clear();
  stripCommits = 0;
  fieldFocused = 0;
});

async function mount(dispatch?: (c: HostCommand) => CommandOutcome | Promise<CommandOutcome>) {
  h = await mountRoom(dispatch === undefined ? {} : { dispatch });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const transport = h.transport;
  await act(async () =>
    root!.render(
      createElement(
        StoreContext,
        { value: h!.store },
        createElement(
          Profiler,
          { id: "strip", onRender: () => (stripCommits += 1) },
          createElement(QueueSlot, {
            transport,
            stream: STREAM as StreamId,
            thread: THREAD,
            onFocusField: () => (fieldFocused += 1),
          }),
        ),
      ),
    ),
  );
  // the slot started the strip's chunk as it mounted
  await act(async () => void (await loadQueueStrip()));
}

const queued = (order: number, n: number, text: string): ViewEntry => ({
  user_message: {
    ...userMessage(order, n, text).user_message!,
    status: "queued",
    turn: uuid(n, 4),
  },
});
const as = (entry: ViewEntry, over: Record<string, unknown>): ViewEntry => ({
  user_message: { ...entry.user_message!, ...over },
});
const held = (status: "idle" | "running" = "idle"): ViewEntry => ({
  header: { ...header(status).header!, queue_held: true },
});

const one = (id: string) => host!.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const all = (id: string) => [...host!.querySelectorAll<HTMLElement>(`[data-testid="${id}"]`)];
const txt = (n: Element | null | undefined) => (n?.textContent ?? "").replace(/\u00a0/g, " ");
/** The head's own line, without a refused press's words. */
const say = () =>
  txt(one("queue-say")).slice(0, txt(one("queue-say")).length - txt(one("queue-error")).length);
const rows = () => all("queued");
const inRow = (i: number, id: string) =>
  rows()[i]?.querySelector<HTMLElement>(`[data-testid="${id}"]`) ?? null;
const flush = () => act(async () => h!.store.flush());

test("queued entries fill the strip in the order they go, each row naming its turn", async () => {
  await mount();
  h!.deliver([
    reset(
      [
        userMessage(1, 1, "Q1"),
        queued(5, 2, "Second."),
        queued(6, 3, "Third."),
        queued(7, 4, "Fourth."),
      ],
      header("running"),
    ),
  ]);
  await h!.settle();
  expect(say()).toBe("3 queued, sent in order.");
  await act(async () => host!.querySelector<HTMLButtonElement>(".queue-fold")?.click());
  expect(rows().map((r) => [r.dataset["turn"], txt(r.querySelector(".queue-text"))])).toEqual([
    [uuid(2, 4), "Second."],
    [uuid(3, 4), "Third."],
    [uuid(4, 4), "Fourth."],
  ]);
});

test("nothing shows over an empty queue that isn't held, nor over a held one with nothing in it", async () => {
  await mount();
  h!.deliver([reset([userMessage(1, 1, "Q1")], header("running"))]);
  await h!.settle();
  expect(host!.innerHTML).toBe("");
  h!.deliver([upsert(held())]);
  await h!.settle();
  expect(host!.querySelector("section")).toBeNull();
});

test("Edit sends the new text alone for its turn; a message with a file shows its words", async () => {
  await mount();
  const withFile = as(queued(5, 2, "Check the lease."), {
    content: [
      { text: { text: "Check the lease." } },
      { resource_link: { uri: "file:///m/Lease.pdf", name: "Lease.pdf" } },
    ],
  });
  h!.deliver([reset([userMessage(1, 1, "Q1"), withFile], header("running"))]);
  await h!.settle();
  expect(txt(rows()[0]?.querySelector(".queue-text"))).toBe("Check the lease.");
  await h!.click(inRow(0, "queued-edit"));
  const field = host!.querySelector<HTMLTextAreaElement>(".queue-edit")!;
  await act(async () => {
    Reflect.set(HTMLTextAreaElement.prototype, "value", "Check the lease and the deed.", field);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const save = [...host!.querySelectorAll("button")].find((b) => txt(b) === "Save");
  await h!.click(save);
  expect(h!.sent("edit_queued_prompt")).toEqual([
    { thread: THREAD, turn: uuid(2, 4), text: "Check the lease and the deed." },
  ]);
});

test("Remove, Send now and Send next send their commands with the right turn", async () => {
  await mount();
  h!.deliver([
    reset(
      [
        userMessage(1, 1, "Q1"),
        step(2, { stop: "cancelled" }),
        queued(5, 2, "A."),
        queued(6, 3, "B."),
      ],
      held(),
    ),
  ]);
  await h!.settle();
  await h!.click(inRow(1, "queued-remove"));
  await h!.click(inRow(0, "queued-send-now"));
  await h!.click(one("send-next"));
  expect(h!.sent("remove_queued_prompt")).toEqual([{ thread: THREAD, turn: uuid(3, 4) }]);
  expect(h!.sent("send_queued_now")).toEqual([{ thread: THREAD, turn: uuid(2, 4) }]);
  expect(h!.sent("resume_queue")).toEqual([{ thread: THREAD }]);
});

test("one command per row at a time: a second press while the first is on its way sends nothing", async () => {
  await mount((c) =>
    c.remove_queued_prompt !== undefined ? new Promise<never>(() => {}) : defaultOutcome(c),
  );
  h!.deliver([reset([queued(5, 2, "A."), queued(6, 3, "B.")], header("running"))]);
  await h!.settle();
  await h!.click(inRow(0, "queued-remove"));
  await h!.click(inRow(0, "queued-remove"));
  // another row's press still goes
  await h!.click(inRow(1, "queued-remove"));
  expect(h!.sent("remove_queued_prompt")).toEqual([
    { thread: THREAD, turn: uuid(2, 4) },
    { thread: THREAD, turn: uuid(3, 4) },
  ]);
});

test("turn_not_queued says the message had already gone, with the frame's flush, then the strip follows the view", async () => {
  await mount((c) => {
    if (c.send_queued_now !== undefined)
      throw { code: "thread.turn_not_queued", message: "raw host text", retryable: false };
    return defaultOutcome(c);
  });
  h!.deliver([reset([queued(5, 2, "A."), queued(6, 3, "B.")], header("running"))]);
  await h!.settle();
  const before = stripCommits;
  await h!.click(inRow(0, "queued-send-now"));
  // the reply has landed, and its words wait for the frame
  expect(one("queue-error")).toBeNull();
  expect(stripCommits).toBe(before);
  await flush();
  expect(stripCommits).toBe(before + 1);
  expect(txt(one("queue-error"))).toBe("That message had already gone.");
  expect(one("queue-error")?.dataset["code"]).toBe("thread.turn_not_queued");
  expect(host!.textContent).not.toContain("raw host text");
  // the message went: the view says sent, and the strip follows
  h!.deliver([upsert(as(queued(5, 2, "A."), { status: "sent" }))]);
  await h!.settle();
  expect(one("queue-error")).toBeNull();
  expect(rows().map((r) => r.dataset["turn"])).toEqual([uuid(3, 4)]);
  expect(say()).toBe("1 queued, sent once this answer is done.");
});

test("another refusal shows its code's words until the next press, never the host's message", async () => {
  let refuse = true;
  await mount((c) => {
    if (c.remove_queued_prompt !== undefined && refuse)
      throw { code: "thread.not_found", message: "raw host text", retryable: false };
    return defaultOutcome(c);
  });
  h!.deliver([reset([queued(5, 2, "A."), queued(6, 3, "B.")], header("running"))]);
  await h!.settle();
  await h!.click(inRow(1, "queued-remove"));
  await flush();
  expect(txt(one("queue-error"))).toBe(errorWords("en", "thread.not_found"));
  expect(host!.textContent).not.toContain("raw host text");
  refuse = false;
  await h!.click(inRow(1, "queued-remove"));
  expect(one("queue-error")).toBeNull();
});

test("at ten the head says the queue holds ten", async () => {
  await mount();
  h!.deliver([
    reset(
      Array.from({ length: 10 }, (_, i) => queued(5 + i, 2 + i, `Message ${i + 1}.`)),
      header("running"),
    ),
  ]);
  await h!.settle();
  expect(say()).toBe("10 queued, sent in order. The queue holds 10.");
});

test("held after Stop: the step's time and Send next", async () => {
  await mount();
  const ts = Date.UTC(2026, 10, 3, 11, 2);
  h!.deliver([
    reset(
      [userMessage(1, 1, "Q1"), step(2, { stop: "cancelled", ts }), queued(5, 2, "A.")],
      held(),
    ),
  ]);
  await h!.settle();
  expect(say()).toBe(
    `The queue is held. You stopped the answer at ${clockTime("en", ts)}, so nothing more is sent until you send.`,
  );
  expect(txt(one("send-next"))).toBe("Send next");
});

test("held after a failure: the failed line and Send next", async () => {
  await mount();
  const ts = Date.UTC(2026, 10, 3, 11, 2);
  h!.deliver([
    reset(
      [
        userMessage(1, 1, "Q1"),
        step(2, { stop: "failed", failure: "agent.exited", ts }),
        queued(5, 2, "A."),
      ],
      held(),
    ),
  ]);
  await h!.settle();
  expect(say()).toBe(
    `The queue is held. The answer failed at ${clockTime("en", ts)}, so nothing more is sent until you send.`,
  );
  expect(one("send-next")).not.toBeNull();
});

test("held after a restart (no step): the count line and Send next", async () => {
  await mount();
  h!.deliver([reset([queued(5, 2, "A."), queued(6, 3, "B.")], held())]);
  await h!.settle();
  expect(say()).toBe("2 queued, sent in order.");
  expect(one("send-next")).not.toBeNull();
  expect(host!.querySelector(".queue")?.hasAttribute("data-held")).toBe(true);
});

test("after Remove takes a row away the focus goes to the next row's Send now; after the last, to the field", async () => {
  await mount();
  h!.deliver([reset([queued(5, 2, "A."), queued(6, 3, "B.")], header("running"))]);
  await h!.settle();
  inRow(0, "queued-remove")!.focus();
  await h!.click(inRow(0, "queued-remove"));
  h!.deliver([upsert(as(queued(5, 2, "A."), { status: "removed" }))]);
  await h!.settle();
  expect(document.activeElement).toBe(inRow(0, "queued-send-now"));
  expect(fieldFocused).toBe(0);
  await h!.click(inRow(0, "queued-send-now"));
  h!.deliver([upsert(as(queued(6, 3, "B."), { status: "sent" }))]);
  await h!.settle();
  expect(rows()).toEqual([]);
  expect(fieldFocused).toBe(1);
});

test("a strip change renders no other transcript row, and each patch is one commit of the strip", async () => {
  await mount();
  h!.deliver([
    reset(
      [userMessage(1, 1, "Read clause nine."), agentText(2, 1, "Clause nine says notice.")],
      header("running"),
    ),
  ]);
  await h!.settle();
  expect(rowCommits.size).toBeGreaterThan(0);
  rowCommits.clear();
  const commitsOf = async (body: Parameters<Harness["deliver"]>[0]) => {
    const before = stripCommits;
    h!.deliver(body);
    const n = stripCommits - before;
    await h!.settle();
    return n;
  };
  // added, edited, another added, removed
  expect(await commitsOf([upsert(queued(5, 2, "A."))])).toBe(1);
  expect(await commitsOf([upsert(as(queued(5, 2, "A, edited."), { edited: true }))])).toBe(1);
  expect(await commitsOf([upsert(queued(6, 3, "B."))])).toBe(1);
  expect(await commitsOf([upsert(as(queued(6, 3, "B."), { status: "removed" }))])).toBe(1);
  expect(say()).toBe("1 queued, sent once this answer is done.");
  // the queued entries' own rows are the transcript's to hide (U7b-transcript); no other row rendered
  const own = new Set([`user_message:${uuid(2, 5)}`, `user_message:${uuid(3, 5)}`]);
  expect([...rowCommits.keys()].filter((id) => !own.has(id))).toEqual([]);
  // a patch that leaves the queue as it was commits nothing in the strip
  expect(await commitsOf([upsert(agentText(2, 1, "Clause nine says notice, given late."))])).toBe(
    0,
  );
});
