// @vitest-environment happy-dom
// UC-873, the starting hint (starting-hint.ts, work-composer.tsx) in the real room over FakeTransport: a thread whose
// header says Starting shows the working line's one more line, "{agent} is taking longer than usual to start.", once
// it has held for 8 s; not at 7.9 s; Stop works while it shows, as it does without it; any other status takes it away
// and clears its timer; a later Starting counts 8 s afresh. The clock is fake for the timers and Date, from after the
// room has mounted (the harness's settle waits on a real timer, so the clicks here are act's own).
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { header, reset, THREAD, upsert } from "../../testing/stream-script.ts";
import type { ThreadSummary } from "../../transport/protocol.ts";
import { STARTING_HINT_MS } from "./starting-hint.ts";
import { mountRoom, summary, type Harness } from "./harness.ts";

let h: Harness | null = null;

afterEach(async () => {
  await h?.unmount();
  h = null;
  // the spies first: restored over fake timers they would leave the fake setTimeout in place
  vi.restoreAllMocks();
  vi.useRealTimers();
});

/** The clock from here on: the timers and Date are fake. */
function fakeClock() {
  vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"] });
  vi.setSystemTime(new Date(2026, 10, 16, 10, 12));
}

/** The thread's header in `status`, its agent as the list has it (claude-acp unless `agent` says). */
const at = (status: "idle" | "starting" | "running", agent = "claude-acp") =>
  upsert({ header: { ...header(status).header!, agent } });

const advance = (ms: number) => act(async () => void vi.advanceTimersByTime(ms));
const hint = () => h!.host.querySelector<HTMLElement>('[data-testid="working-note"]');
const working = () => h!.one("working");

/** A thread ready for a question, with the agent in `agent`, its own turn not yet started. */
async function mount(agent?: string) {
  const threads: ThreadSummary[] | undefined =
    agent === undefined ? undefined : [{ ...summary(), agent }];
  h = await mountRoom(threads === undefined ? {} : { threads });
  h.deliver([reset([], header("idle"))]);
  fakeClock();
}

test("at 7.9 s no hint; at 8 s the hint, under the working line, with Claude's name", async () => {
  await mount();
  h!.deliver([at("starting")]);
  expect(working()?.textContent).toContain("Answering.");
  await advance(STARTING_HINT_MS - 100);
  expect(hint()).toBeNull();
  await advance(100);
  expect(hint()?.textContent).toBe("Claude is taking longer than usual to start.");
  // the line is said with the working line's words, to assistive technology too
  expect(h!.host.textContent).toContain("Claude is taking longer than usual to start.");
  // and the row's own words stand as they were
  expect(working()?.textContent).toContain("Answering.");
});

test("a ChatGPT thread's hint says ChatGPT", async () => {
  await mount("codex-acp");
  h!.deliver([at("starting", "codex-acp")]);
  await advance(STARTING_HINT_MS);
  expect(hint()?.textContent).toBe("ChatGPT is taking longer than usual to start.");
});

test("an ordinary start (3.1 s) never shows the hint", async () => {
  await mount();
  h!.deliver([at("starting")]);
  await advance(3_100);
  h!.deliver([at("running")]);
  await advance(STARTING_HINT_MS * 2);
  expect(hint()).toBeNull();
});

test("Stop works while the hint shows: it sends the same cancel as without it", async () => {
  await mount();
  h!.deliver([at("starting")]);
  await advance(STARTING_HINT_MS);
  expect(hint()).not.toBeNull();
  const stop = h!.one("cancel") as HTMLButtonElement | null;
  expect(stop).not.toBeNull();
  expect(stop?.disabled).toBe(false);
  await act(async () => stop!.click());
  expect(h!.sent("cancel")).toEqual([{ thread: THREAD }]);
  expect(h!.sent("cancel")).toHaveLength(1);
});

test("the status changing removes the hint and clears its timer", async () => {
  await mount();
  const sets = vi.spyOn(globalThis, "setTimeout");
  const clears = vi.spyOn(globalThis, "clearTimeout");
  h!.deliver([at("starting")]);
  // the hint's timer: the one set for STARTING_HINT_MS
  const ids = sets.mock.calls.flatMap((call, i) =>
    call[1] === STARTING_HINT_MS ? [sets.mock.results[i]!.value] : [],
  );
  expect(ids).toHaveLength(1);
  await advance(STARTING_HINT_MS);
  expect(hint()).not.toBeNull();
  h!.deliver([at("running")]);
  expect(hint()).toBeNull();
  expect(working()?.textContent).toContain("Answering.");
  expect(clears.mock.calls.some(([id]) => id === ids[0])).toBe(true);
});

test("a thread that goes back to idle after Starting shows no hint", async () => {
  await mount();
  h!.deliver([at("starting")]);
  await advance(3_000);
  h!.deliver([at("idle")]);
  await advance(STARTING_HINT_MS * 2);
  expect(hint()).toBeNull();
});

test("unmounted while starting: the timer is cleared, and nothing updates after it", async () => {
  await mount();
  const clears = vi.spyOn(globalThis, "clearTimeout");
  const errors = vi.spyOn(console, "error");
  const sets = vi.spyOn(globalThis, "setTimeout");
  h!.deliver([at("starting")]);
  const ids = sets.mock.calls.flatMap((call, i) =>
    call[1] === STARTING_HINT_MS ? [sets.mock.results[i]!.value] : [],
  );
  expect(ids).toHaveLength(1);
  await h!.unmount();
  h = null;
  expect(clears.mock.calls.some(([id]) => id === ids[0])).toBe(true);
  await advance(STARTING_HINT_MS * 2);
  expect(errors).not.toHaveBeenCalled();
});

test("a later Starting starts a fresh 8 s, and the first one's timer does not show it early", async () => {
  await mount();
  h!.deliver([at("starting")]);
  await advance(5_000);
  h!.deliver([at("running")]);
  await advance(1_000);
  // Starting again at 6 s: the first count (due at 8 s) is gone, and the second runs from here
  h!.deliver([at("starting")]);
  await advance(STARTING_HINT_MS - 100);
  expect(hint()).toBeNull();
  await advance(100);
  expect(hint()).not.toBeNull();
});
