// @vitest-environment happy-dom
// Settings, Plan and usage over FakeTransport (U12): each agent's plan, windows and "Updated at", in every state the
// room shows (stale, loading with its checking line, none with Try again, pay as you go, a window whose reset passed,
// and missing_scope, live_only and a newer state as none); never blank, with one line when no plan has an entry; shown
// sent on opening and each minute while the page is in view (paused while hidden, gone once closed), try_again from
// the button, and a view the host pushes after a turn's end read shown at once.
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type {
  AgentLimitsView,
  LimitMeter,
  LimitWindowView,
  LimitsView,
} from "../../transport/protocol.ts";
import { SettingsApp } from "../settings-app.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

const items = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (key: string) => items.get(key) ?? null,
  setItem: (key: string, value: string) => void items.set(key, value),
  removeItem: (key: string) => void items.delete(key),
  clear: () => items.clear(),
});

let host: HTMLDivElement | null = null;
let root: Root | null = null;
let store: DisplayStore | null = null;
let transport: FakeTransport = new FakeTransport();

const NOW = Date.UTC(2026, 10, 3, 17, 0); // a fixed instant; fixture times derive from NOW

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  localStorage.clear();
  history.replaceState(null, "", "/settings.html#usage");
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  root = null;
  host = null;
  store = null;
  vi.useRealTimers();
});

/** hh:mm on NOW's day (TZ is UTC). */
const today = (hh: number, mm: number) => NOW + ((hh - 17) * 60 + mm) * 60_000;
const RESET = today(18, 10);
const READ = today(0, 5);

const row = (used: number | undefined, over: Partial<LimitWindowView> = {}): LimitWindowView => ({
  id: "five_hour",
  kind: "session",
  label: "5-hour limit",
  ...(used === undefined ? {} : { used_percent: used }),
  tone: "neutral",
  resets_at: RESET,
  freshness: "fresh",
  ...over,
});
const meter = (used: number): LimitMeter => ({
  tone: "neutral",
  used_percent: used,
  window_label: "5-hour limit",
  resets_at: RESET,
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

async function mount(): Promise<HTMLDivElement> {
  transport = new FakeTransport();
  const made = connectDisplay(transport).store;
  store = made;
  const el = document.createElement("div");
  host = el;
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root?.render(<SettingsApp store={made} transport={transport} windowControls={null} />),
  );
  await act(async () => settled());
  return el;
}

const view = (v: LimitsView) => act(async () => transport.limitsChanged(v));
const section = () => document.querySelector<HTMLElement>('[data-testid="usage-section"]');
const card = (kind: "claude" | "codex") =>
  document.querySelector<HTMLElement>(`[data-testid="usage-${kind}"]`);

const group = (kind: "claude" | "codex") => card(kind)?.closest("section") ?? null;
const heads = () => [...section()!.querySelectorAll("h3")].map((h) => h.textContent);
const usageNote = () =>
  [...document.querySelectorAll(".set-nav-row")]
    .find((r) =>
      ["Plan and usage", "प्लान और उपयोग"].includes(
        r.querySelector(".set-nav-name")?.textContent ?? "",
      ),
    )
    ?.querySelector(".set-nav-note")?.textContent ?? null;

test("opening the section sends shown once, for every agent", async () => {
  await mount();
  expect(section()).not.toBeNull();
  expect(transport.refreshes()).toEqual([{ agent: undefined, why: "shown" }]);
  await view({ claude: entry(30) });
  expect(transport.refreshes()).toHaveLength(1);
});

test("each plan's head with Updated at, and each window a tile: the share left, a meter, the reset", async () => {
  await mount();
  await view({ claude: entry(62), codex: entry(10, { plan: "ChatGPT Plus" }) });
  expect(heads()).toEqual(["Claude Max", "ChatGPT Plus"]);
  expect(group("claude")?.querySelector(".set-aside-time")?.textContent).toBe(
    "Updated at 12:05 am",
  );
  const tiles = [...card("claude")!.querySelectorAll<HTMLElement>(".set-window-tile")];
  expect(tiles.map((t) => t.querySelector(".set-window-label")?.textContent)).toEqual([
    "5-hour limit",
    "Weekly limit",
  ]);
  expect(tiles.map((t) => t.querySelector(".set-numeral")?.textContent)).toEqual(["38%", "80%"]);
  expect(tiles[0]?.querySelector(".set-window-word")?.textContent).toBe("left");
  const meter = tiles[0]?.querySelector('[role="meter"]');
  expect(meter?.getAttribute("aria-valuenow")).toBe("38");
  expect(meter?.getAttribute("aria-valuetext")).toBe("38% left");
  expect(tiles[0]?.querySelector(".set-window-reset")?.textContent).toBe("Resets 6:10 pm");
  // 62% used is under the close band; the weekly's 20% too
  expect(tiles.map((t) => t.dataset["state"])).toEqual(["plenty", "plenty"]);
  expect(card("codex")?.querySelector(".set-numeral")?.textContent).toBe("90%");
  expect(document.body.textContent).not.toMatch(/% used|ago/);
});

test("from the close band on, a tile speaks up: 75% used, or the window's own warning", async () => {
  await mount();
  await view({
    claude: entry(75, {
      windows: [row(75), row(20, { id: "seven_day", label: "Weekly limit", tone: "near" })],
    }),
  });
  const tiles = [...card("claude")!.querySelectorAll<HTMLElement>(".set-window-tile")];
  expect(tiles.map((t) => t.dataset["state"])).toEqual(["close", "close"]);
});

test("only the agents in the view get an entry: one with no kind (fake-agent) never does", async () => {
  await mount();
  await view({ codex: entry(10, { plan: "ChatGPT Plus" }) });
  expect(card("claude")).toBeNull();
  expect(card("codex")).not.toBeNull();
  expect(heads()).toEqual(["ChatGPT Plus"]);
});

test("stale: the figures kept, muted, and Updated at speaks up", async () => {
  await mount();
  await view({ claude: entry(30, { freshness: "stale" }) });
  expect(group("claude")?.querySelector(".set-aside-time[data-stale]")?.textContent).toBe(
    "Updated at 12:05 am",
  );
  expect(card("claude")?.querySelectorAll(".set-window-tile[data-stale]")).toHaveLength(2);
  expect(card("claude")?.querySelector(".set-numeral")?.textContent).toBe("70%");
});

test("loading: the plan's name and the checking line, no figures", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(heads()).toEqual(["Claude Max"]);
  expect(group("claude")?.textContent).toBe("Claude MaxChecking what's left…");
  expect(card("claude")?.getAttribute("role")).toBe("status");
  expect(group("claude")?.querySelector('[role="meter"]')).toBeNull();
});

test("none: the message, and Try again sends try_again", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "unavailable" }) });
  expect(card("claude")?.textContent).toContain("Usage isn't available right now.");
  expect(card("claude")?.textContent).toContain(
    "Your Claude plan didn't send its limits this time. Asking still works as usual.",
  );
  // the retry's icon leads its word, hidden; the word names the button
  const tryAgain = group("claude")?.querySelector<HTMLElement>('[data-testid="plan-try-again"]');
  expect(iconIn(tryAgain, IconTryAgain, 16)?.getAttribute("aria-hidden")).toBe("true");
  expect(tryAgain?.textContent).toBe("Try again");
  await act(async () =>
    group("claude")?.querySelector<HTMLElement>('[data-testid="plan-try-again"]')?.click(),
  );
  expect(transport.refreshes()).toEqual([
    { agent: undefined, why: "shown" },
    { agent: "claude", why: "try_again" },
  ]);
});

test("Try again holds no pending state: answered with no view, or refused, it stays pressable and reads nothing", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "unavailable" }) });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const button = () =>
    group("claude")?.querySelector<HTMLButtonElement>('[data-testid="plan-try-again"]') ?? null;
  const idle = () => {
    const b = button();
    expect(b).not.toBeNull();
    expect(b!.disabled).toBe(false);
    expect(b!.getAttribute("aria-disabled")).not.toBe("true");
    expect(b!.hasAttribute("aria-busy")).toBe(false);
    expect(b!.hasAttribute("data-pending")).toBe(false);
    expect(card("claude")?.hasAttribute("aria-busy")).toBe(false);
    expect(card("claude")?.textContent).toContain("Usage isn't available right now.");
  };
  // dropped quietly: the command is done, and no view follows
  transport.host.dispatch = () => "done";
  await act(async () => button()?.click());
  await act(async () => settled());
  idle();
  // refused: the throttle's refusal, and no view follows
  transport.host.dispatch = () => {
    throw new Error("refresh_limits throttled");
  };
  await act(async () => button()?.click());
  await act(async () => settled());
  await act(async () => Promise.resolve());
  idle();
  expect(transport.refreshes()).toEqual([
    { agent: undefined, why: "shown" },
    { agent: "claude", why: "try_again" },
    { agent: "claude", why: "try_again" },
  ]);
  expect(warn).toHaveBeenCalledTimes(1);
  warn.mockRestore();
});

test("pay as you go: who bills, no meter and no figure", async () => {
  await mount();
  await view({
    claude: figureless({
      freshness: "not_applicable",
      billing: "pay_as_you_go",
      provider: "anthropic",
    }),
  });
  expect(heads()).toEqual(["Claude"]);
  expect(card("claude")?.textContent).toContain("Pay as you go, billed to your Anthropic account");
  expect(card("claude")?.textContent).toContain(
    "No limits to show. Each question is billed to the key's account.",
  );
  expect(group("claude")?.querySelector('[role="meter"]')).toBeNull();
  expect(group("claude")?.textContent).not.toMatch(/%|percent/);
});

test("pay as you go with another provider: the plan's title billed by use", async () => {
  await mount();
  await view({ codex: figureless({ freshness: "not_applicable", billing: "pay_as_you_go" }) });
  expect(card("codex")?.textContent).toContain("ChatGPT, pay as you go");
});

test("a window whose reset passed: no percentage, no meter, its reset muted", async () => {
  await mount();
  await view({
    claude: entry(30, {
      windows: [row(undefined, { freshness: "reset_passed", resets_at: today(0, 1) })],
    }),
  });
  const tile = card("claude")?.querySelector<HTMLElement>(".set-window-tile");
  expect(tile?.textContent).not.toMatch(/%/);
  expect(tile?.querySelector('[role="meter"]')).toBeNull();
  expect(tile?.dataset["state"]).toBe("plenty");
  expect(tile?.hasAttribute("data-passed")).toBe(true);
  expect(tile?.querySelector(".set-window-reset")?.textContent).toBe(
    "The limit reset at 12:01 am.",
  );
});

test("missing_scope and live_only: the couldn't-check card with Try again, beside the other plan", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "missing_scope" }), codex: entry(10) });
  expect(card("claude")?.textContent).toContain("Usage isn't available right now.");
  await view({ claude: entry(30, { freshness: "live_only" }), codex: entry(10) });
  expect(card("claude")?.textContent).toContain("Usage isn't available right now.");
  expect(group("claude")?.querySelector('[data-testid="plan-try-again"]')).not.toBeNull();
  expect(card("codex")?.querySelector(".set-numeral")?.textContent).toBe("90%");
});

/** What a plan's entry must show in each state: its figures, the checking line, or the couldn't-check card. */
const SHOWS = {
  figures: "38%",
  loading: "Checking what's left…",
  none: "Usage isn't available right now.",
};
const NEVER_BLANK: [string, LimitsView, Partial<Record<"claude" | "codex", string>>][] = [
  ["{}", {}, {}],
  ["fresh", { claude: entry(62) }, { claude: SHOWS.figures }],
  ["loading", { claude: figureless({ freshness: "loading" }) }, { claude: SHOWS.loading }],
  ["unavailable", { codex: figureless({ freshness: "unavailable" }) }, { codex: SHOWS.none }],
  ["missing_scope", { claude: figureless({ freshness: "missing_scope" }) }, { claude: SHOWS.none }],
  ["live_only", { claude: entry(30, { freshness: "live_only" }) }, { claude: SHOWS.none }],
  [
    "a state from a newer Clauseo",
    { claude: figureless({ freshness: "paused_by_vendor" as AgentLimitsView["freshness"] }) },
    { claude: SHOWS.none },
  ],
  [
    "both plans, one loading",
    { claude: figureless({ freshness: "loading" }), codex: entry(62, { plan: "ChatGPT Plus" }) },
    { claude: SHOWS.loading, codex: SHOWS.figures },
  ],
];

test.each(NEVER_BLANK)("never blank: %s", async (_, limits, shows) => {
  await mount();
  await view(limits);
  expect(section()?.textContent?.trim()).not.toBe("");
  const kinds = Object.keys(shows) as ("claude" | "codex")[];
  if (kinds.length === 0) {
    expect(section()?.textContent).toBe("Set up Claude or ChatGPT to see what each plan has left.");
    expect(heads()).toEqual([]);
  }
  for (const kind of ["claude", "codex"] as const) {
    const want = shows[kind];
    if (want === undefined) {
      expect(card(kind)).toBeNull();
      continue;
    }
    const plan = limits[kind]?.plan ?? "";
    expect(group(kind)?.querySelector("h3")?.textContent).toBe(plan);
    expect(card(kind)?.textContent).toContain(want);
  }
  if (kinds.length > 0) expect(section()?.textContent).not.toContain("Set up Claude or ChatGPT");
});

test("Hindi: the checking line and the line with no plan", async () => {
  localStorage.setItem("clauseo.display", JSON.stringify({ language: "hi" }));
  await mount();
  await view({});
  expect(section()?.textContent).toBe(
    "हर प्लान में कितना बाकी है, यह देखने के लिए Claude या ChatGPT सेट अप करें।",
  );
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(card("claude")?.textContent).toBe("कितना बाकी है, इसकी जाँच हो रही है…");
});

test("the note beside its name: each plan named, its 5-hour share left or pay as you go; nothing without figures", async () => {
  await mount();
  expect(usageNote()).toBeNull();
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(usageNote()).toBeNull();
  await view({ claude: entry(38) });
  expect(usageNote()).toBe("Claude: 62% left");
  await view({ claude: figureless({ freshness: "unavailable" }), codex: entry(10) });
  expect(usageNote()).toBe("ChatGPT: 90% left");
  await view({
    claude: figureless({ freshness: "not_applicable", billing: "pay_as_you_go" }),
    codex: entry(10),
  });
  expect(usageNote()).toBe("Claude: pay as you go · ChatGPT: 90% left");
  await view({
    claude: entry(30, {
      windows: [row(undefined, { freshness: "reset_passed" })],
    }),
  });
  expect(usageNote()).toBeNull();
});

test("with ChatGPT in use, Claude's figure never stands unnamed: both plans, each by its name", async () => {
  // the Windows tester's Westlake matter: a ChatGPT conversation, both plans signed in, Claude's 5-hour at 17% left
  await mount();
  await view({ claude: entry(83), codex: entry(40, { plan: "ChatGPT Plus" }) });
  expect(usageNote()).toBe("Claude: 17% left · ChatGPT: 60% left");
  // ChatGPT alone with figures: its own, named, never Claude's
  await view({ claude: figureless({ freshness: "unavailable" }), codex: entry(40) });
  expect(usageNote()).toBe("ChatGPT: 60% left");
  expect(usageNote()).not.toMatch(/^\d/);
});

/** A Max plan's per-model windows (#236), and an unknown label, as the host sends them. */
const MODEL_ROWS: LimitWindowView[] = [
  row(41, { id: "seven_day_overage_included", kind: "model", label: "x", model: "Fable" }),
  row(12, { id: "seven_day_opus", kind: "model", label: "Opus weekly limit", model: "Opus" }),
  row(5, { id: "monthly_plus", kind: "other", label: "Plus monthly cap" }),
];
const tileLabels = (kind: "claude" | "codex") =>
  [...card(kind)!.querySelectorAll(".set-window-label")].map((l) => l.textContent);

test("a tile's label: {model} weekly limit from the host's model, else the label as given (#236)", async () => {
  await mount();
  await view({
    claude: entry(30, { windows: MODEL_ROWS }),
    codex: entry(10, { plan: "ChatGPT Plus" }),
  });
  expect(tileLabels("claude")).toEqual([
    "Fable weekly limit",
    "Opus weekly limit",
    "Plus monthly cap",
  ]);
  expect(tileLabels("codex")).toEqual(["5-hour limit", "Weekly limit"]);
});

test("Hindi: a Fable tile reads Fable की साप्ताहिक सीमा (#236)", async () => {
  localStorage.setItem("clauseo.display", JSON.stringify({ language: "hi" }));
  await mount();
  await view({ claude: entry(30, { windows: MODEL_ROWS }) });
  expect(tileLabels("claude")).toEqual([
    "Fable की साप्ताहिक सीमा",
    "Opus की साप्ताहिक सीमा",
    "Plus monthly cap",
  ]);
});

test("Hindi: the section's words", async () => {
  localStorage.setItem("clauseo.display", JSON.stringify({ language: "hi" }));
  await mount();
  await view({ claude: entry(38) });
  expect(group("claude")?.textContent).toContain("बजे अपडेट किया गया");
  // the note's Hindi is to come: its English shows meanwhile
  expect(usageNote()).toBe("Claude: 62% left");
  expect(card("claude")?.querySelector(".set-window-word")?.textContent).toBe("बाकी");
});

test("axe finds nothing with both plans, one of them stale and one with nothing sent", async () => {
  await mount();
  await view({
    claude: entry(30, { freshness: "stale" }),
    codex: figureless({ freshness: "unavailable", plan: "ChatGPT Plus" }),
  });
  const results = await axe.run(document, { resultTypes: ["violations"] });
  expect(results.violations.map((v) => v.id)).toEqual([]);
});

/** Fakes the interval timers too, from before the page mounts; settled() keeps its real timeout. */
const fakeTicks = () =>
  vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"], now: NOW });
const tick = (ms: number) => act(async () => vi.advanceTimersByTime(ms));
const SHOWN = { agent: undefined, why: "shown" };

test("a view leaves no timer but the minute's", async () => {
  fakeTicks();
  await mount();
  await view({ claude: entry(96) });
  expect(card("claude")).not.toBeNull();
  expect(vi.getTimerCount()).toBe(1);
});

test("while the page is open, shown goes each minute and never faster; closing it stops the tick", async () => {
  fakeTicks();
  await mount();
  expect(transport.refreshes()).toEqual([SHOWN]);
  await tick(59_999);
  expect(transport.refreshes()).toEqual([SHOWN]);
  await tick(1);
  expect(transport.refreshes()).toEqual([SHOWN, SHOWN]);
  await tick(60_000);
  expect(transport.refreshes()).toEqual([SHOWN, SHOWN, SHOWN]);
  await act(async () => root?.unmount());
  root = null;
  expect(vi.getTimerCount()).toBe(0);
  await tick(5 * 60_000);
  expect(transport.refreshes()).toHaveLength(3);
});

test("a hidden window pauses the tick; shown again, it ticks from then", async () => {
  fakeTicks();
  await mount();
  let state: DocumentVisibilityState = "hidden";
  const spy = vi.spyOn(document, "visibilityState", "get").mockImplementation(() => state);
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  expect(vi.getTimerCount()).toBe(0);
  await tick(3 * 60_000);
  expect(transport.refreshes()).toEqual([SHOWN]);
  state = "visible";
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  await tick(60_000);
  expect(transport.refreshes()).toEqual([SHOWN, SHOWN]);
  spy.mockRestore();
});

test("a tick while the last ask is still out sends nothing more", async () => {
  fakeTicks();
  await mount();
  let answer: (() => void) | undefined;
  transport.host.dispatch = () =>
    new Promise<"done">((resolve) => (answer = () => resolve("done")));
  await tick(60_000);
  await tick(60_000);
  await tick(60_000);
  expect(transport.refreshes()).toEqual([SHOWN, SHOWN]);
  await act(async () => answer?.());
  await tick(60_000);
  expect(transport.refreshes()).toEqual([SHOWN, SHOWN, SHOWN]);
});

test("a turn's end read pushed by the host shows at once, with no ask from the page", async () => {
  await mount();
  await view({ claude: entry(30) });
  expect(card("claude")?.querySelector(".set-numeral")?.textContent).toBe("70%");
  // the host read after the turn ended and sent the view to every window that follows limits
  await view({ claude: entry(55, { checked_at: today(17, 0) }) });
  expect(card("claude")?.querySelector(".set-numeral")?.textContent).toBe("45%");
  expect(group("claude")?.querySelector(".set-aside-time")?.textContent).toBe("Updated at 5:00 pm");
  expect(transport.refreshes()).toEqual([SHOWN]);
});
