// @vitest-environment happy-dom
// Settings, Plan and usage over FakeTransport (U12): each agent's plan, windows and "Updated at", in every state the
// room shows (stale, loading, none with Try again, pay as you go, a window whose reset passed, missing_scope and
// live_only with no entry); shown sent once on opening, try_again from the button, and nothing left on a timer.
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

beforeEach(() => {
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

const today = (hh: number, mm: number) => {
  const d = new Date();
  d.setHours(hh, mm, 0, 0);
  return d.getTime();
};
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

test("loading: the plan's name alone, no figures, never blank", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(heads()).toEqual(["Claude Max"]);
  expect(group("claude")?.textContent).toBe("Claude Max");
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

test("missing_scope and live_only: no entry", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "missing_scope" }), codex: entry(10) });
  expect(card("claude")).toBeNull();
  await view({ claude: entry(30, { freshness: "live_only" }), codex: entry(10) });
  expect(card("claude")).toBeNull();
  expect(card("codex")).not.toBeNull();
});

test("the note beside its name: Claude's 5-hour share left, else the first with figures, Pay as you go, or nothing", async () => {
  await mount();
  expect(usageNote()).toBeNull();
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(usageNote()).toBeNull();
  await view({ claude: entry(38) });
  expect(usageNote()).toBe("62% of the 5-hour limit left");
  await view({ claude: figureless({ freshness: "unavailable" }), codex: entry(10) });
  expect(usageNote()).toBe("90% of the 5-hour limit left");
  await view({
    claude: figureless({ freshness: "not_applicable", billing: "pay_as_you_go" }),
    codex: entry(10),
  });
  expect(usageNote()).toBe("Pay as you go");
  await view({
    claude: entry(30, {
      windows: [row(undefined, { freshness: "reset_passed" })],
    }),
  });
  expect(usageNote()).toBeNull();
});

test("Hindi: the section's words", async () => {
  localStorage.setItem("clauseo.display", JSON.stringify({ language: "hi" }));
  await mount();
  await view({ claude: entry(38) });
  expect(group("claude")?.textContent).toContain("बजे अपडेट किया गया");
  expect(usageNote()).toBe("5 घंटे की सीमा का 62% बाकी");
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

test("a view leaves nothing on a timer", async () => {
  await mount();
  vi.useFakeTimers();
  await view({ claude: entry(96) });
  await act(async () => {
    await vi.runAllTimersAsync();
  });
  expect(card("claude")).not.toBeNull();
  expect(vi.getTimerCount()).toBe(0);
});
