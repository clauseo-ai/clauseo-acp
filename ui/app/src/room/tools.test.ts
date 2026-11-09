// The tool line's pure reading: which tools a header's list speaks for, each glyph's state and the line's name for every
// pair of states, and each popover row's words for every ToolState. The Chrome download's words are in
// tools.download.test.ts.
import { expect, test } from "vitest";
import type { ToolState } from "../transport/protocol.ts";
import { toolRow } from "./tool-rows.ts";
import {
  glyphState,
  knownTools,
  sameTools,
  toolsLabel,
  type BrowserState,
  type KnownTool,
  type WebState,
} from "./tools.ts";
import { LIST_EXPIRED, NO_LIST, POLICY, ROOM, UNREADABLE, words } from "./tools-support.ts";

const WEB: WebState[] = ["available", "off", "locked_off", "no_search_on_this_login"];
const BROWSER: BrowserState[] = [
  "available",
  "off",
  "locked_off",
  "managed_by_firm",
  "no_proxy",
  "too_many_browsers",
  "browser_not_installed",
  "browser_blocked_by_policy",
  "browser_policy_unreadable",
  "browser_out_of_date",
  "browser_list_unavailable",
  "browser_list_expired",
];
const avail = (state: string): ToolState["availability"] =>
  state === "available" ? "available" : ({ unavailable: { reason: state } } as never);

test("knownTools keeps the tools and states it has words for, web search first, each once", () => {
  expect(knownTools(undefined)).toEqual([]);
  expect(knownTools([])).toEqual([]);
  expect(
    knownTools([
      { tool: "browser", availability: avail("no_proxy") },
      { tool: "web_search", availability: "available" },
      { tool: "web_search", availability: avail("off") },
    ]),
  ).toEqual([
    { tool: "web_search", state: "available" },
    { tool: "browser", state: "no_proxy" },
  ]);
  for (const w of WEB)
    expect(knownTools([{ tool: "web_search", availability: avail(w) }])).toEqual([
      { tool: "web_search", state: w },
    ]);
  for (const b of BROWSER)
    expect(knownTools([{ tool: "browser", availability: avail(b) }])).toEqual([
      { tool: "browser", state: b },
    ]);
});

test("a tool, an availability or a reason it doesn't know, or a reason with no words on that tool, says nothing", () => {
  expect(
    knownTools([
      { tool: "unknown", availability: "available" },
      { tool: "web_search", availability: "something_newer" },
      { tool: "web_search", availability: avail("unknown") },
      { tool: "browser", availability: avail("unknown") },
      { tool: "browser", availability: avail("no_search_on_this_login") },
      { tool: "web_search", availability: avail("no_proxy") },
      { tool: "browser", availability: avail("a_newer_reason") },
    ]),
  ).toEqual([]);
});

const label = (web: WebState | null, browser: BrowserState | null) => {
  const tools: KnownTool[] = [];
  if (web !== null) tools.push({ tool: "web_search", state: web });
  if (browser !== null) tools.push({ tool: "browser", state: browser });
  return toolsLabel("en", tools);
};

test("each glyph's state: on as drawn, the firm's lock for locked off, managed by firm and its policy, every other off slashed", () => {
  const web: Record<WebState, string> = {
    available: "on",
    off: "off",
    locked_off: "locked",
    no_search_on_this_login: "off",
  };
  const browser: Record<BrowserState, string> = {
    available: "on",
    off: "off",
    locked_off: "locked",
    managed_by_firm: "locked",
    no_proxy: "off",
    too_many_browsers: "off",
    browser_not_installed: "off",
    browser_blocked_by_policy: "locked",
    // not read is no firm's lock
    browser_policy_unreadable: "off",
    browser_out_of_date: "off",
    browser_list_unavailable: "off",
    browser_list_expired: "off",
  };
  for (const w of WEB) expect(glyphState({ tool: "web_search", state: w })).toBe(web[w]);
  for (const b of BROWSER) expect(glyphState({ tool: "browser", state: b })).toBe(browser[b]);
});

test("the line's name and tooltip: one sentence per tool, web search first, its reason when off", () => {
  expect(label(null, null)).toBeNull();
  expect(label("available", "browser_not_installed")).toBe(
    "Web search: on. Browser: off, needs Chrome.",
  );
  expect(label("available", "available")).toBe("Web search: on. Browser: on.");
  expect(label("off", null)).toBe("Web search: off, in Settings.");
  expect(label(null, "off")).toBe("Browser: off, in Settings.");
  expect(label("locked_off", "managed_by_firm")).toBe(
    "Web search: off, managed by firm. Browser: off, managed by firm.",
  );
  expect(label("no_search_on_this_login", "no_proxy")).toBe(
    "Web search: off, can't search on this login. Browser: off, not available yet.",
  );
  expect(label(null, "too_many_browsers")).toBe("Browser: off, too many browsers open.");
  expect(label(null, "locked_off")).toBe("Browser: off, managed by firm.");
  expect(label("available", "browser_blocked_by_policy")).toBe(
    "Web search: on. Browser: off, blocked by your firm.",
  );
  expect(label(null, "browser_policy_unreadable")).toBe(
    "Browser: off, couldn't check browser settings.",
  );
  // Chrome keeps its capital mid-sentence
  expect(label(null, "browser_out_of_date")).toBe("Browser: off, needs a newer Chrome.");
  expect(label(null, "browser_list_unavailable")).toBe(
    "Browser: off, couldn't check Chrome's version.",
  );
  expect(label(null, "browser_list_expired")).toBe("Browser: off, can't update Chrome right now.");
  // a notice is the popover's alone: the line says only on
  expect(
    toolsLabel("en", [{ tool: "browser", state: "available", notice: "browser_list_old" }]),
  ).toBe("Browser: on.");
});

test("knownTools keeps a browser's notice only while it is on, and one it doesn't know is left out", () => {
  const on = { tool: "browser", availability: "available" } as const;
  expect(knownTools([{ ...on, notice: "browser_list_old" } as never])).toEqual([
    { tool: "browser", state: "available", notice: "browser_list_old" },
  ]);
  expect(knownTools([{ ...on, notice: "a_newer_notice" } as never])).toEqual([
    { tool: "browser", state: "available" },
  ]);
  expect(knownTools([on])).toEqual([{ tool: "browser", state: "available" }]);
  expect(
    knownTools([
      {
        tool: "browser",
        availability: avail("browser_out_of_date"),
        notice: "browser_list_old",
      } as never,
    ]),
  ).toEqual([{ tool: "browser", state: "browser_out_of_date" }]);
});

test("sameTools reads two lists the same only when every tool and state match", () => {
  const a: KnownTool[] = [{ tool: "browser", state: "off" }];
  expect(sameTools(a, [{ tool: "browser", state: "off" }])).toBe(true);
  expect(sameTools(a, [{ tool: "browser", state: "no_proxy" }])).toBe(false);
  expect(sameTools(a, [])).toBe(false);
  // a notice that comes or goes is a change
  const on: KnownTool[] = [{ tool: "browser", state: "available" }];
  const noticed: KnownTool[] = [
    { tool: "browser", state: "available", notice: "browser_list_old" },
  ];
  expect(sameTools(on, noticed)).toBe(false);
  expect(
    sameTools(noticed, [{ tool: "browser", state: "available", notice: "browser_list_old" }]),
  ).toBe(true);
});

test("each row: its glyph, name, an On or Off chip, and the reason in a few words only when off", () => {
  const reasons: Record<string, string | null> = {
    available: null,
    off: "In Settings",
    locked_off: "Managed by firm",
    managed_by_firm: "Managed by firm",
    no_search_on_this_login: "Can't search on this login",
    no_proxy: "Not available yet",
    too_many_browsers: "Too many browsers open",
    browser_not_installed: "Needs Chrome",
    browser_blocked_by_policy: "Blocked by your firm",
    browser_policy_unreadable: "Couldn't check browser settings",
    browser_out_of_date: "Needs a newer Chrome",
    browser_list_unavailable: "Couldn't check Chrome's version",
    browser_list_expired: "Can't update Chrome right now",
  };
  const notes: Record<string, string> = {
    browser_blocked_by_policy: POLICY,
    browser_policy_unreadable: UNREADABLE,
    browser_list_unavailable: NO_LIST,
    browser_list_expired: LIST_EXPIRED,
  };
  const rows: KnownTool[] = [
    ...WEB.map((state): KnownTool => ({ tool: "web_search", state })),
    ...BROWSER.map((state): KnownTool => ({ tool: "browser", state })),
  ];
  for (const tool of rows) {
    const row = words(tool);
    const on = tool.state === "available";
    expect(row).toEqual({
      tool: tool.tool,
      glyph: glyphState(tool),
      name: tool.tool === "web_search" ? "Web search" : "Browser",
      on,
      chip: on ? "On" : "Off",
      reason: reasons[tool.state],
      // In Settings sits in the chip's row
      beside: tool.state === "off",
      reasonTip: null,
      // the firm's policy, a policy not read, a version not checked, or a Chrome too old that can't be
      // updated yet, says it in full
      note: notes[tool.state] ?? null,
      settings: tool.state === "off",
      download: null,
    });
  }
});

test("Open Settings is offered only for Off in Settings, with the settings window", () => {
  expect(words({ tool: "web_search", state: "off" }).settings).toBe(true);
  expect(words({ tool: "web_search", state: "off" }, { settings: false }).settings).toBe(false);
  expect(words({ tool: "browser", state: "locked_off" }).settings).toBe(false);
  expect(words({ tool: "browser", state: "no_proxy" }).settings).toBe(false);
});

test("in Hindi, the tools' words say their Hindi, and a reason keeps its brand name's capital", () => {
  expect(toolRow("hi", { tool: "browser", state: "available" }, ROOM).name).toBe("ब्राउज़र");
  expect(toolRow("hi", { tool: "browser", state: "locked_off" }, ROOM).reason).toBe(
    "फ़र्म द्वारा प्रबंधित",
  );
  expect(toolsLabel("hi", [{ tool: "browser", state: "browser_not_installed" }])).toBe(
    "ब्राउज़र: बंद, Chrome चाहिए।",
  );
  // provisional Hindi, until the app thread writes it
  expect(toolsLabel("hi", [{ tool: "browser", state: "browser_blocked_by_policy" }])).toBe(
    "ब्राउज़र: बंद, आपकी फ़र्म ने रोका है।",
  );
});
