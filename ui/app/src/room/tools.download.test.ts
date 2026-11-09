// The tool line's Chrome words in the popover: the download's offer, progress and failure causes, the store that starts
// the install, and each browser state that offers no download, the list of Chrome versions among them.
import { expect, test } from "vitest";
import type { BrowserInstallFailure, BrowserInstallState } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import {
  BrowserInstallStore,
  failCodeOf,
  type BrowserInstall,
  type InstallFailCode,
} from "./browser-install.ts";
import { FAILED as FAILED_KEYS, toolRow } from "./tool-rows.ts";
import type { KnownTool } from "./tools.ts";
import { LIST_EXPIRED, NO_LIST, POLICY, ROOM, UNREADABLE, words } from "./tools-support.ts";

const LIST_OLD = "Clauseo hasn't checked for a newer Chrome in a while. The browser still works.";

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
  hash_mismatch: "File didn't match",
  list_unavailable: "Can't download Chrome yet",
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

test("the firm's browser policy offers no download in any download state: it blocks every browser", () => {
  const blocked: KnownTool = { tool: "browser", state: "browser_blocked_by_policy" };
  for (const install of [null, offer, { progress: "done" }] as (BrowserInstall | null)[])
    expect(words(blocked, { install })).toMatchObject({
      glyph: "locked",
      reason: "Blocked by your firm",
      reasonTip: null,
      note: POLICY,
      settings: false,
      download: null,
    });
});

test("a browser policy Clauseo couldn't check offers no download either, and says it checks again", () => {
  const unread: KnownTool = { tool: "browser", state: "browser_policy_unreadable" };
  for (const install of [null, offer, { progress: "done" }] as (BrowserInstall | null)[])
    expect(words(unread, { install })).toMatchObject({
      glyph: "off",
      reason: "Couldn't check browser settings",
      reasonTip: null,
      note: UNREADABLE,
      settings: false,
      download: null,
    });
  expect(toolRow("hi", unread, ROOM)).toMatchObject({
    reason: "ब्राउज़र सेटिंग्स की जाँच नहीं हो सकी",
    note:
      "Clauseo इस कंप्यूटर की ब्राउज़र सेटिंग्स की जाँच नहीं कर सका, इसलिए वह अभी ब्राउज़र का उपयोग नहीं करेगा। " +
      "अगली बार ब्राउज़र की ज़रूरत होने पर वह फिर से जाँच करेगा।",
  });
});

test("a Chrome too old to use offers the download, as no Chrome does, in every download state", () => {
  const old: KnownTool = { tool: "browser", state: "browser_out_of_date" };
  expect(words(old, { install: offer })).toMatchObject({
    glyph: "off",
    reason: "Needs a newer Chrome",
    note: null,
    settings: false,
    download: {
      kind: "offer",
      button: "Download Chrome",
      size: `200${NB}MB`,
      tip: `From Google, once. Needs 1${NB}GB free.`,
    },
  });
  for (const install of [
    offer,
    { progress: { downloading: { bytes: 1e6, total: 200e6 } } },
    { progress: "unpacking" },
    { progress: "done" },
    { progress: { failed: { code: "hash_mismatch", message: "" } } },
    { progress: { failed: { code: "list_unavailable", message: "" } } },
  ] as BrowserInstall[])
    expect(words(old, { install }).download).toEqual(missing(install).download);
  // once the new Chrome is in, what it is ready for
  expect(words(old, { install: { progress: "done" } })).toMatchObject({
    reason: "Ready for new conversations",
    reasonTip: "The browser comes with your next conversation.",
  });
  expect(toolRow("hi", old, ROOM).reason).toBe("नया Chrome चाहिए");
});

test("with no list of Chrome versions, no download in any download state, and the reason in full", () => {
  const noList: KnownTool = { tool: "browser", state: "browser_list_unavailable" };
  for (const install of [null, offer, { progress: "done" }] as (BrowserInstall | null)[])
    expect(words(noList, { install })).toMatchObject({
      glyph: "off",
      reason: "Couldn't check Chrome's version",
      reasonTip: null,
      note: NO_LIST,
      settings: false,
      download: null,
    });
  expect(toolRow("hi", noList, ROOM)).toMatchObject({
    reason: "Chrome के संस्करण की जाँच नहीं हो सकी",
    note:
      "Clauseo यह पक्का नहीं कर सका कि उसका Chrome अप-टू-डेट है, इसलिए वह अभी ब्राउज़र का उपयोग नहीं कर रहा। " +
      "वह अपने आप जाँच करता रहेगा।",
  });
});

test("with a list of Chrome versions too old, no download in any download state, and the reason in full", () => {
  const expired: KnownTool = { tool: "browser", state: "browser_list_expired" };
  for (const install of [null, offer, { progress: "done" }] as (BrowserInstall | null)[])
    expect(words(expired, { install })).toMatchObject({
      glyph: "off",
      reason: "Can't update Chrome right now",
      reasonTip: null,
      note: LIST_EXPIRED,
      settings: false,
      download: null,
    });
  expect(toolRow("hi", expired, ROOM)).toMatchObject({
    reason: "Chrome अभी अपडेट नहीं हो सकता",
    note:
      "Clauseo का Chrome इतना पुराना है कि उसका उपयोग नहीं हो सकता, और Clauseo अभी उसे अपडेट नहीं कर सकता। " +
      "वह अपने आप कोशिश करता रहेगा।",
  });
});

test("a browser that is on with an old list of Chrome versions stays on, its notice a muted line", () => {
  const noticed: KnownTool = { tool: "browser", state: "available", notice: "browser_list_old" };
  expect(words(noticed, { install: offer })).toEqual({
    tool: "browser",
    glyph: "on",
    name: "Browser",
    on: true,
    chip: "On",
    reason: null,
    beside: false,
    reasonTip: null,
    note: LIST_OLD,
    settings: false,
    download: null,
  });
  expect(toolRow("hi", noticed, ROOM).note).toBe(
    "Clauseo ने कुछ समय से नए Chrome की जाँच नहीं की है। ब्राउज़र अब भी काम करता है।",
  );
});
