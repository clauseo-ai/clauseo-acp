// @vitest-environment happy-dom
// U12 over FakeTransport: the stop at the usage limit (its mark, its title with and without the stop's time, the
// window's line, with and without a reset, a reset gone by, the workspace's reasons), the composer's line while the
// thread is paused at the limit, send enabled in every state, and no timer left behind.
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { afterEach, expect, test, vi } from "vitest";
import { header, reset, step, TURN, userMessage, uuid } from "../../testing/stream-script.ts";
import type { HeaderEntry, LimitStopEntry, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, summary, type Harness } from "./harness.ts";
import { atLimitWords, planNameOf } from "./limit-words.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

// the thread's clock reads the computer's zone: this file's times are Asia/Kolkata's whatever zone the run has
process.env["TZ"] = "Asia/Kolkata";

/** A fixed "now", Monday 2 November 2026 at 3 pm local. */
const NOW = new Date(2026, 10, 2, 15, 0);
const local = (days: number, hh: number, mm: number) =>
  new Date(2026, 10, 2 + days, hh, mm).getTime();
const fixNow = () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
};

const STOPPED = local(0, 14, 52);

const limit = (n: number, over: Partial<LimitStopEntry> = {}): ViewEntry => ({
  limit_stop: {
    order: 10 + n,
    turn: uuid(n, 4),
    reason: "plan_window",
    ts: STOPPED,
    ...over,
  },
});
/** An entry from before the stop had its own time. */
const untimed = (n: number, over: Partial<LimitStopEntry> = {}): ViewEntry => {
  const entry = limit(n, over);
  delete (entry.limit_stop as Partial<LimitStopEntry>).ts;
  return entry;
};
const pausedHead = (resets_at?: number): ViewEntry => ({
  header: {
    ...(header().header as HeaderEntry),
    status: { paused_at_limit: resets_at === undefined ? {} : { resets_at } },
  },
});

const resetsAt = (row: HTMLElement | undefined) =>
  row?.querySelector<HTMLTimeElement>('[data-testid="limit-resets-at"]') ?? null;
const mark = (row: HTMLElement | undefined) =>
  row?.querySelector('[data-testid="limit-stopped-here"]') ?? null;
const title = (row: HTMLElement | undefined) =>
  row?.querySelector(".lim-note-title")?.textContent ?? null;
const sub = (row: HTMLElement | undefined) =>
  row?.querySelector(".lim-note-sub")?.textContent ?? null;
const took = () => h!.host.querySelector(".room-status-took")?.textContent ?? null;
const send = () => h!.one("send") as HTMLButtonElement | null;

test("a plan stop with its time: the mark, the title at that time, the window's line with the reset, today or on its day", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      limit(1, { window: "five_hour", resets_at: local(0, 18, 10) }),
      limit(2, { window: "five_hour", resets_at: local(1, 0, 30) }),
      limit(3, { window: "seven_day", resets_at: local(4, 9, 0) }),
      limit(4, { window: "seven_day", resets_at: local(0, 22, 0) }),
    ]),
  ]);
  const [hit, past, weekly, tonight] = h.all("limit-stop");
  expect(mark(hit)?.textContent).toBe(lim("en", "Stopped here at {time}", { time: "2:52 pm" }));
  expect(title(hit)).toBe(
    lim("en", "{name} plan limit reached at {time}", { name: "Claude", time: "2:52 pm" }),
  );
  expect(sub(hit)).toBe("The 5-hour limit. It resets at 6:10 pm.");
  expect(resetsAt(hit)?.dateTime).toBe(new Date(local(0, 18, 10)).toISOString());
  // 3 November 2026 is a Tuesday, 6 November a Friday
  expect(sub(past)).toBe("The 5-hour limit. It resets on Tuesday at 12:30 am.");
  expect(sub(weekly)).toBe("The weekly limit. It resets on Friday at 9:00 am.");
  expect(sub(tonight)).toBe("The weekly limit. It resets at 10:00 pm.");
  // no span: nothing here counts down
  for (const row of [hit, past, weekly, tonight]) expect(row?.textContent).not.toMatch(/\bin \d/);
});

test("a stop on another day marks its day; a window it doesn't name keeps the reset alone", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      limit(1, { ts: local(-1, 16, 0), window: "seven_day_opus", resets_at: local(0, 20, 0) }),
    ]),
  ]);
  const row = h.one("limit-stop") ?? undefined;
  // 1 November 2026 is a Sunday
  expect(mark(row)?.textContent).toBe(
    lim("en", "Stopped here on {day} at {time}", { day: "Sunday", time: "4:00 pm" }),
  );
  expect(title(row)).toBe("Claude plan limit reached at Sunday 4:00 pm");
  expect(sub(row)).toBe(lim("en", "Resets {time}", { time: "8:00 pm" }));
});

test("without the stop's time: the title alone and no mark; without a reset, Reset not given.", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      untimed(1, { window: "five_hour", resets_at: local(0, 18, 10) }),
      untimed(2),
      limit(3, { window: "five_hour" }),
    ]),
  ]);
  const [timeless, bare, noReset] = h.all("limit-stop");
  expect(mark(timeless)).toBeNull();
  expect(title(timeless)).toBe(lim("en", "{name} plan limit reached", { name: "Claude" }));
  expect(sub(timeless)).toBe("The 5-hour limit. It resets at 6:10 pm.");
  expect(resetsAt(timeless)).not.toBeNull();
  expect(mark(bare)).toBeNull();
  expect(sub(bare)).toBe(lim("en", "Reset not given."));
  expect(resetsAt(bare)).toBeNull();
  expect(mark(noReset)).not.toBeNull();
  expect(sub(noReset)).toBe(lim("en", "Reset not given."));
});

test("a reset gone by says when it reset, never the window's line", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      limit(1, { window: "five_hour", resets_at: local(0, 8, 0) }),
      limit(2, { window: "seven_day", resets_at: local(-2, 9, 0) }),
    ]),
  ]);
  const [today, saturday] = h.all("limit-stop");
  expect(sub(today)).toBe(lim("en", "The limit reset at {time}.", { time: "8:00 am" }));
  expect(sub(saturday)).toBe(
    lim("en", "The limit reset on {day} at {time}.", { day: "Saturday", time: "9:00 am" }),
  );
  expect(resetsAt(saturday)?.dateTime).toBe(new Date(local(-2, 9, 0)).toISOString());
});

test("a workspace's limit, a temporary one and an unknown reason: the usage limit and its reset, no mark", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([
    reset([
      limit(1, { reason: "workspace_credits", window: "five_hour", resets_at: local(2, 9, 5) }),
      limit(2, { reason: "workspace_spend_cap" }),
      limit(3, { reason: "temporary", resets_at: local(0, 16, 0) }),
      limit(4, { reason: "unknown" }),
    ]),
  ]);
  const rows = h.all("limit-stop");
  expect(rows.map((r) => r.dataset["reason"])).toEqual([
    "workspace_credits",
    "workspace_spend_cap",
    "temporary",
    "unknown",
  ]);
  for (const row of rows) {
    expect(mark(row)).toBeNull();
    expect(title(row)).toBe(lim("en", "Usage limit reached"));
  }
  expect(sub(rows[0])).toBe(
    lim("en", "Resets {day} {time}", { day: "Wednesday", time: "9:05 am" }),
  );
  expect(sub(rows[1])).toBe(lim("en", "Reset not given."));
  expect(sub(rows[2])).toBe(lim("en", "Resets {time}", { time: "4:00 pm" }));
});

test("the stop is never red and holds no button: the note's tile stays concrete", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([reset([limit(1, { window: "five_hour", resets_at: local(0, 18, 10) })])]);
  const row = h.one("limit-stop")!;
  expect(row.querySelector(".lim-note")?.hasAttribute("data-waiting")).toBe(false);
  expect(row.querySelector("button")).toBeNull();
  expect(row.outerHTML).not.toMatch(/red|danger|destructive|error/);
  expect(row.getAttribute("role")).toBe("status");
});

test("paused with the stop's time: the composer says where it stopped, and send stays enabled", async () => {
  fixNow();
  h = await mountRoom({
    threads: [summary(undefined, { paused_at_limit: { resets_at: local(0, 18, 10) } })],
  });
  h.deliver([
    reset(
      [
        userMessage(5, 1, "Draft the clause"),
        limit(1, { turn: TURN, window: "five_hour", resets_at: local(0, 18, 10) }),
        step(12, { stop: "at_limit" }),
      ],
      pausedHead(local(0, 18, 10)),
    ),
  ]);
  expect(took()).toBe(lim("en", "Stopped at {time}, at the plan's limit", { time: "2:52 pm" }));
  expect(h.one("composer")?.hasAttribute("disabled")).toBe(false);
  await h.type("Continue");
  expect(send()?.disabled).toBe(false);
  await h.click(send());
  expect(h.sent("send_prompt")).toHaveLength(1);
});

test("paused without the stop's time: the toolbar's at-limit words, its day form, or no reset given", async () => {
  fixNow();
  const cases: [number | undefined, string][] = [
    [local(0, 18, 10), lim("en", "Limit reached until {time}", { time: "6:10 pm" })],
    [
      local(1, 9, 0),
      lim("en", "Limit reached until {day} {time}", { day: "Tuesday", time: "9:00 am" }),
    ],
    [undefined, lim("en", "Limit reached. Reset not given.")],
  ];
  for (const [resets, words] of cases) {
    h = await mountRoom();
    h.deliver([
      reset(
        [
          userMessage(5, 1, "Q"),
          untimed(1, { turn: TURN, ...(resets === undefined ? {} : { resets_at: resets }) }),
        ],
        pausedHead(resets),
      ),
    ]);
    expect(took()).toBe(words);
    await h.type("Again");
    expect(send()?.disabled).toBe(false);
    await h.unmount();
    h = null;
    document.body.replaceChildren();
  }
});

test("paused with no stop in view, the at-limit words; not paused, no limit line on the composer", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([reset([userMessage(5, 1, "Q")], pausedHead(local(0, 18, 10)))]);
  expect(took()).toBe(lim("en", "Limit reached until {time}", { time: "6:10 pm" }));
  await h.unmount();
  document.body.replaceChildren();
  // a stop from an earlier turn, the thread idle since: the note stays in the thread, the composer says nothing of it
  h = await mountRoom();
  h.deliver([reset([limit(1, { window: "five_hour", resets_at: local(0, 18, 10) })])]);
  expect(h.one("limit-stop")).not.toBeNull();
  expect(took()).toBeNull();
  await h.type("Again");
  expect(send()?.disabled).toBe(false);
});

test("a newer question after the stop: the pause words don't take the old stop's time", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([reset([limit(1), userMessage(20, 2, "Again")], pausedHead(local(0, 18, 10)))]);
  expect(took()).toBe(lim("en", "Limit reached until {time}", { time: "6:10 pm" }));
});

test("the stop, the pause and their words start no timer: no interval, no delayed timeout, no frame", async () => {
  fixNow();
  h = await mountRoom();
  h.deliver([reset([userMessage(5, 1, "Q")])]);
  await h.settle();
  // from here on, every timer the room asks for is counted
  const timeouts = vi.spyOn(globalThis, "setTimeout");
  const intervals = vi.spyOn(globalThis, "setInterval");
  const frames = vi.spyOn(globalThis, "requestAnimationFrame");
  h.deliver([
    reset(
      [
        userMessage(5, 1, "Q"),
        limit(1, { turn: TURN, window: "five_hour", resets_at: local(0, 18, 10) }),
      ],
      pausedHead(local(0, 18, 10)),
    ),
  ]);
  expect(h.one("limit-stop")).not.toBeNull();
  expect(took()).not.toBeNull();
  expect(intervals).not.toHaveBeenCalled();
  expect(timeouts.mock.calls.filter(([, ms]) => (ms ?? 0) > 0)).toEqual([]);
  expect(frames).not.toHaveBeenCalled();
  timeouts.mockRestore();
  intervals.mockRestore();
  frames.mockRestore();
});

test("the plan's name: ChatGPT for Codex, Claude for Claude or an agent not known, else the agent's own", () => {
  expect(planNameOf(undefined)).toBe("Claude");
  expect(planNameOf({ kind: "claude", name: null })).toBe("Claude");
  expect(planNameOf({ kind: "codex", name: null })).toBe("ChatGPT");
  expect(planNameOf({ kind: undefined, name: "Gemini" })).toBe("Gemini");
  expect(planNameOf({ kind: undefined, name: " " })).toBe("Claude");
  expect(atLimitWords("hi", undefined, NOW.getTime())).toBe(
    lim("hi", "Limit reached. Reset not given."),
  );
});
