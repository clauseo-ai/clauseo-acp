// The tool line's pure reading: which tools a header's list speaks for, each glyph's state and the line's name for every
// pair of states, and each popover row's words for every ToolState, on a Mac and on Windows.
import { expect, test } from "vitest";
import type { ToolState } from "../transport/protocol.ts";
import { toolRow, type ToolRowRoom } from "./tool-rows.ts";
import {
  glyphState,
  knownTools,
  sameTools,
  toolsLabel,
  type BrowserState,
  type KnownTool,
  type WebState,
} from "./tools.ts";

const WEB: WebState[] = ["available", "off", "locked_off", "no_search_on_this_login"];
const BROWSER: BrowserState[] = [
  "available",
  "off",
  "locked_off",
  "browser_not_installed",
  "browser_blocked_by_policy",
  "browser_policy_unreadable",
];
const avail = (state: string): ToolState["availability"] =>
  state === "available" ? "available" : ({ unavailable: { reason: state } } as never);

test("knownTools keeps the tools and states it has words for, web search first, each once", () => {
  expect(knownTools(undefined)).toEqual([]);
  expect(knownTools([])).toEqual([]);
  expect(
    knownTools([
      { tool: "browser", availability: avail("browser_not_installed") },
      { tool: "web_search", availability: "available" },
      { tool: "web_search", availability: avail("off") },
    ]),
  ).toEqual([
    { tool: "web_search", state: "available" },
    { tool: "browser", state: "browser_not_installed" },
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
  // the guarded browser's reasons never come for #870's shell browser, so they have no words
  for (const gone of ["managed_by_firm", "no_proxy", "too_many_browsers"])
    expect(knownTools([{ tool: "browser", availability: avail(gone) }])).toEqual([]);
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
  return toolsLabel("en", "mac", tools);
};

test("each glyph's state: on as drawn, the firm's lock for locked off and its policy, every other off slashed", () => {
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
    browser_not_installed: "off",
    browser_blocked_by_policy: "locked",
    // not read is no firm's lock
    browser_policy_unreadable: "off",
  };
  for (const w of WEB) expect(glyphState({ tool: "web_search", state: w })).toBe(web[w]);
  for (const b of BROWSER) expect(glyphState({ tool: "browser", state: b })).toBe(browser[b]);
});

test("the line's name and tooltip: one sentence per tool, web search first, its reason when off", () => {
  expect(label(null, null)).toBeNull();
  expect(label("available", "browser_not_installed")).toBe(
    "Web search: on. Browser: off, install Chrome.",
  );
  expect(
    toolsLabel("en", "windows", [
      { tool: "web_search", state: "available" },
      { tool: "browser", state: "browser_not_installed" },
    ]),
  ).toBe("Web search: on. Browser: off, install Chrome or Edge.");
  expect(label("available", "available")).toBe("Web search: on. Browser: on.");
  expect(label("off", null)).toBe("Web search: off, in Settings.");
  expect(label(null, "off")).toBe("Browser: off, in Settings.");
  expect(label("locked_off", "locked_off")).toBe(
    "Web search: off, managed by firm. Browser: off, managed by firm.",
  );
  expect(label("no_search_on_this_login", null)).toBe(
    "Web search: off, can't search on this login.",
  );
  expect(label(null, "locked_off")).toBe("Browser: off, managed by firm.");
  expect(label("available", "browser_blocked_by_policy")).toBe(
    "Web search: on. Browser: off, blocked by your firm.",
  );
  expect(label(null, "browser_policy_unreadable")).toBe(
    "Browser: off, couldn't check browser settings.",
  );
});

test("sameTools reads two lists the same only when every tool and state match", () => {
  const a: KnownTool[] = [{ tool: "browser", state: "off" }];
  expect(sameTools(a, [{ tool: "browser", state: "off" }])).toBe(true);
  expect(sameTools(a, [{ tool: "browser", state: "locked_off" }])).toBe(false);
  expect(sameTools(a, [])).toBe(false);
});

const POLICY =
  "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.";
const UNREADABLE =
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. " +
  "It checks again the next time it needs one.";
const INSTALL_MAC = "Install Google Chrome, then start a new conversation.";
const INSTALL_WINDOWS = "Install Google Chrome or Microsoft Edge, then start a new conversation.";

const ROOM: ToolRowRoom = { settings: true, platform: "mac" };
const words = (tool: KnownTool, room: Partial<ToolRowRoom> = {}) =>
  toolRow("en", tool, { ...ROOM, ...room });

test("each row: its glyph, name, an On or Off chip, and the reason in a few words only when off", () => {
  const reasons: Record<string, string | null> = {
    available: null,
    off: "In Settings",
    locked_off: "Managed by firm",
    no_search_on_this_login: "Can't search on this login",
    browser_not_installed: "Install Chrome",
    browser_blocked_by_policy: "Blocked by your firm",
    browser_policy_unreadable: "Couldn't check browser settings",
  };
  const notes: Record<string, string> = {
    browser_not_installed: INSTALL_MAC,
    browser_blocked_by_policy: POLICY,
    browser_policy_unreadable: UNREADABLE,
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
      // what to install, the firm's policy, or a policy not read, says it in full
      note: notes[tool.state] ?? null,
      settings: tool.state === "off",
      // without the app feed, no Try again
      recheck: null,
    });
  }
});

test("Open Settings is offered only for Off in Settings, with the settings window", () => {
  expect(words({ tool: "web_search", state: "off" }).settings).toBe(true);
  expect(words({ tool: "web_search", state: "off" }, { settings: false }).settings).toBe(false);
  expect(words({ tool: "browser", state: "locked_off" }).settings).toBe(false);
  expect(words({ tool: "browser", state: "browser_not_installed" }).settings).toBe(false);
});

test("no browser on the computer: what to install, by the OS, and that a new conversation then finds it", () => {
  const missing: KnownTool = { tool: "browser", state: "browser_not_installed" };
  expect(words(missing)).toMatchObject({
    glyph: "off",
    reason: "Install Chrome",
    reasonTip: null,
    note: INSTALL_MAC,
    settings: false,
    recheck: null,
  });
  expect(words(missing, { platform: "windows" })).toMatchObject({
    reason: "Install Chrome or Edge",
    note: INSTALL_WINDOWS,
  });
  // Linux is for development only: Chrome, as on a Mac
  expect(words(missing, { platform: "linux" })).toMatchObject({
    reason: "Install Chrome",
    note: INSTALL_MAC,
  });
  // a policy read has nothing to say to it
  expect(words(missing, { policy: { state: "clear" } }).reason).toBe("Install Chrome");
});

test("the firm's browser policy blocks every browser, on either OS", () => {
  const blocked: KnownTool = { tool: "browser", state: "browser_blocked_by_policy" };
  for (const platform of ["mac", "windows"] as const)
    expect(words(blocked, { platform })).toMatchObject({
      glyph: "locked",
      reason: "Blocked by your firm",
      reasonTip: null,
      note: POLICY,
      settings: false,
    });
});

test("a browser policy Clauseo couldn't check says it checks again", () => {
  const unread: KnownTool = { tool: "browser", state: "browser_policy_unreadable" };
  for (const platform of ["mac", "windows"] as const)
    expect(words(unread, { platform })).toMatchObject({
      glyph: "off",
      reason: "Couldn't check browser settings",
      reasonTip: null,
      note: UNREADABLE,
      settings: false,
    });
  expect(toolRow("hi", unread, ROOM)).toMatchObject({
    reason: "ब्राउज़र सेटिंग्स की जाँच नहीं हो सकी",
    note:
      "Clauseo इस कंप्यूटर की ब्राउज़र सेटिंग्स की जाँच नहीं कर सका, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। " +
      "अगली बार ब्राउज़र की ज़रूरत होने पर वह फिर से जाँच करेगा।",
  });
});

test("in Hindi, the tools' words say their Hindi, and a reason keeps its brand name's capital", () => {
  expect(toolRow("hi", { tool: "browser", state: "available" }, ROOM).name).toBe("ब्राउज़र");
  expect(toolRow("hi", { tool: "browser", state: "locked_off" }, ROOM).reason).toBe(
    "फ़र्म द्वारा प्रबंधित",
  );
  expect(toolsLabel("hi", "mac", [{ tool: "browser", state: "browser_not_installed" }])).toBe(
    "ब्राउज़र: बंद, Chrome इंस्टॉल करें।",
  );
  expect(toolsLabel("hi", "windows", [{ tool: "browser", state: "browser_not_installed" }])).toBe(
    "ब्राउज़र: बंद, Chrome या Edge इंस्टॉल करें।",
  );
  expect(toolRow("hi", { tool: "browser", state: "browser_not_installed" }, ROOM).note).toBe(
    "Google Chrome इंस्टॉल करें, फिर एक नई बातचीत शुरू करें।",
  );
  // provisional Hindi, until the app thread writes it
  expect(toolsLabel("hi", "mac", [{ tool: "browser", state: "browser_blocked_by_policy" }])).toBe(
    "ब्राउज़र: बंद, आपकी फ़र्म ने रोका है।",
  );
});

test("Try again sits only under the browser whose policy Clauseo couldn't check, waiting while the read runs", () => {
  const unread: KnownTool = { tool: "browser", state: "browser_policy_unreadable" };
  expect(words(unread, { policy: { state: null } }).recheck).toEqual({
    button: "Try again",
    waiting: false,
  });
  expect(words(unread, { policy: { state: "unreadable" } }).recheck?.waiting).toBe(false);
  expect(words(unread, { policy: { state: "checking" } }).recheck).toEqual({
    button: "Try again",
    waiting: true,
  });
  expect(toolRow("hi", unread, { ...ROOM, policy: { state: null } }).recheck?.button).toBe(
    "फिर से प्रयास करें",
  );
  // no app feed to hear the read on: nothing to press
  expect(words(unread).recheck).toBeNull();
  // the firm's block, and every other state, gets none
  for (const state of BROWSER.filter((s) => s !== "browser_policy_unreadable"))
    expect(words({ tool: "browser", state }, { policy: { state: null } }).recheck).toBeNull();
});

test("a read that found the policy clear says the browser is ready for new conversations", () => {
  const unread: KnownTool = { tool: "browser", state: "browser_policy_unreadable" };
  expect(words(unread, { policy: { state: "clear" } })).toMatchObject({
    reason: "Ready for new conversations",
    reasonTip: "The browser comes with your next conversation.",
    note: null,
    recheck: null,
  });
});
