// The tool line's pure reading: which tools a header's list speaks for, the status line's phrase for every pair of
// states, and each popover row's words for every ToolState and every Chrome download state.
import { expect, test } from "vitest";
import type { ToolState } from "../transport/protocol.ts";
import type { BrowserInstall } from "./browser-install.ts";
import { toolRow, type ToolRowRoom } from "./tool-rows.ts";
import {
  knownTools,
  sameTools,
  toolsSummary,
  type BrowserState,
  type KnownTool,
  type WebState,
} from "./tools.ts";

const WEB: WebState[] = ["available", "off", "locked_off", "no_search_on_this_login"];
const BROWSER: BrowserState[] = [
  "available",
  "off",
  "locked_off",
  "managed_by_firm",
  "no_proxy",
  "too_many_browsers",
  "browser_not_installed",
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

const phrase = (web: WebState | null, browser: BrowserState | null) => {
  const tools: KnownTool[] = [];
  if (web !== null) tools.push({ tool: "web_search", state: web });
  if (browser !== null) tools.push({ tool: "browser", state: browser });
  return toolsSummary(tools);
};

test("the status line's phrase, over every pair of states", () => {
  expect(phrase(null, null)).toBeNull();
  for (const b of [null, ...BROWSER] as const) {
    const on = b === "available";
    expect(phrase("available", b)).toBe(on ? "Web search and browser on" : "Web search on");
    expect(phrase("no_search_on_this_login", b)).toBe(
      on ? "Web pages and browser on" : "Web pages only",
    );
    for (const w of ["off", "locked_off"] as const)
      expect(phrase(w, b)).toBe(on ? "Browser on, no web search" : "Web search off");
    if (b !== null) expect(phrase(null, b)).toBe(on ? "Browser on" : "Browser off");
  }
});

test("sameTools reads two lists the same only when every tool and state match", () => {
  const a: KnownTool[] = [{ tool: "browser", state: "off" }];
  expect(sameTools(a, [{ tool: "browser", state: "off" }])).toBe(true);
  expect(sameTools(a, [{ tool: "browser", state: "no_proxy" }])).toBe(false);
  expect(sameTools(a, [])).toBe(false);
});

const ROOM: ToolRowRoom = { plan: "Claude", settings: true, install: null };
const words = (tool: KnownTool, room: Partial<ToolRowRoom> = {}) =>
  toolRow("en", tool, { ...ROOM, ...room });

test("each web search row's words, and Open Settings only for Off with the settings window", () => {
  const w = (state: WebState, room?: Partial<ToolRowRoom>) =>
    words({ tool: "web_search", state }, room);
  expect(w("available")).toEqual({
    name: "Web search",
    lines: ["On. Clauseo can search the web and read pages."],
    progress: null,
    action: null,
  });
  expect(w("off")).toMatchObject({
    lines: ["Off. You turned it off in Settings."],
    action: "settings",
  });
  expect(w("off", { settings: false }).action).toBeNull();
  expect(w("locked_off")).toMatchObject({
    lines: ["Off. Managed by your organisation."],
    action: null,
  });
  expect(w("no_search_on_this_login").lines).toEqual([
    "This Claude login can't search the web. Clauseo can still read web pages.",
  ]);
  expect(w("no_search_on_this_login", { plan: "ChatGPT" }).lines[0]).toContain(
    "This ChatGPT login",
  );
});

test("each browser row's words", () => {
  const b = (state: BrowserState) => words({ tool: "browser", state });
  const expected: Record<BrowserState, string> = {
    available: "On. Clauseo can open web pages in its own browser.",
    off: "Off. You turned it off in Settings.",
    locked_off: "Off. Managed by your organisation.",
    managed_by_firm:
      "Off. Your organisation chooses the tools Claude has on this computer, so Clauseo adds none.",
    no_proxy: "Off until Clauseo can check each website the browser opens.",
    too_many_browsers:
      "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
    browser_not_installed: "Off. There's no Chrome on this computer.",
  };
  for (const state of BROWSER) {
    const row = b(state);
    expect(row.name).toBe("Browser");
    expect(row.lines).toEqual([expected[state]]);
    expect(row.action).toBe(state === "off" ? "settings" : null);
    expect(row.progress).toBeNull();
  }
});

const OFFER = { offer: "download", bytes: 201_000_000 } as const;
const missing = (install: BrowserInstall | null) =>
  words({ tool: "browser", state: "browser_not_installed" }, { install });
const NO_CHROME = "Off. There's no Chrome on this computer.";

test("no Chrome: the download's words for every install state", () => {
  expect(missing(null)).toMatchObject({ lines: [NO_CHROME], action: null, progress: null });
  expect(missing({ offer: null, progress: null })).toMatchObject({
    lines: [NO_CHROME],
    action: null,
  });
  expect(missing({ offer: OFFER, progress: null })).toMatchObject({
    lines: [
      NO_CHROME,
      "Clauseo can download Chrome for its own use: about 200 MB from Google, once for every conversation. It needs about 1 GB free.",
    ],
    action: "download",
  });
  expect(missing({ offer: { offer: "none" }, progress: null })).toMatchObject({
    lines: [NO_CHROME, "Install Google Chrome to use the browser."],
    action: null,
  });
  expect(
    missing({ offer: OFFER, progress: { downloading: { bytes: 64_400_000, total: 201_000_000 } } }),
  ).toEqual({
    name: "Browser",
    lines: [NO_CHROME],
    progress: { value: 64_400_000, max: 201_000_000, label: "Downloading Chrome. 64 of 201 MB." },
    action: null,
  });
  expect(missing({ offer: OFFER, progress: "unpacking" })).toMatchObject({
    lines: [NO_CHROME],
    progress: { value: 1, max: 1, label: "Unpacking Chrome." },
    action: null,
  });
  expect(missing({ offer: OFFER, progress: "done" })).toMatchObject({
    lines: [NO_CHROME, "Chrome is ready. New conversations get the browser."],
    progress: null,
    action: null,
  });
  const failed = {
    disk: "Chrome couldn't be downloaded. It needs about 1 GB free on this computer.",
    refused: "Your network is blocking the download.",
    check: "The download didn't match Google's, so Clauseo deleted it.",
    other: "Chrome couldn't be downloaded.",
  } as const;
  for (const code of ["disk", "refused", "check", "other"] as const)
    expect(missing({ offer: OFFER, progress: { failed: { code } } })).toMatchObject({
      lines: [NO_CHROME, failed[code]],
      progress: null,
      action: "retry",
    });
});

test("the download's words leave the other browser states alone", () => {
  const install: BrowserInstall = { offer: OFFER, progress: "done" };
  expect(words({ tool: "browser", state: "no_proxy" }, { install }).lines).toHaveLength(1);
});

test("in Hindi, the hi: later words show their English", () => {
  expect(toolRow("hi", { tool: "browser", state: "available" }, ROOM).name).toBe("Browser");
  // the managed sentence keeps one language while its own Hindi is still to come
  expect(toolRow("hi", { tool: "browser", state: "locked_off" }, ROOM).lines).toEqual([
    "Off. Managed by your organisation.",
  ]);
});
