// The tool line's pure reading: which tools a header's list speaks for, each glyph's state and the line's name for every
// pair of states, and each popover row's words for every ToolState and every Chrome download state.
import { expect, test } from "vitest";
import type { ToolState } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import {
  BrowserInstallStore,
  failCodeOf,
  type BrowserInstall,
  type InstallFailCode,
} from "./browser-install.ts";
import { FAILED as FAILED_KEYS, toolRow, type ToolRowRoom } from "./tool-rows.ts";
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

const label = (web: WebState | null, browser: BrowserState | null) => {
  const tools: KnownTool[] = [];
  if (web !== null) tools.push({ tool: "web_search", state: web });
  if (browser !== null) tools.push({ tool: "browser", state: browser });
  return toolsLabel("en", tools);
};

test("each glyph's state: on as drawn, the firm's lock for locked off and managed by firm, every other off slashed", () => {
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
  expect(label("off", null)).toBe("Web search: off in Settings.");
  expect(label(null, "off")).toBe("Browser: off in Settings.");
  expect(label("locked_off", "managed_by_firm")).toBe(
    "Web search: off, managed by firm. Browser: off, managed by firm.",
  );
  expect(label("no_search_on_this_login", "no_proxy")).toBe(
    "Web search: off, can't search on this login. Browser: off, not available yet.",
  );
  expect(label(null, "too_many_browsers")).toBe("Browser: off, too many browsers open.");
  expect(label(null, "locked_off")).toBe("Browser: off, managed by firm.");
});

test("sameTools reads two lists the same only when every tool and state match", () => {
  const a: KnownTool[] = [{ tool: "browser", state: "off" }];
  expect(sameTools(a, [{ tool: "browser", state: "off" }])).toBe(true);
  expect(sameTools(a, [{ tool: "browser", state: "no_proxy" }])).toBe(false);
  expect(sameTools(a, [])).toBe(false);
});

const ROOM: ToolRowRoom = { settings: true, install: null };
const words = (tool: KnownTool, room: Partial<ToolRowRoom> = {}) =>
  toolRow("en", tool, { ...ROOM, ...room });

test("each row: its glyph, name, an On or Off chip, and the reason in a few words only when off", () => {
  const reasons: Record<string, string | null> = {
    available: null,
    off: "Off in Settings",
    locked_off: "Managed by firm",
    managed_by_firm: "Managed by firm",
    no_search_on_this_login: "Can't search on this login",
    no_proxy: "Not available yet",
    too_many_browsers: "Too many browsers open",
    browser_not_installed: "Needs Chrome",
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

const OFFER = { offer: "download", bytes: 201_000_000 } as const;
const missing = (install: BrowserInstall | null) =>
  words({ tool: "browser", state: "browser_not_installed" }, { install });

const NB = "\u00a0";

test("no Chrome: the download for every install state, the reason staying until Chrome is ready", () => {
  expect(missing(null)).toMatchObject({ reason: "Needs Chrome", download: null });
  expect(missing({ offer: null, progress: null }).download).toBeNull();
  expect(missing({ offer: OFFER, progress: null })).toMatchObject({
    reason: "Needs Chrome",
    download: {
      kind: "offer",
      button: "Download Chrome",
      size: `200${NB}MB`,
      tip: `About 200${NB}MB from Google, once. Needs about 1${NB}GB free.`,
    },
  });
  expect(missing({ offer: { offer: "none" }, progress: null })).toMatchObject({
    reason: "Needs Chrome",
    download: { kind: "none", line: "Install Chrome to use it" },
  });
  expect(
    missing({ offer: OFFER, progress: { downloading: { bytes: 64_400_000, total: 201_000_000 } } }),
  ).toMatchObject({
    reason: "Needs Chrome",
    download: {
      kind: "progress",
      value: 64_400_000,
      max: 201_000_000,
      text: `64 of 201${NB}MB`,
      name: "Download Chrome",
    },
  });
  expect(missing({ offer: OFFER, progress: "unpacking" }).download).toEqual({
    kind: "progress",
    value: 1,
    max: 1,
    text: "Unpacking",
    name: "Download Chrome",
  });
  expect(missing({ offer: OFFER, progress: "done" })).toMatchObject({
    reason: null,
    chip: "Off",
    download: { kind: "done", line: "Chrome ready" },
  });
});

const FAILED: Record<InstallFailCode, string> = {
  network: "Connection dropped",
  size_mismatch: "File didn't match",
  signature: "File didn't match",
  disk_space: `Needs 1${NB}GB free`,
  host_refused: "Network blocked it",
  unpack: "Couldn't unpack",
  unsupported: "Install Chrome to use it",
  cancelled: "Download stopped",
  unknown: "Download failed",
};

test("a failed download's cause by its code, Try again for all but unsupported, never the host's message", () => {
  expect(new Set(Object.keys(FAILED_KEYS))).toEqual(new Set(Object.keys(FAILED)));
  for (const code of Object.keys(FAILED) as InstallFailCode[]) {
    const row = missing({
      offer: OFFER,
      progress: { failed: { code, message: "curl: (56) Recv failure" } },
    });
    expect(row.reason).toBe("Needs Chrome");
    expect(row.download).toEqual({
      kind: "failed",
      cause: FAILED[code],
      retry: code !== "unsupported",
    });
    expect(JSON.stringify(row)).not.toContain("curl");
  }
});

test("the host's failure code is an open enum: a value this version doesn't know is unknown", () => {
  for (const code of Object.keys(FAILED)) expect(failCodeOf(code)).toBe(code);
  expect(failCodeOf("disk")).toBe("unknown");
  expect(failCodeOf("a_newer_code")).toBe("unknown");
  expect(failCodeOf(undefined)).toBe("unknown");
  expect(failCodeOf(7)).toBe("unknown");
});

test("every number before MB or GB in the download's words is held to its unit by a non-breaking space", () => {
  const all = [
    missing({ offer: OFFER, progress: null }),
    missing({ offer: OFFER, progress: { downloading: { bytes: 1e6, total: 201_000_000 } } }),
    missing({ offer: OFFER, progress: { failed: { code: "disk_space", message: "" } } }),
  ].map((r) => JSON.stringify(r.download));
  const joined = all.join(" ");
  expect(joined).toMatch(/\d\u00a0(MB|GB)/);
  expect(joined).not.toMatch(/\d[ \t\n](MB|GB)/);
});

/** A store over FakeTransport whose installBrowser is refused as a command, as the host refuses a second one. */
function refusing() {
  const transport = new FakeTransport({
    query: () => {
      throw new Error("no query");
    },
    next: { parts: ["app"], query: () => Promise.reject(new Error("not scripted")) },
  });
  let refuse: (error: Error) => void = () => {};
  transport.installBrowser = () =>
    new Promise<void>((_, reject) => {
      refuse = reject;
    });
  return { transport, store: new BrowserInstallStore(transport), refuse: (e: Error) => refuse(e) };
}

test("a refused InstallBrowser is no failure: the row goes back to where it stood before the press", async () => {
  const { transport, store, refuse } = refusing();
  transport.appEvent({ browser_install: OFFER });
  store.install();
  expect(store.get()?.progress).toEqual({ downloading: { bytes: 0, total: 201_000_000 } });
  refuse(new Error("already running"));
  await Promise.resolve();
  await Promise.resolve();
  expect(store.get()).toEqual({ offer: OFFER, progress: null });
  // after a failure, a refused Try again goes back to that failure, not to a new one
  transport.appEvent({ browser_install: { failed: { code: "network", message: "reset" } } });
  store.install();
  refuse(new Error("already running"));
  await Promise.resolve();
  await Promise.resolve();
  expect(store.get()?.progress).toEqual({ failed: { code: "network", message: "reset" } });
});

test("a refused InstallBrowser leaves alone what the host's events said since the press", async () => {
  const { transport, store, refuse } = refusing();
  transport.appEvent({ browser_install: OFFER });
  store.install();
  transport.appEvent({ browser_install: { downloading: { bytes: 50e6, total: 201e6 } } });
  refuse(new Error("already running"));
  await Promise.resolve();
  await Promise.resolve();
  expect(store.get()?.progress).toEqual({ downloading: { bytes: 50e6, total: 201e6 } });
});

test("a failure's code is read and its message kept for the log", () => {
  const { transport, store } = refusing();
  transport.appEvent({ browser_install: { failed: { code: "what_is_this", message: "m" } } });
  expect(store.get()?.progress).toEqual({ failed: { code: "unknown", message: "m" } });
});

test("the download's words leave the other browser states alone", () => {
  const install: BrowserInstall = { offer: OFFER, progress: "done" };
  expect(words({ tool: "browser", state: "no_proxy" }, { install })).toMatchObject({
    reason: "Not available yet",
    download: null,
  });
});

test("in Hindi, the hi: later words show their English", () => {
  expect(toolRow("hi", { tool: "browser", state: "available" }, ROOM).name).toBe("Browser");
  expect(toolRow("hi", { tool: "browser", state: "locked_off" }, ROOM).reason).toBe(
    "Managed by firm",
  );
  expect(toolsLabel("hi", [{ tool: "browser", state: "browser_not_installed" }])).toBe(
    "Browser: off, needs Chrome.",
  );
});
