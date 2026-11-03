// @vitest-environment happy-dom
// The plan in the room over FakeTransport (U12): the toolbar's line for the open conversation's agent (claude-acp and
// codex-acp, never fake-agent), its card and what opening it and Try again send, the composer's warning with the
// host's reset in a <time>, Ask in every state, and nothing left on a timer once a view is shown.
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { reset } from "../../testing/stream-script.ts";
import type {
  AgentLimitsView,
  LimitMeter,
  LimitWindowView,
  LimitsView,
  MeterTone,
} from "../../transport/protocol.ts";
import { fakeNext } from "../fake-next.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";
import { LIMIT_WARNING_FEED } from "./limit-warning-feed.ts";
import { preloadPlanCard } from "./plan-card-load.ts";

// the card's preload, watched: the same function, counted
vi.mock("./plan-card-load.ts", async (actual) => {
  const real = await actual<typeof import("./plan-card-load.ts")>();
  return { preloadPlanCard: vi.fn(real.preloadPlanCard) };
});

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const today = (hh: number, mm: number) => {
  const d = new Date();
  d.setHours(hh, mm, 0, 0);
  return d.getTime();
};
const RESET = today(18, 10);
const READ = today(0, 5);
const toneOf = (used: number): MeterTone =>
  used >= 100 ? "at_limit" : used >= 75 ? "near" : "neutral";

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
const meter = (used: number, over: Partial<LimitMeter> = {}): LimitMeter => ({
  tone: toneOf(used),
  used_percent: used,
  window_label: "5-hour limit",
  resets_at: RESET,
  ...over,
});
const entry = (used: number, over: Partial<AgentLimitsView> = {}): AgentLimitsView => ({
  plan: "Claude Max",
  billing: "plan",
  meter: meter(used),
  windows: [row(used), row(20, { id: "seven_day", kind: "weekly", label: "Weekly limit" })],
  at_limit_text: "",
  freshness: "fresh",
  checked_at: READ,
  ...over,
});
const figureless = (over: Partial<AgentLimitsView>): AgentLimitsView => {
  const { meter: _, ...e } = entry(0, { windows: [], ...over });
  return e;
};

/** The room on a new conversation, whose agent is the one the host lists (R15): `agent`, of its kind where it has
 * one; with words in the field, so Ask stands enabled as long as nothing holds it. */
async function room(agent = "claude-acp") {
  const kind = agent.startsWith("claude")
    ? "claude"
    : agent.startsWith("codex")
      ? "codex"
      : undefined;
  h = await mountRoom({
    threads: [],
    next: fakeNext({
      matters: () => [],
      agents: [kind === undefined ? { id: agent } : { id: agent, kind }],
      parts: ["agents"],
    }),
  });
  h.deliver([reset([])]);
  await h.type("Summarise the notice to the bank.");
  return h;
}

/** Plays a view, and lets a chunk it asks for (the warning's note) load. */
const view = async (v: LimitsView) => {
  await act(async () => h!.transport.limitsChanged(v));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
};
const plan = () => h!.one("plan");
const send = () => h!.one("send") as HTMLButtonElement | null;
const sendEnabled = () =>
  send() !== null && !send()!.disabled && send()!.getAttribute("aria-disabled") !== "true";

/** Presses the plan and waits for the card's chunk. */
async function openCard() {
  await h!.click(plan());
  const card = () => document.querySelector<HTMLElement>('[data-testid="plan-card"]');
  for (let i = 0; i < 5 && card() === null; i += 1) {
    await act(async () => {
      await vi.dynamicImportSettled();
      await new Promise((r) => setTimeout(r, 0));
    });
  }
  return card();
}

test("claude-acp gets the Claude plan's line, with its tone and freshness; no line before a view", async () => {
  await room();
  expect(plan()).toBeNull();
  await view({ claude: entry(30) });
  expect(plan()?.dataset["tone"]).toBe("neutral");
  expect(plan()?.dataset["freshness"]).toBe("fresh");
  expect(plan()?.textContent).toContain("Claude plan");
  expect(plan()?.textContent).toContain("Plenty left until the 6:10 pm reset");
  expect(sendEnabled()).toBe(true);
});

test("codex-acp gets the ChatGPT plan's line, from the codex entry", async () => {
  await room("codex-acp");
  await view({ claude: entry(10), codex: entry(80, { plan: "ChatGPT Plus" }) });
  expect(plan()?.textContent).toContain("ChatGPT plan");
  expect(plan()?.textContent).toContain("Close to the limit. Resets at 6:10 pm.");
  expect(plan()?.dataset["tone"]).toBe("near");
});

test("fake-agent has no plan line, whatever the view holds", async () => {
  await room("fake-agent");
  await view({ claude: entry(96), codex: entry(96) });
  expect(plan()).toBeNull();
  expect(h!.one("limit-warning")).toBeNull();
  expect(sendEnabled()).toBe(true);
});

test("the warning at 95%, none at 94.9%, with the host's reset in a <time>; Ask stays", async () => {
  await room();
  await view({ claude: entry(94.9) });
  expect(h!.one("limit-warning")).toBeNull();
  await view({ claude: entry(95) });
  const warning = h!.one("limit-warning");
  expect(warning?.textContent).toBe(
    "Close to your 5-hour limit. Long tasks may pause until 6:10 pm.",
  );
  const time = warning?.querySelector("time");
  expect(time?.getAttribute("datetime")).toBe(new Date(RESET).toISOString());
  expect(time?.textContent).toBe("Long tasks may pause until 6:10 pm.");
  expect(sendEnabled()).toBe(true);
});

test("the agent's warning, tone near under 75%, warns too", async () => {
  await room();
  await view({ claude: entry(40, { meter: meter(40, { tone: "near" }) }) });
  expect(plan()?.dataset["tone"]).toBe("near");
  expect(h!.one("limit-warning")).not.toBeNull();
});

test("at the limit: the at-limit words, never a disabled Ask", async () => {
  await room();
  await view({ claude: entry(100) });
  expect(plan()?.dataset["tone"]).toBe("at_limit");
  expect(plan()?.textContent).toContain("Limit reached until 6:10 pm");
  expect(sendEnabled()).toBe(true);
});

test("Ask is enabled in every freshness and billing", async () => {
  await room();
  const states: AgentLimitsView[] = [
    entry(30),
    entry(30, { freshness: "stale" }),
    figureless({ freshness: "loading" }),
    figureless({ freshness: "unavailable" }),
    figureless({ freshness: "missing_scope" }),
    figureless({ freshness: "live_only" }),
    figureless({ freshness: "not_applicable", billing: "pay_as_you_go", provider: "anthropic" }),
    entry(100),
  ];
  for (const s of states) {
    await view({ claude: s });
    expect(sendEnabled(), s.freshness).toBe(true);
  }
});

test("the card: As of, each window's share left, its bar and reset; opening it sends shown", async () => {
  await room();
  await view({ claude: entry(62) });
  const card = await openCard();
  expect(card).not.toBeNull();
  expect(card?.textContent).toContain("Claude Max");
  expect(card?.textContent).toContain("As of 12:05 am");
  expect(card?.textContent).toContain("5-hour limit");
  expect(card?.textContent).toContain("38% left");
  expect(card?.textContent).toContain("80% left");
  expect(card?.textContent).toContain("Resets 6:10 pm");
  expect(card?.querySelectorAll(".plan-track")).toHaveLength(2);
  expect(h!.transport.refreshes()).toEqual([{ agent: "claude", why: "shown" }]);
});

test("a window whose reset passed shows no share and no bar, only when it reset", async () => {
  await room();
  await view({
    claude: entry(30, {
      windows: [row(undefined, { freshness: "reset_passed", resets_at: today(0, 1) })],
    }),
  });
  const card = await openCard();
  expect(card?.textContent).not.toMatch(/%/);
  expect(card?.querySelector(".plan-track")).toBeNull();
  expect(card?.textContent).toContain("The limit reset at 12:01 am.");
});

test("stale: the line kept, the plan and the card's figures muted", async () => {
  await room();
  await view({ claude: entry(30, { freshness: "stale" }) });
  expect(plan()?.dataset["freshness"]).toBe("stale");
  expect(plan()?.dataset["look"]).toBe("stale");
  const card = await openCard();
  expect(card?.querySelector(".plan-card-stale")?.textContent).toBe("As of 12:05 am");
  expect(card?.querySelector(".plan-window[data-stale]")).not.toBeNull();
});

test("loading: the name alone, no meter and nothing to open", async () => {
  await room();
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(plan()?.textContent).toContain("Claude plan");
  // no line under the name, and no track
  expect(plan()?.querySelector("p")).toBeNull();
  expect(plan()?.querySelector(".rounded-meter")).toBeNull();
  expect(plan()?.tagName).not.toBe("BUTTON");
});

test("none: its line, and the card's Try again sends try_again", async () => {
  await room();
  await view({ claude: figureless({ freshness: "unavailable" }) });
  expect(plan()?.textContent).toContain("Usage not shown right now");
  const card = await openCard();
  expect(card?.textContent).toContain(
    "Your Claude plan didn't send its limits this time. Asking still works as usual.",
  );
  const tryAgain = document.querySelector('[data-testid="plan-try-again"]');
  // the retry's icon leads its word, hidden; the word names the button
  expect(iconIn(tryAgain, IconTryAgain, 16)?.getAttribute("aria-hidden")).toBe("true");
  expect(tryAgain?.textContent).toBe("Try again");
  await h!.click(tryAgain);
  expect(h!.transport.refreshes()).toEqual([
    { agent: "claude", why: "shown" },
    { agent: "claude", why: "try_again" },
  ]);
  expect(sendEnabled()).toBe(true);
});

test("pay as you go: the title and who bills, no bar and no figure, in the toolbar and the card", async () => {
  await room();
  await view({
    claude: figureless({
      freshness: "not_applicable",
      billing: "pay_as_you_go",
      provider: "anthropic",
    }),
  });
  expect(plan()?.textContent).toContain("Claude, pay as you go");
  expect(plan()?.textContent).toContain("Billed to your Anthropic account");
  expect(plan()?.textContent).not.toMatch(/%|percent/);
  expect(plan()?.querySelector(".rounded-meter")).toBeNull();
  const card = await openCard();
  expect(card?.textContent).toContain("Claude, pay as you go");
  expect(card?.querySelector(".plan-track")).toBeNull();
  expect(card?.textContent).not.toMatch(/%/);
});

test("the hidden plan-state marker: none before a view and for fake-agent, loading, live_only with no line", async () => {
  await room();
  const marker = () => h!.one("plan-state");
  expect(marker()?.dataset["freshness"]).toBe("none");
  expect(marker()?.hidden).toBe(true);
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(marker()?.dataset["freshness"]).toBe("loading");
  expect(plan()).not.toBeNull();
  await view({ claude: entry(30, { freshness: "live_only" }) });
  expect(marker()?.dataset["freshness"]).toBe("live_only");
  expect(marker()?.hidden).toBe(true);
  expect(marker()?.textContent).toBe("");
  expect(plan()).toBeNull();
  await h!.unmount();
  h = null;
  document.body.replaceChildren();
  await room("fake-agent");
  await view({ claude: entry(30) });
  expect(marker()?.dataset["freshness"]).toBe("none");
  expect(plan()).toBeNull();
});

test("limit-warning's feed as claude-acp: the marker goes none, loading, unavailable, then live_only", async () => {
  // the host's own lines (limit-warning-feed.ts) through the channel's parse into the store: fake_agent's reader
  // fails, so the first read ends unavailable; the turn's usage_update (the agent's warning, five_hour at 96%) is the
  // live signal that makes the view live_only (limits-warning.spec.ts)
  await room();
  const seen: (string | undefined)[] = [];
  const note = () => seen.push(h!.one("plan-state")?.dataset["freshness"]);
  note();
  for (const line of LIMIT_WARNING_FEED) {
    await act(async () => h!.transport.deliver(line));
    await act(async () => {
      await vi.dynamicImportSettled();
    });
    note();
  }
  expect(h!.transport.problems).toEqual([]);
  expect([...new Set(seen)]).toEqual(["none", "loading", "unavailable", "live_only"]);
  expect(plan()).toBeNull();
  expect(h!.one("limit-warning")).not.toBeNull();
  expect(sendEnabled()).toBe(true);
});

test("limit-warning's feed carries the scenario's reset, and it is still ahead", () => {
  // a reset already past makes the host drop the row's figure and the meter at once, so no warning shows (PR #228)
  // the scenario the e2e plays, as fake_agent bundles it
  const scenario = readFileSync(
    join(import.meta.dirname, "../../../../../crates/fake_agent/scenarios/limit-warning.jsonl"),
    "utf8",
  );
  const resetsAt = Number(/"resetsAt":(\d+)/.exec(scenario)?.[1]) * 1000;
  expect(resetsAt).toBeGreaterThan(Date.now());
  const live = LIMIT_WARNING_FEED.map(
    (line) => JSON.parse(line) as { limits_view_changed: LimitsView },
  ).find((line) => line.limits_view_changed.claude?.freshness === "live_only");
  expect(live?.limits_view_changed.claude?.windows[0]?.resets_at).toBe(resetsAt);
});

test("focus on the plan starts loading its card before any press; the card stays closed", async () => {
  await room();
  await view({ claude: entry(30) });
  const preload = vi.mocked(preloadPlanCard);
  preload.mockClear();
  await act(async () => plan()?.focus());
  expect(preload).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[data-testid="plan-card"]')).toBeNull();
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  const card = await openCard();
  expect(card?.textContent).toContain("Claude Max");
});

test("missing_scope and live_only: no line", async () => {
  await room();
  await view({ claude: figureless({ freshness: "missing_scope" }) });
  expect(plan()).toBeNull();
  await view({ claude: entry(30, { freshness: "live_only" }) });
  expect(plan()).toBeNull();
});

test("a view leaves nothing on a timer, and an unchanged view renders nothing again", async () => {
  await room();
  vi.useFakeTimers();
  await view({ claude: entry(96) });
  await act(async () => {
    await vi.runAllTimersAsync();
  });
  expect(plan()).not.toBeNull();
  expect(h!.one("limit-warning")).not.toBeNull();
  expect(vi.getTimerCount()).toBe(0);
  const before = plan();
  await view({ claude: entry(96) });
  expect(plan()).toBe(before);
  expect(vi.getTimerCount()).toBe(0);
});
