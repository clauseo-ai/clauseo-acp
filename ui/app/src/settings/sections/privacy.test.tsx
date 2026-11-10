// @vitest-environment happy-dom
// Settings, Privacy over FakeTransport: the section on the side; one row per plan the window knows (both while it
// knows none, Claude's alone with Claude's limits only), Brave Search behind an info icon on Claude's row only; the
// websites' row with its visit sentence behind an info icon, unless both tools are off; the "On the web" group only
// when the host sends the tools' switches (SettingsView.tools), each row led by its tool's glyph with the state on it and
// one helper line at most, each switch showing its value and a flip sent as set_tool_settings with its key; a locked
// key's switch disabled, showing the host's value, its glyph locked, U11-07's line in its helper's place; the Browser
// switch's helper the same on or off (no "Not available yet" since #870); the nav's icon not the web's globe; no
// Indian Kanoon. The anonymous counts' switch (D-052) only when the host sends SettingsView.privacy, in the first
// run's words, a flip sent as set_privacy_settings, a firm's lock shown off and disabled, a host change shown live.
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { Settings } from "../../room/host-next.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type {
  AgentLimitsView,
  LimitsView,
  PrivacySettings,
  ToolsSettingsView,
} from "../../transport/protocol.ts";
import { IconSafeWorkspace, IconWebsite } from "@clauseo/components/icons/icons.tsx";
import { SECTIONS } from "../sections.tsx";
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

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/settings.html#privacy");
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  store?.dispose();
  root = null;
  host = null;
  store = null;
});

const UI: Settings["ui"] = {
  theme: "system",
  font_size: 16,
  language: "en",
  lite: "automatic",
  motion: "system",
};
const ON: ToolsSettingsView = { web_search: true, browser: true };
const PLAN: AgentLimitsView = {
  plan: "Claude Max",
  billing: "plan",
  windows: [],
  at_limit_text: "",
  freshness: "unavailable",
};

async function mount({
  tools,
  privacy,
  locked = [],
  limits,
  refuse = false,
}: {
  tools?: ToolsSettingsView;
  privacy?: PrivacySettings;
  locked?: string[];
  limits?: LimitsView;
  refuse?: boolean;
} = {}) {
  const settings: Settings = {
    ui: UI,
    locked,
    ...(tools === undefined ? {} : { tools }),
    ...(privacy === undefined ? {} : { privacy }),
  };
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings };
        throw new Error("not scripted");
      },
      // the host refuses a write to a key its policy locks: settings.locked
      command: () => {
        if (refuse) throw { code: "settings.locked", message: "locked" };
      },
    },
  });
  if (limits !== undefined) transport.limitsChanged(limits);
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
  return { el, transport };
}

const pane = (el: HTMLElement) => el.querySelector(".set-pane") as HTMLElement;
const switchNamed = (el: HTMLElement, label: string) => {
  const row = [...el.querySelectorAll(".set-row")].find((r) => r.textContent?.startsWith(label));
  return row?.querySelector<HTMLElement>('[role="switch"]') ?? null;
};

test("Privacy shows on the side, its pane headed, with no lead that repeats its first heading", async () => {
  const { el } = await mount();
  const names = [...el.querySelectorAll(".set-nav-name")].map((n) => n.textContent);
  expect(names).toContain("Privacy");
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Privacy");
  expect(el.querySelector(".set-pane-lead")).toBeNull();
  const text = pane(el).textContent ?? "";
  expect(text.split("What leaves this computer").length - 1).toBe(1);
});

const WHAT = "Questions, the papers' parts they need, web searches.";
const BRAVE = "Anthropic lists Brave Search as its web search subprocessor.";
const VISIT = "Websites see Clauseo's visits, as they see any browser's.";
const tipButton = (el: HTMLElement, label: string) =>
  el.querySelector<HTMLElement>(`button[aria-label="${label}"]`);

test("knowing no plan, both plans' rows show their to-line alone; Brave Search waits behind Claude's info icon only", async () => {
  const { el } = await mount();
  const claude = el.querySelector<HTMLElement>('[data-testid="dest-claude"]');
  const chatgpt = el.querySelector<HTMLElement>('[data-testid="dest-codex"]');
  expect(claude?.textContent).toBe("To Claude, on your plan");
  expect(chatgpt?.textContent).toBe("To ChatGPT, on your plan");
  expect(document.body.textContent).not.toContain(BRAVE);
  expect(tipButton(claude!, "More about Brave Search")).not.toBeNull();
  expect(chatgpt?.querySelector("button")).toBeNull();
  await act(async () => tipButton(claude!, "More about Brave Search")?.click());
  await act(async () => settled());
  expect(document.body.textContent).toContain(BRAVE);
});

test("with two plans, what goes to a plan shows once, as the group's note under its head", async () => {
  const { el } = await mount();
  expect(el.querySelectorAll('[data-testid^="dest-"]').length).toBeGreaterThanOrEqual(2);
  const text = pane(el).textContent ?? "";
  expect(text.split(WHAT).length - 1).toBe(1);
  const note = [...el.querySelectorAll(".set-group-note")].find((n) => n.textContent === WHAT);
  expect(note?.closest("section")?.querySelector("h3")?.textContent).toBe(
    "What leaves this computer",
  );
  expect(text).not.toContain("Each question");
});

test("with Claude's limits alone, only Claude's row shows", async () => {
  const { el } = await mount({ limits: { claude: PLAN } });
  expect(el.querySelector('[data-testid="dest-claude"]')).not.toBeNull();
  expect(el.querySelector('[data-testid="dest-codex"]')).toBeNull();
});

test("without the tools from the host, no On the web group and no switch", async () => {
  const { el } = await mount();
  expect(pane(el).textContent).not.toContain("On the web");
  expect(pane(el).querySelector('[role="switch"]')).toBeNull();
});

test("with the tools, each switch shows its value, and a flip sends set_tool_settings with its key", async () => {
  const { el, transport } = await mount({ tools: { web_search: false, browser: true } });
  expect(pane(el).textContent).toContain("On the web");
  expect(pane(el).textContent).toContain("Changes apply to new conversations.");
  const search = switchNamed(el, "Web search");
  const browser = switchNamed(el, "Browser");
  expect(search?.getAttribute("aria-checked")).toBe("false");
  expect(browser?.getAttribute("aria-checked")).toBe("true");
  await act(async () => search?.click());
  await act(async () => settled());
  await act(async () => browser?.click());
  await act(async () => settled());
  expect(transport.only("next_command").map((c) => c.command)).toEqual([
    { set_tool_settings: { patch: { web_search: true } } },
    { set_tool_settings: { patch: { browser: false } } },
  ]);
  expect(switchNamed(el, "Web search")?.getAttribute("aria-checked")).toBe("true");
  expect(switchNamed(el, "Browser")?.getAttribute("aria-checked")).toBe("false");
});

const SEARCH_NOTE = "Search the web and read pages.";
const BROWSER_NOTE = "Runs on this computer.";
// the room popover's lock wording, one for both
const LOCKED = "Managed by firm";
// UEP-16, retired after #870: the browser runs without a website check, so this never shows
const NO_CHECK = "Not available yet";
/** A tool row's leading glyph, and the state drawn on it: a slash for off, a lock for a firm's lock, neither for on. */
const glyphOf = (row: Element | null | undefined) => {
  const svg = row?.querySelector('[data-part="icon"] svg');
  if (svg === null || svg === undefined) return null;
  if (svg.querySelector('[data-part="slash"]') !== null) return "off";
  if (svg.querySelector('[data-part="lock"]') !== null) return "locked";
  return "on";
};

test("a locked key's switch is disabled, shows the host's value, gives the lock's reason for its note, and a press sends nothing", async () => {
  const { el, transport } = await mount({
    tools: { web_search: false, browser: true },
    locked: ["tools.web_search"],
  });
  const search = switchNamed(el, "Web search");
  const searchRow = search?.closest(".set-row");
  expect(search?.hasAttribute("data-disabled")).toBe(true);
  expect(search?.getAttribute("aria-checked")).toBe("false");
  expect(searchRow?.textContent).not.toContain(SEARCH_NOTE);
  expect(searchRow?.textContent).toBe(`Web search${LOCKED}`);
  expect(searchRow?.textContent).not.toContain("organisation");
  const browserRow = switchNamed(el, "Browser")?.closest(".set-row");
  expect(browserRow?.textContent).not.toContain(LOCKED);
  await act(async () => search?.click());
  await act(async () => settled());
  expect(transport.only("next_command")).toEqual([]);
  expect(search?.getAttribute("aria-checked")).toBe("false");
});

test("unlocked, a switch keeps its note and gives no lock's reason", async () => {
  const { el } = await mount({ tools: ON });
  const searchRow = switchNamed(el, "Web search")?.closest(".set-row");
  expect(searchRow?.textContent).toContain(SEARCH_NOTE);
  expect(searchRow?.textContent).not.toContain(LOCKED);
});

test("the websites' row follows the plans' rows, and shows without the tools or with either on", async () => {
  for (const tools of [
    undefined,
    ON,
    { web_search: false, browser: true },
    { web_search: true, browser: false },
  ]) {
    const { el } = await mount(tools === undefined ? {} : { tools });
    const row = el.querySelector('[data-testid="dest-websites"]');
    expect(row?.textContent).toBe("To the websites Clauseo visits");
    expect(row?.previousElementSibling?.getAttribute("data-testid")).toMatch(
      /^dest-(claude|codex)$/,
    );
    await act(async () => root?.unmount());
    host?.remove();
    store?.dispose();
    root = null;
    host = null;
    store = null;
  }
});

test("each tool row leads with its glyph, hidden from assistive tech, the state drawn on it", async () => {
  const { el } = await mount({ tools: { web_search: true, browser: false } });
  const searchRow = switchNamed(el, "Web search")?.closest(".set-row");
  const browserRow = switchNamed(el, "Browser")?.closest(".set-row");
  expect(glyphOf(searchRow)).toBe("on");
  expect(glyphOf(browserRow)).toBe("off");
  for (const row of [searchRow, browserRow]) {
    const svg = row?.querySelector('[data-part="icon"] svg');
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.getAttribute("width")).toBe("16");
  }
  // the glyph follows a flip
  await act(async () => switchNamed(el, "Browser")?.click());
  await act(async () => settled());
  expect(glyphOf(switchNamed(el, "Browser")?.closest(".set-row"))).toBe("on");
  // the switch's name is the tool's alone
  const label = switchNamed(el, "Web search")?.closest(".set-row")?.querySelector("label");
  expect(label?.textContent).toBe("Web search");
});

test("a locked tool's glyph carries the lock", async () => {
  const { el } = await mount({ tools: ON, locked: ["tools.web_search"] });
  expect(glyphOf(switchNamed(el, "Web search")?.closest(".set-row"))).toBe("locked");
  expect(glyphOf(switchNamed(el, "Browser")?.closest(".set-row"))).toBe("on");
});

test("each tool row says one helper line at most, in every state", async () => {
  for (const state of [
    { tools: ON },
    { tools: { web_search: false, browser: false } },
    { tools: ON, locked: ["tools.web_search", "tools.browser"] },
  ]) {
    const { el } = await mount(state);
    for (const name of ["Web search", "Browser"]) {
      const row = switchNamed(el, name)?.closest(".set-row");
      const label = row?.querySelector("label")?.textContent ?? "";
      expect(row?.textContent?.startsWith(label)).toBe(true);
      expect([SEARCH_NOTE, BROWSER_NOTE, LOCKED]).toContain(row?.textContent?.slice(label.length));
    }
    await act(async () => root?.unmount());
    host?.remove();
    store?.dispose();
    root = null;
    host = null;
    store = null;
  }
});

test("the websites' visit sentence waits behind its info icon", async () => {
  const { el } = await mount();
  const row = el.querySelector<HTMLElement>('[data-testid="dest-websites"]');
  expect(row?.textContent).toBe("To the websites Clauseo visits");
  expect(document.body.textContent).not.toContain(VISIT);
  await act(async () => tipButton(row!, "About websites")?.click());
  await act(async () => settled());
  expect(document.body.textContent).toContain(VISIT);
});

test("with both tools off, no websites' row", async () => {
  const { el } = await mount({ tools: { web_search: false, browser: false } });
  expect(el.querySelector('[data-testid="dest-websites"]')).toBeNull();
});

test.each([true, false])(
  "the Browser switch gives its helper whether on or off (%s), and never Not available yet",
  async (on) => {
    const { el } = await mount({ tools: { web_search: true, browser: on } });
    expect(pane(el).textContent).not.toContain(NO_CHECK);
    expect(switchNamed(el, "Browser")?.closest(".set-row")?.textContent).toBe(
      `Browser${BROWSER_NOTE}`,
    );
  },
);

test("Privacy's nav icon is neither the web's globe nor the Safe workspace's lock", () => {
  const privacy = SECTIONS.find((s) => s.id === "privacy");
  expect(privacy?.icon).toBeDefined();
  expect(privacy?.icon).not.toBe(IconWebsite);
  expect(privacy?.icon).not.toBe(IconSafeWorkspace);
});

test("the section says nothing of Indian Kanoon, in either state", async () => {
  for (const state of [{}, { tools: ON }]) {
    const { el } = await mount(state);
    expect(pane(el).textContent).not.toContain("Kanoon");
    await act(async () => root?.unmount());
    host?.remove();
    store?.dispose();
    root = null;
    host = null;
    store = null;
  }
});

test("axe finds nothing on Privacy with the switches, one locked, and an info tip open", async () => {
  const { el } = await mount({ tools: ON, locked: ["tools.browser"] });
  await act(async () => tipButton(el, "More about Brave Search")?.click());
  await act(async () => settled());
  const results = await axe.run(document, { resultTypes: ["violations"] });
  expect(results.violations.map((v) => v.id)).toEqual([]);
});

test("a write the host refuses (settings.locked) leaves the switch where it was, and says nothing new", async () => {
  const { el, transport } = await mount({ tools: ON, refuse: true });
  const before = pane(el).textContent;
  await act(async () => switchNamed(el, "Web search")?.click());
  await act(async () => settled());
  expect(transport.only("next_command")).toHaveLength(1);
  expect(switchNamed(el, "Web search")?.getAttribute("aria-checked")).toBe("true");
  expect(pane(el).textContent).toBe(before);
});

const COUNTS = "Send anonymous counts";
const COUNTS_NOTE =
  "How many questions, which features, how long answers take. Never your papers, your questions or the answers.";
const PRIVACY_ON: PrivacySettings = { counts: true, crash_reports: false, counts_started: true };

test("without the privacy switches from the host, no anonymous counts' switch", async () => {
  const { el } = await mount({ tools: ON });
  expect(switchNamed(el, COUNTS)).toBeNull();
});

test("the anonymous counts' switch says the first run's words, shows the host's value, and a flip sends set_privacy_settings", async () => {
  const { el, transport } = await mount({ privacy: PRIVACY_ON });
  const counts = switchNamed(el, COUNTS);
  expect(counts?.closest(".set-row")?.textContent).toBe(`${COUNTS}${COUNTS_NOTE}`);
  expect(counts?.getAttribute("aria-checked")).toBe("true");
  await act(async () => counts?.click());
  await act(async () => settled());
  expect(transport.only("next_command").map((c) => c.command)).toEqual([
    { set_privacy_settings: { patch: { counts: false } } },
  ]);
  expect(switchNamed(el, COUNTS)?.getAttribute("aria-checked")).toBe("false");
  await act(async () => switchNamed(el, COUNTS)?.click());
  await act(async () => settled());
  expect(
    transport
      .only("next_command")
      .map((c) => c.command)
      .at(-1),
  ).toEqual({
    set_privacy_settings: { patch: { counts: true } },
  });
});

test("a firm's lock on privacy.counts shows the switch off and disabled, says so, and a press sends nothing", async () => {
  const { el, transport } = await mount({
    privacy: { ...PRIVACY_ON, counts: false },
    locked: ["privacy.counts"],
  });
  const counts = switchNamed(el, COUNTS);
  expect(counts?.getAttribute("aria-checked")).toBe("false");
  expect(counts?.hasAttribute("data-disabled")).toBe(true);
  expect(counts?.closest(".set-row")?.textContent).toBe(`${COUNTS}${COUNTS_NOTE}${LOCKED}`);
  await act(async () => counts?.click());
  await act(async () => settled());
  expect(transport.only("next_command")).toEqual([]);
});

test("the other window's change to the counts reaches the switch live", async () => {
  const { el, transport } = await mount({ privacy: PRIVACY_ON });
  expect(switchNamed(el, COUNTS)?.getAttribute("aria-checked")).toBe("true");
  await act(async () =>
    transport.settingsChanged({ ui: UI, locked: [], privacy: { ...PRIVACY_ON, counts: false } }),
  );
  expect(switchNamed(el, COUNTS)?.getAttribute("aria-checked")).toBe("false");
});
