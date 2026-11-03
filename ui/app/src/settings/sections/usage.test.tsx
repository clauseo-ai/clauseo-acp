// @vitest-environment happy-dom
// Settings, Plan and usage over FakeTransport (U12): each agent's plan, windows and "Updated at", in every state the
// room shows (stale, loading, none with Try again, pay as you go, a window whose reset passed, missing_scope and
// live_only with no entry); shown sent once on opening, try_again from the button, and nothing left on a timer.
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

test("opening the section sends shown once, for every agent", async () => {
  await mount();
  expect(section()).not.toBeNull();
  expect(transport.refreshes()).toEqual([{ agent: undefined, why: "shown" }]);
  await view({ claude: entry(30) });
  expect(transport.refreshes()).toHaveLength(1);
});

test("each agent's plan: windows with what's left, a bar and the reset, and Updated at", async () => {
  await mount();
  await view({ claude: entry(62), codex: entry(10, { plan: "ChatGPT Plus" }) });
  const groups = [...section()!.querySelectorAll("h3")].map((h) => h.textContent);
  expect(groups).toEqual(["Claude", "ChatGPT"]);
  const claude = card("claude");
  expect(claude?.textContent).toContain("Claude Max");
  expect(claude?.textContent).toContain("Updated at 12:05 am");
  expect(claude?.textContent).not.toContain("As of");
  expect(claude?.textContent).toContain("5-hour limit");
  expect(claude?.textContent).toContain("38% left");
  expect(claude?.textContent).toContain("Weekly limit");
  expect(claude?.textContent).toContain("80% left");
  expect(claude?.textContent).toContain("Resets 6:10 pm");
  expect(claude?.querySelectorAll(".plan-track")).toHaveLength(2);
  expect(card("codex")?.textContent).toContain("ChatGPT Plus");
  expect(card("codex")?.textContent).toContain("90% left");
});

test("only the agents in the view get an entry: one with no kind (fake-agent) never does", async () => {
  await mount();
  await view({ codex: entry(10) });
  expect(card("claude")).toBeNull();
  expect(card("codex")).not.toBeNull();
  expect([...section()!.querySelectorAll("h3")].map((h) => h.textContent)).toEqual(["ChatGPT"]);
});

test("stale: the figures kept and muted", async () => {
  await mount();
  await view({ claude: entry(30, { freshness: "stale" }) });
  expect(card("claude")?.querySelector(".plan-card-stale")?.textContent).toBe(
    "Updated at 12:05 am",
  );
  expect(card("claude")?.querySelector(".plan-window[data-stale]")).not.toBeNull();
});

test("loading: the plan's name alone, no window and no time", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "loading" }) });
  expect(card("claude")?.textContent).toBe("Claude Max");
  expect(card("claude")?.querySelector(".plan-track")).toBeNull();
});

test("none: the message, and Try again sends try_again", async () => {
  await mount();
  await view({ claude: figureless({ freshness: "unavailable" }) });
  expect(card("claude")?.textContent).toContain(
    "Your Claude plan didn't send its limits this time. Asking still works as usual.",
  );
  await act(async () =>
    card("claude")?.querySelector<HTMLElement>('[data-testid="plan-try-again"]')?.click(),
  );
  expect(transport.refreshes()).toEqual([
    { agent: undefined, why: "shown" },
    { agent: "claude", why: "try_again" },
  ]);
});

test("pay as you go: the title and who bills, no bar and no figure", async () => {
  await mount();
  await view({
    claude: figureless({
      freshness: "not_applicable",
      billing: "pay_as_you_go",
      provider: "anthropic",
    }),
  });
  expect(card("claude")?.textContent).toContain("Claude, pay as you go");
  expect(card("claude")?.textContent).toContain("Pay as you go, billed to your Anthropic account");
  expect(card("claude")?.querySelector(".plan-track")).toBeNull();
  expect(card("claude")?.textContent).not.toMatch(/%|percent/);
});

test("a window whose reset passed: no percentage, no track, its reset muted", async () => {
  await mount();
  await view({
    claude: entry(30, {
      windows: [row(undefined, { freshness: "reset_passed", resets_at: today(0, 1) })],
    }),
  });
  expect(card("claude")?.textContent).not.toMatch(/%/);
  expect(card("claude")?.querySelector(".plan-track")).toBeNull();
  expect(card("claude")?.querySelector(".plan-window-reset")?.textContent).toBe(
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

test("Hindi: Updated at in the section's words", async () => {
  localStorage.setItem("clauseo.display", JSON.stringify({ language: "hi" }));
  await mount();
  await view({ claude: entry(30) });
  expect(card("claude")?.textContent).toContain("बजे अपडेट किया गया");
});

test("axe finds nothing with both plans, one of them stale and one with nothing sent", async () => {
  await mount();
  await view({
    claude: entry(30, { freshness: "stale" }),
    codex: figureless({ freshness: "unavailable" }),
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
