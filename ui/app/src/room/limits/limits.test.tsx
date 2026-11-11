// @vitest-environment happy-dom
// The plan in the room over FakeTransport (U12): the toolbar's line for the open conversation's agent (claude-acp and
// codex-acp, never fake-agent), its card and what opening it and Try again send, the composer's warning with the
// host's reset in a <time>, Ask in every state, and nothing left on a timer once a view is shown.
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { reset } from "../../testing/stream-script.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type {
  AgentLimitsView,
  LimitMeter,
  LimitWindowView,
  LimitsView,
  MeterTone,
  SetupEvent,
  SignIn,
} from "../../transport/protocol.ts";
import { CLAUDE, setupStoreOf, type SetupSource } from "../../store/setup.ts";
import { fakeNext } from "../fake-next.ts";
import { loadThread } from "../thread/chunk.ts";
import { mountRoom, type Harness } from "../thread/harness.ts";
import { LIMIT_WARNING_FEED } from "./limit-warning-feed.ts";
import { bounded, startWaits } from "./limits-test-wait.ts";
import { preloadPlanCard } from "./plan-card-load.ts";

// the card's preload, watched: the same function, counted
vi.mock("./plan-card-load.ts", async (actual) => {
  const real = await actual<typeof import("./plan-card-load.ts")>();
  return { preloadPlanCard: vi.fn(real.preloadPlanCard) };
});

const NOW = Date.UTC(2026, 10, 3, 17, 0); // a fixed instant; fixture times derive from NOW
const MIDNIGHT = NOW - 17 * 3_600_000; // the start of NOW's day

let h: Harness | null = null;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  startWaits();
});

afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.useRealTimers();
});

const today = (hh: number, mm: number) => MIDNIGHT + hh * 3_600_000 + mm * 60_000;
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

// the sign-ins the setup store hears for the agent: the header's plan shows once one names the plan
const AT = NOW;
const planSignIn = (plan: string): SignIn => ({
  signed_in: { plan, method: "plan", source: "own", at: AT },
});
const KEY_SIGN_IN: SignIn = { signed_in: { method: "api_key", source: "own", at: AT } };
const FIRM_SIGN_IN: SignIn = { signed_in: { method: "firm", source: "own", at: AT } };
const NO_PLAN_SIGN_IN: SignIn = { signed_in: { method: "plan", source: "own", at: AT } };
const SIGNED_OUT: SignIn = { signed_out: {} };
const WAITING: SignIn = { waiting: { since: AT, nudge: false } };

/** The room on a new conversation, whose agent is the one the host lists (R15): `agent`, of its
 * kind where it has one; with words in the field, so Ask stands enabled as long as nothing holds
 * it. Its sign-in is `sign`: signed in with the kind's plan by default; null for a host that has
 * said nothing of the sign-in yet. */
async function room(
  agent = "claude-acp",
  language: "en" | "hi" = "en",
  sign: SignIn | null = planSignIn(agent.startsWith("codex") ? "ChatGPT Plus" : "Claude Max"),
) {
  const kind = agent.startsWith("claude")
    ? "claude"
    : agent.startsWith("codex")
      ? "codex"
      : undefined;
  const setupFirst: SetupEvent[] = [{ setup_info: { keys_available: true } }];
  if (sign !== null && kind !== undefined)
    setupFirst.push({ sign_in_changed: { agent, state: sign } });
  h = await mountRoom({
    threads: [],
    next: fakeNext({
      matters: () => [],
      agents: [kind === undefined ? { id: agent } : { id: agent, kind }],
      parts: ["agents"],
    }),
    display: { language },
    setup: { first: () => setupFirst },
    waitForRenderer: false,
  });
  // the thread's chunk, which the harness would await in an act of its own with no bound
  await bounded("the thread's chunk", loadThread);
  h.deliver([reset([])]);
  await h.type("Summarise the notice to the bank.");
  return h;
}

/** Plays a view, and lets a chunk it asks for (the warning's note) load. */
const view = async (v: LimitsView) => {
  await act(async () => h!.transport.limitsChanged(v));
  await bounded("the view's chunks", () => vi.dynamicImportSettled());
};
/** Lets the setup wire's events and a chunk they ask for land. */
const settle = () => bounded("the setup wire's chunks", () => vi.dynamicImportSettled());
const plan = () => h!.one("plan");
// the plan's words as they show; the meter's hidden "x" (Base UI's, for VoiceOver) is none of them
const shown = () => plan()?.textContent?.replace(/x$/, "");
const send = () => h!.one("send") as HTMLButtonElement | null;
const sendEnabled = () =>
  send() !== null && !send()!.disabled && send()!.getAttribute("aria-disabled") !== "true";

/** Presses the plan and waits for the card's chunk. */
async function openCard() {
  await h!.click(plan());
  const card = () => document.querySelector<HTMLElement>('[data-testid="plan-card"]');
  for (let i = 0; i < 5 && card() === null; i += 1) {
    await bounded("the card's chunk", async () => {
      await vi.dynamicImportSettled();
      await new Promise((r) => setTimeout(r, 0));
    });
  }
  return card();
}

/** The sign-in the setup store holds for claude-acp, as it has heard it so far. */
const signInHeard = () =>
  setupStoreOf(h!.transport as HostTransport & SetupSource).state().agents[CLAUDE]?.signIn;

test("no line before a view, signed out: the sign-in is driven to signed out first", async () => {
  await room("claude-acp", "en", SIGNED_OUT);
  await settle();
  expect(signInHeard()).toEqual(SIGNED_OUT);
  expect(plan()).toBeNull();
  await act(async () => {
    h!.transport.setupEvent({
      sign_in_changed: { agent: "claude-acp", state: planSignIn("Claude Max") },
    });
  });
  await settle();
  expect(signInHeard()).toEqual(planSignIn("Claude Max"));
  expect(plan()).toBeNull();
});

test("a view arriving while signed out gives the name only, never the plan's line", async () => {
  await room("claude-acp", "en", SIGNED_OUT);
  await settle();
  expect(signInHeard()).toEqual(SIGNED_OUT);
  await view({ claude: entry(30) });
  expect(shown()).toBe("Claude");
  expect(plan()?.dataset["look"]).toBe("bare");
  expect(plan()?.dataset["freshness"]).toBe("fresh");
  expect(plan()?.querySelector(".rounded-meter")).toBeNull();
});

test("claude-acp gets the Claude plan's line, with its tone and freshness; no line before a view", async () => {
  await room();
  await settle();
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

/** A Max plan's per-model windows (#236): the scoped Fable one, the fixed Opus one, an unknown label, as the host sends
 * them; the label is built from `model`, never read out of the English `label`. */
const MODEL_ROWS: LimitWindowView[] = [
  row(41, { id: "seven_day_overage_included", kind: "model", label: "x", model: "Fable" }),
  row(12, { id: "seven_day_opus", kind: "model", label: "Opus weekly limit", model: "Opus" }),
  row(5, { id: "monthly_plus", kind: "other", label: "Plus monthly cap" }),
];

test("a per-model window reads {model} weekly limit from the host's model; an unknown label as given (#236)", async () => {
  await room();
  await view({ claude: entry(30, { windows: MODEL_ROWS }) });
  const names = [...(await openCard())!.querySelectorAll(".plan-window-name")].map(
    (n) => n.textContent,
  );
  expect(names).toEqual(["Fable weekly limit", "Opus weekly limit", "Plus monthly cap"]);
});

test("a Codex window has no model: its label as the host gives it (#236)", async () => {
  await room("codex-acp");
  await view({
    codex: entry(30, {
      plan: "ChatGPT Plus",
      windows: [row(30), row(10, { id: "seven_day", kind: "weekly", label: "Weekly limit" })],
    }),
  });
  const names = [...(await openCard())!.querySelectorAll(".plan-window-name")].map(
    (n) => n.textContent,
  );
  expect(names).toEqual(["5-hour limit", "Weekly limit"]);
});

test("Hindi: a Fable window reads Fable की साप्ताहिक सीमा (#236)", async () => {
  await room("claude-acp", "hi");
  await view({ claude: entry(30, { windows: MODEL_ROWS }) });
  const card = await openCard();
  expect(card?.textContent).toContain("Fable की साप्ताहिक सीमा");
  expect(card?.textContent).toContain("Opus की साप्ताहिक सीमा");
  expect(card?.textContent).toContain("Plus monthly cap");
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
  await room("claude-acp", "en", KEY_SIGN_IN);
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

test("signed out: the name alone, with no plan line and no billing line, though the view says pay as you go", async () => {
  await room("claude-acp", "en", SIGNED_OUT);
  await view({
    claude: figureless({
      freshness: "not_applicable",
      billing: "pay_as_you_go",
      provider: "anthropic",
    }),
  });
  expect(shown()).toBe("Claude");
  expect(plan()?.dataset["look"]).toBe("bare");
  expect(plan()?.textContent).not.toMatch(/pay as you go|Billed|plan/);
  expect(plan()?.classList.contains("room-plan-press")).toBe(false);
});

test("signed out with figures: the name alone, no meter, no line and no card", async () => {
  await room("claude-acp", "en", SIGNED_OUT);
  await view({ claude: entry(30) });
  expect(shown()).toBe("Claude");
  expect(plan()?.querySelector(".rounded-meter")).toBeNull();
  expect(plan()?.textContent).not.toMatch(/%|Plenty|plan/);
  expect(plan()?.classList.contains("room-plan-press")).toBe(false);
});

test("codex-acp signed out: the name alone", async () => {
  await room("codex-acp", "en", SIGNED_OUT);
  await view({ codex: entry(30, { plan: "ChatGPT Plus" }) });
  expect(shown()).toBe("ChatGPT");
  expect(plan()?.textContent).not.toMatch(/%|Plenty|plan/);
});

test("signing in (waiting): the name alone until the sign-in is done", async () => {
  await room("claude-acp", "en", WAITING);
  await view({ claude: entry(30) });
  expect(shown()).toBe("Claude");
  expect(plan()?.textContent).not.toMatch(/%|Plenty/);
});

test("a sign-in heard after the view: the line stands once the plan is named", async () => {
  await room("claude-acp", "en", SIGNED_OUT);
  await view({ claude: entry(30) });
  expect(shown()).toBe("Claude");
  await act(async () => {
    h!.transport.setupEvent({
      sign_in_changed: { agent: "claude-acp", state: planSignIn("Claude Max") },
    });
  });
  await settle();
  expect(plan()?.textContent).toContain("Claude plan");
  expect(plan()?.textContent).toContain("Plenty left until the 6:10 pm reset");
});

test("signed in by a key or the firm's endpoint: the line as the view gives it", async () => {
  await room("claude-acp", "en", KEY_SIGN_IN);
  await view({
    claude: figureless({
      freshness: "not_applicable",
      billing: "pay_as_you_go",
      provider: "anthropic",
    }),
  });
  expect(plan()?.textContent).toContain("Claude, pay as you go");
  expect(plan()?.textContent).toContain("Billed to your Anthropic account");
  await act(async () => {
    h!.transport.setupEvent({ sign_in_changed: { agent: "claude-acp", state: FIRM_SIGN_IN } });
  });
  await settle();
  expect(plan()?.textContent).toContain("Claude, pay as you go");
  expect(plan()?.textContent).toContain("Billed to your Anthropic account");
});

test("signed in by a plan login whose plan is not named: the limits' line and meter, as before", async () => {
  await room("claude-acp", "en", NO_PLAN_SIGN_IN);
  await view({ claude: entry(30) });
  expect(plan()?.textContent).toContain("Claude plan");
  expect(plan()?.textContent).toContain("Plenty left until the 6:10 pm reset");
  expect(plan()?.querySelector(".rounded-meter")).not.toBeNull();
});

test("no sign-in heard yet: the name alone, whatever the view holds", async () => {
  await room("claude-acp", "en", null);
  await view({ claude: entry(30) });
  expect(shown()).toBe("Claude");
  expect(plan()?.textContent).not.toMatch(/%|Plenty/);
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
    await bounded("the line's chunks", () => vi.dynamicImportSettled());
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
  await bounded("the card's preload", () => vi.dynamicImportSettled());
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
