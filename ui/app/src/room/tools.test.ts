// The tool line's pure reading: which tools a header's list speaks for, the status line's phrase for every pair of
// states, and each popover row's words for every ToolState and every Chrome download state.
import { expect, test } from "vitest";
import type { ToolState } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import {
  BrowserInstallStore,
  failCodeOf,
  type BrowserInstall,
  type InstallFailCode,
} from "./browser-install.ts";
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
    "Off. This Claude login can't search the web. Clauseo can still read web pages.",
  ]);
  expect(w("no_search_on_this_login", { plan: "ChatGPT" }).lines[0]).toContain(
    "Off. This ChatGPT login",
  );
});

test("each browser row's words", () => {
  const b = (state: BrowserState) => words({ tool: "browser", state });
  const expected: Record<BrowserState, string> = {
    available: "On. Clauseo can open web pages in its own browser.",
    off: "Off. You turned it off in Settings.",
    locked_off: "Off. Managed by your organisation.",
    managed_by_firm:
      "Off. Your organisation chooses the tools Claude has on this computer, so Clauseo can't add its browser.",
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

const NB = "\u00a0";

test("no Chrome: the download's words for every install state", () => {
  expect(missing(null)).toMatchObject({ lines: [NO_CHROME], action: null, progress: null });
  expect(missing({ offer: null, progress: null })).toMatchObject({
    lines: [NO_CHROME],
    action: null,
  });
  expect(missing({ offer: OFFER, progress: null })).toMatchObject({
    lines: [
      NO_CHROME,
      `Clauseo can download its own Chrome from Google, about 200${NB}MB, once for all conversations. It needs about 1${NB}GB free.`,
    ],
    action: "download",
  });
  expect(missing({ offer: { offer: "none" }, progress: null })).toMatchObject({
    lines: [NO_CHROME, "Install Google Chrome to use the browser."],
    action: null,
  });
});

test("while Chrome downloads or unpacks, the first line says what it waits for; once done, only that it's ready", () => {
  expect(
    missing({ offer: OFFER, progress: { downloading: { bytes: 64_400_000, total: 201_000_000 } } }),
  ).toEqual({
    name: "Browser",
    lines: ["Off until Chrome is downloaded."],
    progress: {
      value: 64_400_000,
      max: 201_000_000,
      label: `Downloading Chrome. 64 of 201${NB}MB.`,
    },
    action: null,
  });
  expect(missing({ offer: OFFER, progress: "unpacking" })).toMatchObject({
    lines: ["Off until Chrome is downloaded."],
    progress: { value: 1, max: 1, label: "Unpacking Chrome." },
    action: null,
  });
  expect(missing({ offer: OFFER, progress: "done" })).toEqual({
    name: "Browser",
    lines: ["Chrome is ready. New conversations get the browser."],
    progress: null,
    action: null,
  });
});

const FAILED: Record<InstallFailCode, string> = {
  network: "The connection dropped before Chrome finished downloading.",
  size_mismatch: "The download didn't match the file Google published, so Clauseo deleted it.",
  signature: "The download didn't match the file Google published, so Clauseo deleted it.",
  disk_space: `Chrome couldn't be downloaded. It needs about 1${NB}GB free on this computer.`,
  host_refused: "Your network is blocking the download.",
  unpack: "Chrome downloaded but couldn't be unpacked.",
  unsupported: "Clauseo can't download Chrome itself. Install Google Chrome to use the browser.",
  cancelled: "The download stopped before it finished.",
  unknown: "Chrome couldn't be downloaded.",
};

test("a failed download's words by its code, Try again for all but unsupported, never the host's message", () => {
  for (const code of Object.keys(FAILED) as InstallFailCode[]) {
    const row = missing({
      offer: OFFER,
      progress: { failed: { code, message: "curl: (56) Recv failure" } },
    });
    expect(row).toEqual({
      name: "Browser",
      lines: [NO_CHROME, FAILED[code]],
      progress: null,
      action: code === "unsupported" ? null : "retry",
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
  ].flatMap((r) => [...r.lines, r.progress?.label ?? ""]);
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
  expect(words({ tool: "browser", state: "no_proxy" }, { install }).lines).toHaveLength(1);
});

test("in Hindi, the hi: later words show their English", () => {
  expect(toolRow("hi", { tool: "browser", state: "available" }, ROOM).name).toBe("Browser");
  // the managed sentence keeps one language while its own Hindi is still to come
  expect(toolRow("hi", { tool: "browser", state: "locked_off" }, ROOM).lines).toEqual([
    "Off. Managed by your organisation.",
  ]);
});
