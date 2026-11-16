// @vitest-environment happy-dom
// R20b's resume cue (resume-cue.ts, queue-strip.tsx, queue.tsx) over FakeTransport in the real room: a thread whose
// header says resuming_since shows "Getting {plan} ready." once more than 300 ms have passed since it, on the head of
// the queue strip, or alone over an empty queue; it clears with the field; it never shows without the field; a held
// line gives way to it; and unmounting clears its timer. The clock is fake for the timer and for Date, from after the
// room has mounted (the harness's own settle waits on a real timer).
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { AgentPlanContext } from "./thread/agent-plan.ts";
import { StoreContext } from "../store/hooks.ts";
import { header, reset, STREAM, THREAD, userMessage, uuid } from "../testing/stream-script.ts";
import type { StreamId, ViewEntry } from "../transport/protocol.ts";
import { loadQueueStrip } from "./queue-chunk.ts";
import { QueueSlot } from "./queue-slot.tsx";
import { mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

// noon on 3 November 2026 on the computer's clock
const NOW = new Date(2026, 10, 3, 12, 0);

/** The room mounted, with the slot beside it in its own root in the same store; the timers are the real ones. */
async function mount(agent?: { kind: "claude" | "codex" | undefined; name: string | null }) {
  h = await mountRoom();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const slot = createElement(QueueSlot, {
    transport: h.transport,
    stream: STREAM as StreamId,
    thread: THREAD,
    onFocusField: () => {},
  });
  await act(async () =>
    root!.render(
      createElement(
        StoreContext,
        { value: h!.store },
        agent === undefined ? slot : createElement(AgentPlanContext, { value: agent }, slot),
      ),
    ),
  );
  await act(async () => void (await loadQueueStrip()));
}

/** The clock from here on: Date and the timers are fake, at NOW. */
function fakeClock() {
  vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"] });
  vi.setSystemTime(NOW);
}

/** The thread's header restarting after an idle stop, its resuming_since as given (absent for none). */
const resuming = (since: number | undefined, over: Record<string, unknown> = {}): ViewEntry => ({
  header: {
    ...header("idle").header!,
    status: "starting",
    ...(since === undefined ? {} : { resuming_since: since }),
    ...over,
  },
});
const queued = (order: number, n: number, text: string): ViewEntry => ({
  user_message: {
    ...userMessage(order, n, text).user_message!,
    status: "queued",
    turn: uuid(n, 4),
  },
});

const one = (id: string) => host!.querySelector<HTMLElement>(`[data-testid="${id}"]`);
const said = () => one("queue-said")?.textContent ?? "";
const advance = (ms: number) => act(async () => void vi.advanceTimersByTime(ms));

test("the field set at t0: no line at 299 ms, the line at 301 ms", async () => {
  await mount();
  fakeClock();
  h!.deliver([reset([], resuming(NOW.getTime()))]);
  await advance(299);
  expect(one("resume-cue")).toBeNull();
  expect(said()).toBe("");
  await advance(2);
  expect(one("resume-cue")?.textContent).toBe("Getting Claude ready.");
  expect(said()).toBe("Getting Claude ready.");
  expect(one("resume-cue")?.closest("section")?.className).toBe("queue");
});

test("the field clears: the line goes at once, and so does the strip", async () => {
  await mount();
  fakeClock();
  h!.deliver([reset([], resuming(NOW.getTime()))]);
  await advance(301);
  expect(one("resume-cue")).not.toBeNull();
  h!.deliver([reset([], header("running"))]);
  expect(one("resume-cue")).toBeNull();
  expect(host!.querySelector("section")).toBeNull();
  expect(said()).toBe("");
});

test("the field absent: the line is never shown, and no timer is set", async () => {
  await mount();
  fakeClock();
  h!.deliver([reset([], resuming(undefined))]);
  await advance(10_000);
  expect(one("resume-cue")).toBeNull();
  expect(said()).toBe("");
  expect(vi.getTimerCount()).toBe(0);
});

test("a held line plus the resume: only the resume line shows, the hold's words and Send next do not", async () => {
  await mount();
  fakeClock();
  h!.deliver([
    reset(
      [queued(5, 2, "A.")],
      resuming(NOW.getTime(), { queue_held: true, queue_held_reason: "update" }),
    ),
  ]);
  await advance(301);
  expect(said()).toBe("Getting Claude ready.");
  expect(one("queue-next")).toBeNull();
  expect(one("queue-why")).toBeNull();
  expect(host!.querySelectorAll("li")).toHaveLength(1);
});

test("the timer is cleared when its slot unmounts", async () => {
  await mount();
  fakeClock();
  const sets = vi.spyOn(globalThis, "setTimeout");
  const clears = vi.spyOn(globalThis, "clearTimeout");
  h!.deliver([reset([], resuming(NOW.getTime()))]);
  // the cue's timers: 301 ms, the time left at its start, one per mounted slot (the room's own composer has one too);
  // React's timers have other delays
  const ids = sets.mock.calls.flatMap((call, i) =>
    call[1] === 301 ? [sets.mock.results[i]!.value] : [],
  );
  expect(ids.length).toBeGreaterThanOrEqual(1);
  await act(async () => root!.unmount());
  root = null;
  // this slot's timer is cleared, and only its own
  const cleared = ids.filter((id) => clears.mock.calls.some(([x]) => x === id));
  expect(cleared).toHaveLength(1);
});

test("a ChatGPT agent's resume says ChatGPT", async () => {
  await mount({ kind: "codex", name: null });
  fakeClock();
  h!.deliver([reset([], resuming(NOW.getTime()))]);
  await advance(301);
  expect(said()).toBe("Getting ChatGPT ready.");
});
