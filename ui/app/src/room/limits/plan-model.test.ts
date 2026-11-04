// The plan's words by the share used (HANDOFF item 13's thresholds), the share left rounded down, each freshness's
// look, and when the composer warns: a session window at 95% used, or the agent's own warning under 75%.
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type {
  AgentLimitsView,
  Freshness,
  LimitMeter,
  LimitWindowView,
  MeterTone,
} from "../../transport/protocol.ts";
import {
  bandOf,
  forgetShownResets,
  lookOf,
  meterLine,
  planLine,
  shareLeft,
  warningKey,
} from "./plan-model.ts";
import { limThread } from "@clauseo/components/lib/words-limits-thread.ts";
import { warningWords } from "../thread/warning-words.ts";

const NOW = Date.UTC(2026, 10, 3, 17, 0); // a fixed instant; fixture times derive from NOW
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  forgetShownResets();
});
afterEach(() => {
  vi.useRealTimers();
});

const MIDNIGHT = NOW - 17 * 3_600_000; // the start of NOW's day
const today = (h: number, m: number) => MIDNIGHT + h * 3_600_000 + m * 60_000;
const RESET = today(18, 10);

/** The host's tone for a share, as `limits` gives it without an agent's warning. */
const toneOf = (used: number): MeterTone =>
  used >= 100 ? "at_limit" : used >= 75 ? "near" : "neutral";

const meter = (used: number, over: Partial<LimitMeter> = {}): LimitMeter => ({
  tone: toneOf(used),
  used_percent: used,
  window_label: "5-hour limit",
  resets_at: RESET,
  ...over,
});

const row = (used: number | undefined, over: Partial<LimitWindowView> = {}): LimitWindowView => ({
  id: "five_hour",
  kind: "session",
  label: "5-hour limit",
  ...(used === undefined ? {} : { used_percent: used }),
  tone: toneOf(used ?? 0),
  resets_at: RESET,
  freshness: "fresh",
  ...over,
});

const entry = (used: number, over: Partial<AgentLimitsView> = {}): AgentLimitsView => ({
  plan: "Claude Max",
  billing: "plan",
  meter: meter(used),
  windows: [row(used)],
  at_limit_text: "",
  freshness: "fresh",
  checked_at: NOW,
  ...over,
});

describe("the toolbar's line by the share used", () => {
  test.each([
    [0, "plenty", "Plenty left until the {time} reset"],
    [49.9, "plenty", "Plenty left until the {time} reset"],
    [50, "half", "Under half left. Resets at {time}."],
    [74.9, "half", "Under half left. Resets at {time}."],
    [75, "close", "Close to the limit. Resets at {time}."],
    [99.9, "close", "Close to the limit. Resets at {time}."],
  ] as const)("%s%% used: %s", (used, band, key) => {
    expect(bandOf(meter(used))).toBe(band);
    expect(meterLine("en", meter(used), NOW)).toBe(lim("en", key, { time: "6:10 pm" }));
  });

  test("100% used: the at-limit words, today's time, another day's, and no reset", () => {
    expect(meterLine("en", meter(100), NOW)).toBe("Limit reached until 6:10 pm");
    const tomorrow = RESET + 86_400_000;
    const day = new Intl.DateTimeFormat("en-GB", { weekday: "long" }).format(tomorrow);
    expect(meterLine("en", meter(100, { resets_at: tomorrow }), NOW)).toBe(
      `Limit reached until ${day} 6:10 pm`,
    );
    const { resets_at: _, ...bare } = meter(100);
    expect(meterLine("en", bare, NOW)).toBe("Limit reached. Reset not given.");
    expect(meterLine("hi", bare, NOW)).toBe(lim("hi", "Limit reached. Reset not given."));
  });

  test("tone near under 75% (the agent warned) is close; tone at_limit is reached whatever the share", () => {
    expect(bandOf(meter(40, { tone: "near" }))).toBe("close");
    expect(bandOf(meter(60, { tone: "at_limit" }))).toBe("reached");
  });

  test("the share left is 100 minus used, rounded down", () => {
    expect(shareLeft(49.9)).toBe(50);
    expect(shareLeft(74.9)).toBe(25);
    expect(shareLeft(0.1)).toBe(99);
    expect(shareLeft(100)).toBe(0);
    expect(shareLeft(120)).toBe(0);
    const line = planLine("en", "claude", entry(74.9), NOW);
    expect(line?.valueText).toBe("25 percent of the plan left");
    expect(line?.left).toBe(0.25);
  });
});

describe("each freshness", () => {
  const looks: [Freshness, ReturnType<typeof lookOf>][] = [
    ["fresh", "figures"],
    ["stale", "stale"],
    ["loading", "loading"],
    ["unavailable", "none"],
    ["not_applicable", "payg"],
    ["missing_scope", "hidden"],
    ["live_only", "hidden"],
    ["from_a_newer_clauseo" as Freshness, "hidden"],
  ];
  test.each(looks)("%s: %s", (freshness, look) => {
    expect(lookOf(entry(30, { freshness }))).toBe(look);
  });

  test("fresh and stale keep their state's line; stale goes muted", () => {
    expect(planLine("en", "claude", entry(30), NOW)).toMatchObject({
      name: "Claude",
      status: "Plenty left until the 6:10 pm reset",
      tone: "neutral",
      freshness: "fresh",
    });
    expect(planLine("en", "claude", entry(80, { freshness: "stale" }), NOW)).toMatchObject({
      status: "Close to the limit. Resets at 6:10 pm.",
      look: "stale",
      state: "close",
      tone: "near",
    });
  });

  test("loading: the name, no meter and no line", () => {
    const { meter: _, ...loading } = entry(0, { freshness: "loading", windows: [] });
    expect(planLine("en", "codex", loading, NOW)).toEqual({
      name: "ChatGPT",
      left: 0,
      valueText: "",
      status: "",
      state: "plenty",
      look: "loading",
      freshness: "loading",
    });
  });

  test("unavailable: the name and Usage not shown right now, no figure", () => {
    const { meter: _, ...none } = entry(0, { freshness: "unavailable", windows: [] });
    expect(planLine("en", "claude", none, NOW)).toMatchObject({
      status: "Usage not shown right now",
      look: "none",
      valueText: "",
    });
  });

  test("pay as you go: its title, who bills for Anthropic, nothing for another provider, no figure", () => {
    const payg = (provider: AgentLimitsView["provider"]) => {
      const { meter: _, ...e } = entry(0, {
        billing: "pay_as_you_go",
        freshness: "not_applicable",
        windows: [],
        ...(provider === undefined ? {} : { provider }),
      });
      return e;
    };
    expect(planLine("en", "claude", payg("anthropic"), NOW)).toMatchObject({
      name: "Claude, pay as you go",
      status: "Billed to your Anthropic account",
      look: "bare",
      valueText: "",
    });
    expect(planLine("en", "codex", payg("open_ai"), NOW)).toMatchObject({
      name: "ChatGPT, pay as you go",
      status: "",
      look: "bare",
    });
    expect(planLine("hi", "claude", payg("anthropic"), NOW)?.name).toBe(
      lim("hi", "{name}, pay as you go", { name: "Claude" }),
    );
  });

  test("missing_scope and live_only have no line until QU.8's words", () => {
    expect(planLine("en", "claude", entry(30, { freshness: "missing_scope" }), NOW)).toBeNull();
    expect(planLine("en", "claude", entry(30, { freshness: "live_only" }), NOW)).toBeNull();
  });
});

describe("the warning", () => {
  test("a session window: none at 94.9%, from 95%", () => {
    expect(warningKey(entry(94.9))).toBe("");
    expect(warningKey(entry(95))).not.toBe("");
    expect(warningWords("en", warningKey(entry(95)), NOW)).toEqual({
      lead: "Close to your 5-hour limit.",
      until: { text: "Long tasks may pause until 6:10 pm.", at: RESET },
    });
  });

  test("a weekly window at 95% is not the session's", () => {
    const weekly = entry(10, { windows: [row(10), row(96, { id: "seven_day", kind: "weekly" })] });
    expect(warningKey(weekly)).toBe("");
  });

  test("the agent's warning: tone near under 75%, from the meter's window and reset", () => {
    const warned = entry(40, { meter: meter(40, { tone: "near" }) });
    expect(warningWords("en", warningKey(warned), NOW)?.until?.text).toBe(
      "Long tasks may pause until 6:10 pm.",
    );
    // near from 75% is the share's own: no warning below the session's 95%
    expect(warningKey(entry(80))).toBe("");
  });

  test("without a reset only the first sentence; in Hindi the design's words", () => {
    const { resets_at: _, ...bare } = row(97);
    const e = entry(97, { windows: [bare] });
    expect(warningWords("en", warningKey(e), NOW)).toEqual({
      lead: "Close to your 5-hour limit.",
      until: null,
    });
    expect(warningWords("hi", warningKey(entry(97)), NOW)?.until?.text).toBe(
      limThread("hi", "Long tasks may pause until {time}.", { time: clock("hi", "6:10 pm") }),
    );
  });

  test("never close to a limit already reached: the meter at 100%, or the session window at it", () => {
    expect(planLine("en", "claude", entry(100), NOW)?.status).toBe(
      lim("en", "Limit reached until {time}", { time: "6:10 pm" }),
    );
    expect(warningKey(entry(100))).toBe("");
    expect(warningKey(entry(120))).toBe("");
    // the meter on the weekly window at its limit, the session close: the toolbar says Limit reached, no warning
    const weekly = entry(100, {
      meter: meter(100, { window_label: "Weekly limit" }),
      windows: [row(97), row(100, { id: "seven_day", kind: "weekly" })],
    });
    expect(warningKey(weekly)).toBe("");
    // the session window at 100% under a meter that isn't: still no "close to"
    expect(warningKey(entry(60, { windows: [row(100)] }))).toBe("");
  });

  test("never for an account billed by use", () => {
    expect(warningKey(entry(99, { billing: "pay_as_you_go" }))).toBe("");
  });
});

describe("the reset the line shows", () => {
  const status = (resets_at: number) =>
    planLine("en", "claude", entry(30, { meter: meter(30, { resets_at }) }), NOW)?.status;

  test("two reads 100 ms apart across a minute's edge show the same minute", () => {
    const edge = today(18, 10) - 50; // 6:09:59.950 pm, then 6:10:00.050 pm
    expect(status(edge)).toBe("Plenty left until the 6:09 pm reset");
    expect(status(edge + 100)).toBe("Plenty left until the 6:09 pm reset");
  });

  test("a reset that moves five minutes shows the new time", () => {
    expect(status(RESET)).toBe("Plenty left until the 6:10 pm reset");
    expect(status(RESET + 5 * 60_000)).toBe("Plenty left until the 6:15 pm reset");
  });
});
