// @vitest-environment happy-dom
// Settings, Privacy over FakeTransport: the section on the side; one row per plan the window knows (both while it
// knows none, Claude's alone with Claude's limits only), Brave Search named on Claude's row only; the "On the web"
// group only when the host sends the tools' switches (app-local today), each switch showing its value and a flip
// sent as set_tool_settings with its key; a locked key's switch disabled with its managed line; no Indian Kanoon.
import axe from "axe-core";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { connectDisplay } from "../../display/connect.ts";
import type { DisplayStore } from "../../room/display-store.ts";
import type { Settings, ToolSettings } from "../../room/host-next.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import type { AgentLimitsView, LimitsView } from "../../transport/protocol.ts";
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
const ON: ToolSettings = { web_search: true, browser: true };
const PLAN: AgentLimitsView = {
  plan: "Claude Max",
  billing: "plan",
  windows: [],
  at_limit_text: "",
  freshness: "unavailable",
};

async function mount({
  tools,
  locked = [],
  limits,
  refuse = false,
}: { tools?: ToolSettings; locked?: string[]; limits?: LimitsView; refuse?: boolean } = {}) {
  const settings: Settings = { ui: UI, locked, ...(tools === undefined ? {} : { tools }) };
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

test("Privacy shows on the side, its pane headed and led in the design's words", async () => {
  const { el } = await mount();
  const names = [...el.querySelectorAll(".set-nav-name")].map((n) => n.textContent);
  expect(names).toContain("Privacy");
  expect(el.querySelector(".set-pane h2")?.textContent).toBe("Privacy");
  expect(el.querySelector(".set-pane-lead")?.textContent).toBe(
    "What leaves this computer, and who sees it.",
  );
});

test("knowing no plan, both plans' rows show; Brave Search is named on Claude's row only", async () => {
  const { el } = await mount();
  const claude = el.querySelector('[data-testid="dest-claude"]');
  const chatgpt = el.querySelector('[data-testid="dest-codex"]');
  expect(claude?.textContent).toContain("To Claude, on your plan");
  expect(claude?.textContent).toContain(
    "Each question, the parts of the papers it needs, and its web searches. Claude sees what each question sends it.",
  );
  expect(claude?.textContent).toContain(
    "Anthropic lists Brave Search as its web search subprocessor.",
  );
  expect(chatgpt?.textContent).toContain("To ChatGPT, on your plan");
  expect(chatgpt?.textContent).toContain("ChatGPT sees what each question sends it.");
  expect(chatgpt?.textContent).not.toContain("Brave");
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
  expect(pane(el).textContent).toContain(
    "A change applies to new conversations. Ones already open keep what they had.",
  );
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

test("a locked key's switch is disabled, says it's managed, and a press sends nothing", async () => {
  const { el, transport } = await mount({ tools: ON, locked: ["tools.web_search"] });
  const search = switchNamed(el, "Web search");
  const searchRow = search?.closest(".set-row");
  expect(search?.hasAttribute("data-disabled")).toBe(true);
  expect(searchRow?.querySelector('[data-testid="managed"]')?.textContent).toBe(
    "Managed by your organisation",
  );
  const browserRow = switchNamed(el, "Browser")?.closest(".set-row");
  expect(browserRow?.querySelector('[data-testid="managed"]')).toBeNull();
  await act(async () => search?.click());
  await act(async () => settled());
  expect(transport.only("next_command")).toEqual([]);
  expect(search?.getAttribute("aria-checked")).toBe("true");
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

test("axe finds nothing on Privacy with the switches, one locked", async () => {
  await mount({ tools: ON, locked: ["tools.browser"] });
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
