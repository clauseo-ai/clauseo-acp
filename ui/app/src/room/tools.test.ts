// The tool line's pure reading: which tools a header's list speaks for, each glyph's state and the line's name for every
// pair of states, and each popover row's words for every ToolState and every Chrome download state.
import { expect, test } from "vitest";
import type {
  BrowserInstallFailure,
  BrowserInstallState,
  ToolState,
} from "../transport/protocol.ts";
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
    off: "In Settings",
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
      // In Settings sits in the chip's row
      beside: tool.state === "off",
      reasonTip: null,
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

const DL = (bytes: number, total: number): BrowserInstallState => ({
  downloading: { bytes, total },
});
const FAIL = (code: string): BrowserInstallState => ({
  failed: { code: code as BrowserInstallFailure, message: "curl: (56) Recv failure" },
});
const missing = (install: BrowserInstall | null) =>
  words({ tool: "browser", state: "browser_not_installed" }, { install });
const offer = { progress: null };

const NB = "\u00a0";

test("no Chrome: the offer until a state says otherwise, and a reason in every one", () => {
  expect(missing(null)).toMatchObject({ reason: "Needs Chrome", download: null });
  expect(missing(offer)).toMatchObject({
    reason: "Needs Chrome",
    download: {
      kind: "offer",
      button: "Download Chrome",
      size: `200${NB}MB`,
      tip: `From Google, once. Needs 1${NB}GB free.`,
    },
  });
  expect(
    missing({ progress: { downloading: { bytes: 64_400_000, total: 200_000_000 } } }),
  ).toMatchObject({
    reason: "Needs Chrome",
    download: {
      kind: "progress",
      value: 64_400_000,
      max: 200_000_000,
      text: `64 of 200${NB}MB`,
      name: "Download Chrome",
    },
  });
  // the first Downloading has not brought the total yet: an empty bar, no words
  expect(missing({ progress: { downloading: { bytes: 0, total: 0 } } }).download).toMatchObject({
    kind: "progress",
    value: 0,
    max: 1,
    text: "",
  });
  // no bar while unpacking: DESIGN's bars are determinate only
  expect(missing({ progress: "unpacking" }).download).toEqual({
    kind: "unpacking",
    text: "Unpacking",
  });
  // done keeps a reason: what Chrome is ready for, its tooltip saying when
  expect(missing({ progress: "done" })).toMatchObject({
    reason: "Ready for new conversations",
    reasonTip: "The browser comes with your next conversation.",
    beside: false,
    chip: "Off",
    download: { kind: "done" },
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
  expect(new Set(Object.keys(FAILED_KEYS))).toEqual(
    new Set(Object.keys(FAILED).filter((c) => c !== "unsupported" && c !== "cancelled")),
  );
  for (const code of Object.keys(FAILED) as InstallFailCode[]) {
    // a cancel is no failure: the store turns it into the offer (see the store tests), so no row gets it
    if (code === "cancelled") continue;
    const row = missing({
      progress: { failed: { code, message: "curl: (56) Recv failure" } },
    } as never);
    expect(row.reason).toBe("Needs Chrome");
    if (code === "unsupported") {
      // this computer can't download it: a muted line, no Try again
      expect(row.download).toEqual({ kind: "none", line: "Install Chrome to use it" });
    } else {
      expect(row.download).toEqual({ kind: "failed", cause: FAILED[code] });
    }
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
    missing(offer),
    missing({ progress: { downloading: { bytes: 1e6, total: 200_000_000 } } }),
    missing({ progress: { failed: { code: "disk_space", message: "" } } }),
  ].map((r) => JSON.stringify(r.download));
  const joined = all.join(" ");
  expect(joined).toMatch(/\d\u00a0(MB|GB)/);
  expect(joined).not.toMatch(/\d[ \t\n](MB|GB)/);
});

/** A store over FakeTransport with the app feed. `first` is the catch-up the host sends a new subscriber; `refuse`
 * makes every install command fail, as a host refuses one. */
function storeOver(options: { first?: BrowserInstallState[]; refuse?: boolean } = {}) {
  const transport = new FakeTransport({
    query: () => {
      throw new Error("no query");
    },
    next: {
      parts: ["app"],
      query: () => Promise.reject(new Error("not scripted")),
      first: () => (options.first ?? []).map((state) => ({ browser_install: { state } })),
    },
    dispatch: () => (options.refuse ? Promise.reject(new Error("the host refused it")) : "done"),
  });
  return { transport, store: new BrowserInstallStore(transport) };
}
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const installs = (transport: FakeTransport) =>
  transport
    .only("dispatch")
    .map((c) => c.command.command)
    .filter((c) => "install_browser" in c);

test("a new store hears the catch-up: the latest state, or the offer when the host has none", async () => {
  const none = storeOver();
  await settle();
  expect(none.store.get()).toEqual({ progress: null });
  const running = storeOver({ first: [DL(50e6, 201e6)] });
  await settle();
  expect(running.store.get()).toEqual({ progress: { downloading: { bytes: 50e6, total: 201e6 } } });
  // the last download's end, until the next one starts: a cancel is the offer again
  const cancelled = storeOver({ first: [DL(1e6, 201e6), FAIL("cancelled")] });
  await settle();
  expect(cancelled.store.get()).toEqual({ progress: null });
});

test("Install sends install_browser once through dispatch, and the bar waits for the host's total", async () => {
  const { transport, store } = storeOver();
  await settle();
  store.install();
  expect(installs(transport)).toEqual([{ install_browser: {} }]);
  expect(store.get().progress).toEqual({ downloading: { bytes: 0, total: 0 } });
  transport.appEvent({ browser_install: { state: DL(64e6, 200e6) } });
  expect(store.get().progress).toEqual({ downloading: { bytes: 64e6, total: 200e6 } });
});

test("a second Install while one runs sends nothing and shows no reset", async () => {
  const { transport, store } = storeOver({ first: [DL(50e6, 201e6)] });
  await settle();
  store.install();
  store.install();
  expect(installs(transport)).toEqual([]);
  expect(store.get().progress).toEqual({ downloading: { bytes: 50e6, total: 201e6 } });
  transport.appEvent({ browser_install: { state: "unpacking" } });
  store.install();
  expect(installs(transport)).toEqual([]);
  expect(store.get().progress).toBe("unpacking");
});

test("Try again after a failure sends Install again", async () => {
  const { transport, store } = storeOver({ first: [FAIL("network")] });
  await settle();
  expect(store.get().progress).toEqual({
    failed: { code: "network", message: "curl: (56) Recv failure" },
  });
  store.install();
  expect(installs(transport)).toHaveLength(1);
});

test("a refused Install is no failure: the row goes back to where it stood before the press", async () => {
  const { store } = storeOver({ refuse: true });
  await settle();
  store.install();
  expect(store.get().progress).toEqual({ downloading: { bytes: 0, total: 0 } });
  await settle();
  expect(store.get()).toEqual({ progress: null });
  // after a failure, a refused Try again goes back to that failure, not to a new one
  const failed = storeOver({ first: [FAIL("network")], refuse: true });
  await settle();
  failed.store.install();
  await settle();
  expect(failed.store.get().progress).toEqual({
    failed: { code: "network", message: "curl: (56) Recv failure" },
  });
});

test("a refused Install leaves alone what the host's events said since the press", async () => {
  const { transport, store } = storeOver({ refuse: true });
  await settle();
  store.install();
  transport.appEvent({ browser_install: { state: DL(50e6, 201e6) } });
  await settle();
  expect(store.get().progress).toEqual({ downloading: { bytes: 50e6, total: 201e6 } });
});

test("a cancelled download goes back to the offer with no failure", async () => {
  const { transport, store } = storeOver({ first: [DL(1e6, 201e6)] });
  await settle();
  transport.appEvent({ browser_install: { state: FAIL("cancelled") } });
  expect(store.get()).toEqual({ progress: null });
});

test("a failure's code is read and its message kept for the log", () => {
  const { transport, store } = storeOver();
  transport.appEvent({ browser_install: { state: FAIL("what_is_this") } });
  expect(store.get().progress).toEqual({
    failed: { code: "unknown", message: "curl: (56) Recv failure" },
  });
});

test("the download's words leave the other browser states alone", () => {
  const install: BrowserInstall = { progress: "done" };
  expect(words({ tool: "browser", state: "no_proxy" }, { install })).toMatchObject({
    reason: "Not available yet",
    download: null,
  });
});

test("in Hindi, the tools' words say their Hindi, and a reason keeps its brand name's capital", () => {
  expect(toolRow("hi", { tool: "browser", state: "available" }, ROOM).name).toBe("ब्राउज़र");
  expect(toolRow("hi", { tool: "browser", state: "locked_off" }, ROOM).reason).toBe(
    "फ़र्म द्वारा प्रबंधित",
  );
  expect(toolsLabel("hi", [{ tool: "browser", state: "browser_not_installed" }])).toBe(
    "ब्राउज़र: बंद, Chrome चाहिए।",
  );
});
