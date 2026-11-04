// @vitest-environment happy-dom
// The room's display settings: read from and written to localStorage["clauseo.display"] in H7's `ui` shape, the
// computer's light and reduced motion followed live until set, lite's batch, and with a NextHost the host's settings.
import type { Display } from "@clauseo/components/lib/display.ts";
import { expect, test, vi } from "vitest";
import { frameBatch, liteBatch } from "../store/batch.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import {
  createDisplayStore,
  DEFAULT_UI,
  DISPLAY_KEY,
  displayOf,
  type DisplayEnv,
} from "./display-store.ts";
import type { HostEvent, SettingsView } from "../transport/protocol.ts";
import { nextHostOf, type Settings, type SettingsHost, type UiSettings } from "./host-next.ts";
import { wireSettingsHost } from "./host-wire.ts";
import { connectRoom } from "./start.ts";

/** A media query the test flips by hand. */
class FakeQuery extends EventTarget {
  constructor(
    readonly media: string,
    public matches: boolean,
  ) {
    super();
  }
  flip(matches: boolean) {
    this.matches = matches;
    this.dispatchEvent(new Event("change"));
  }
}

function harness(over: Partial<DisplayEnv> = {}, stored: string | null = null) {
  const items = new Map<string, string>();
  if (stored !== null) items.set(DISPLAY_KEY, stored);
  const queries = {
    dark: new FakeQuery("(prefers-color-scheme: dark)", false),
    reduced: new FakeQuery("(prefers-reduced-motion: reduce)", false),
  };
  const started: Display[] = [];
  const shown: Display[] = [];
  const store = createDisplayStore({
    storage: {
      getItem: (key) => items.get(key) ?? null,
      setItem: (key, value) => void items.set(key, value),
    },
    matchMedia: (query) =>
      (query === queries.dark.media ? queries.dark : queries.reduced) as unknown as MediaQueryList,
    host: null,
    start: (d) => started.push(d),
    show: (d) => shown.push(d),
    ...over,
  });
  const saved = () => {
    const text = items.get(DISPLAY_KEY);
    return text === undefined ? undefined : (JSON.parse(text) as unknown);
  };
  return { store, queries, started, shown, saved };
}

const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

test("with nothing stored: the computer's light and motion, full render, English, text size 16", () => {
  const { started, store } = harness();
  expect(store.ui()).toEqual(DEFAULT_UI);
  expect(started).toHaveLength(1);
  expect(started[0]).toMatchObject({
    theme: "day",
    render: "full",
    motion: "full",
    lang: "en",
    textSize: 16,
  });
});

test("reads clauseo.display, keeps its valid keys and drops the rest", () => {
  const { started, store } = harness(
    {},
    JSON.stringify({ theme: "dark", language: "hi", font_size: 18, lite: "sideways", motion: 3 }),
  );
  expect(store.ui()).toEqual({ ...DEFAULT_UI, theme: "dark", language: "hi", font_size: 18 });
  expect(started[0]).toMatchObject({ theme: "night", lang: "hi", textSize: 18, render: "full" });
});

test("an unreadable clauseo.display starts from the defaults", () => {
  const { store } = harness({}, "{not json");
  expect(store.ui()).toEqual(DEFAULT_UI);
});

test("set writes the whole ui table to clauseo.display and shows it", () => {
  const { store, shown, saved } = harness();
  store.set({ theme: "dark", font_size: 20 });
  expect(saved()).toEqual({ ...DEFAULT_UI, theme: "dark", font_size: 20 });
  expect(shown.at(-1)).toMatchObject({ theme: "night", textSize: 20 });
});

test("the theme follows prefers-color-scheme, live, until it is set", () => {
  const { store, queries, shown } = harness();
  queries.dark.flip(true);
  expect(store.display().theme).toBe("night");
  queries.dark.flip(false);
  expect(store.display().theme).toBe("day");
  store.set({ theme: "light" });
  const before = shown.length;
  queries.dark.flip(true);
  expect(store.display().theme).toBe("day");
  expect(shown).toHaveLength(before);
  store.set({ theme: "system" });
  expect(store.display().theme).toBe("night");
});

test("motion follows prefers-reduced-motion, live, until it is set", () => {
  const { store, queries } = harness();
  queries.reduced.flip(true);
  expect(store.display().motion).toBe("reduced");
  store.set({ motion: "full" });
  expect(store.display().motion).toBe("full");
  queries.reduced.flip(false);
  queries.reduced.flip(true);
  expect(store.display().motion).toBe("full");
});

test("lite picks the view store's liteBatch; full and automatic keep frameBatch", () => {
  const { store } = harness();
  expect(store.batch()).toBe(frameBatch);
  store.set({ lite: "lite" });
  expect(store.display().render).toBe("lite");
  expect(store.batch()).toBe(liteBatch);
  store.set({ lite: "automatic" });
  expect(store.display().render).toBe("full");
  expect(store.batch()).toBe(frameBatch);
});

test("the platform is the host's, never a setting", () => {
  const { store, saved } = harness();
  store.setPlatform("windows");
  expect(store.display().platform).toBe("windows");
  expect(saved()).toBeUndefined();
});

test("the first display is laid out for the platform the store starts with; the host's same answer moves nothing", () => {
  const { store, started, shown } = harness({ platform: "windows" });
  expect(started.map((d) => d.platform)).toEqual(["windows"]);
  store.setPlatform("windows");
  expect(shown).toEqual([]);
  // a wrong guess is corrected by the host's answer
  store.setPlatform("mac");
  expect(shown.map((d) => d.platform)).toEqual(["mac"]);
});

test("connectRoom starts the display on the webview's own OS, and display_info confirms it", async () => {
  vi.stubGlobal("navigator", { platform: "Win32", userAgent: "Windows NT 10.0" });
  try {
    const store = connectRoom(new FakeTransport({ platform: "windows" }));
    expect(store.display().platform).toBe("windows");
    expect(document.documentElement.dataset["platform"]).toBe("windows");
    await settled();
    expect(store.display().platform).toBe("windows");
  } finally {
    vi.unstubAllGlobals();
  }
});

test("an override shows for this page only, is never written, and a later set of the same key takes over", () => {
  const { store, saved } = harness({ override: { theme: "dark", language: "hi" } });
  expect(store.display()).toMatchObject({ theme: "night", lang: "hi" });
  store.set({ theme: "light" });
  expect(store.display()).toMatchObject({ theme: "day", lang: "hi" });
  expect(saved()).toEqual({ ...DEFAULT_UI, theme: "light" });
});

test("after dispose the computer's preferences no longer move the display", () => {
  const { store, queries, shown } = harness();
  store.dispose();
  queries.dark.flip(true);
  expect(shown).toHaveLength(0);
});

test("displayOf maps every setting to the display it shows", () => {
  const ui: UiSettings = {
    theme: "system",
    font_size: 14,
    language: "hi",
    lite: "full",
    motion: "system",
  };
  expect(displayOf(ui, { dark: true, reduced: true, platform: "linux" })).toEqual({
    theme: "night",
    render: "full",
    motion: "reduced",
    lang: "hi",
    textSize: 14,
    platform: "linux",
  });
});

/* ---------- with a NextHost (H7), over FakeTransport ---------- */

const HOST_SETTINGS: Settings = {
  ui: { theme: "dark", font_size: 17, language: "hi", lite: "lite", motion: "reduced" },
  locked: [],
};

function withHost(settings: Settings = HOST_SETTINGS) {
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings };
        throw new Error("not scripted");
      },
    },
  });
  return { transport, ...harness({ host: nextHostOf(transport).settings }) };
}

test("a transport whose host has no H7 gives no settings part; one scripting other rows only gives none either", () => {
  expect(nextHostOf(new FakeTransport()).settings).toBeNull();
  expect(nextHostOf(withHost().transport).settings).not.toBeNull();
  const papersOnly = new FakeTransport({
    next: { parts: ["papers"], query: () => ({ matters: [] }) },
  });
  expect(nextHostOf(papersOnly).settings).toBeNull();
  expect(nextHostOf(papersOnly).papers).not.toBeNull();
});

test("with a NextHost the store reads get_settings at start and shows them", async () => {
  const { transport, store } = withHost();
  await settled();
  expect(transport.only("next_query")).toEqual([{ call: "next_query", query: "get_settings" }]);
  expect(store.ui()).toEqual(HOST_SETTINGS.ui);
  expect(store.display()).toMatchObject({
    theme: "night",
    lang: "hi",
    render: "lite",
    textSize: 17,
  });
  expect(store.batch()).toBe(liteBatch);
});

test("with a NextHost set sends a set_ui_settings patch of the changed keys; localStorage keeps the host's answer", async () => {
  const { transport, store, saved } = withHost();
  await settled();
  store.set({ theme: "light" });
  expect(transport.only("next_command")).toEqual([
    { call: "next_command", command: { set_ui_settings: { patch: { theme: "light" } } } },
  ]);
  expect(store.display().theme).toBe("day");
  // localStorage keeps the host's last answer, not the lawyer's unconfirmed change
  expect(saved()).toEqual(HOST_SETTINGS.ui);
});

test("with a NextHost the store follows settings_changed", async () => {
  const { transport, store } = withHost();
  await settled();
  transport.settingsChanged({
    ui: { ...HOST_SETTINGS.ui, language: "en", lite: "full" },
    locked: [],
  });
  expect(store.display()).toMatchObject({ lang: "en", render: "full" });
  store.dispose();
  transport.settingsChanged(HOST_SETTINGS);
  expect(store.display().lang).toBe("en");
});

test("a repeated settings_changed with the same values shows nothing new and sends nothing back", async () => {
  // the bridge's frame after a restart and, once H4 lands, the host's catch-up frame may both arrive
  const { transport, store, shown, saved } = withHost();
  await settled();
  const before = shown.length;
  transport.settingsChanged(HOST_SETTINGS);
  transport.settingsChanged({ ...HOST_SETTINGS, ui: { ...HOST_SETTINGS.ui } });
  expect(shown.length).toBe(before);
  expect(store.ui()).toEqual(HOST_SETTINGS.ui);
  expect(store.locked()).toEqual(HOST_SETTINGS.locked);
  expect(transport.only("next_command")).toEqual([]);
  expect(saved()).toEqual(HOST_SETTINGS.ui);
});

test("without a host no key is locked", () => {
  expect(harness().store.locked()).toEqual([]);
});

/* ---------- over the desktop host's wire (host-wire.ts) ---------- */

const WIRE_VIEW: SettingsView = {
  ui: { theme: "dark", font_size: 17, language: "en", lite: "off", motion: "system" },
  tools: { web_search: true, browser: false },
  privacy: { counts: true, crash_reports: false, counts_started: false },
  locked: ["ui.theme"],
};

/** The wire settings host over a scripted source: get_settings answers `held`, set_ui_settings is refused while
 * `refuse` holds an error. */
function overWire() {
  const wire = { held: WIRE_VIEW, refuse: null as unknown, reads: 0 };
  const listeners = new Set<(event: HostEvent) => void>();
  const host = wireSettingsHost({
    query: async () => {
      wire.reads += 1;
      return { settings: wire.held };
    },
    dispatch: async () => {
      if (wire.refuse !== null) throw wire.refuse;
      return "done";
    },
    onAppEvent: (listener) => {
      listeners.add(listener);
      return { ready: Promise.resolve(), stop: () => void listeners.delete(listener) };
    },
  });
  const changed = (view: SettingsView) => {
    wire.held = view;
    for (const listener of listeners) listener({ settings_changed: view });
  };
  return { wire, changed, ...harness({ host }) };
}

test("over the wire the locked keys come from get_settings and follow settings_changed", async () => {
  const { store, changed, saved } = overWire();
  await settled();
  expect(store.locked()).toEqual(["ui.theme"]);
  expect(store.ui()).toMatchObject({ theme: "dark", lite: "full", font_size: 17 });
  expect(saved()).toEqual(store.ui());
  changed({ ...WIRE_VIEW, ui: { ...WIRE_VIEW.ui, lite: "on" }, locked: ["ui.theme", "ui.lite"] });
  expect(store.locked()).toEqual(["ui.theme", "ui.lite"]);
  expect(store.display().render).toBe("lite");
  expect(saved()).toMatchObject({ lite: "lite" });
});

test("the first frame shows the host's last answer from localStorage, before the host answers again", () => {
  const cached = JSON.stringify({ ...DEFAULT_UI, theme: "dark" });
  const host: SettingsHost = {
    getSettings: () => new Promise(() => {}),
    setUiSettings: async () => {},
    onSettingsChanged: () => () => {},
  };
  const { started } = harness({ host }, cached);
  expect(started[0]?.theme).toBe("night");
});

test("a get_settings answer that a settings_changed overtook is dropped", async () => {
  let answer: (settings: Settings) => void = () => {};
  let changed: (settings: Settings) => void = () => {};
  const host: SettingsHost = {
    getSettings: () => new Promise((resolve) => (answer = resolve)),
    setUiSettings: async () => {},
    onSettingsChanged: (listener) => {
      changed = listener;
      return () => {};
    },
  };
  const { store } = harness({ host });
  // the event, newer, reaches the page before the read's answer
  changed({ ui: { ...HOST_SETTINGS.ui, theme: "light" }, locked: ["ui.lite"] });
  answer({ ...HOST_SETTINGS, ui: { ...HOST_SETTINGS.ui, theme: "dark" } });
  await settled();
  expect(store.ui().theme).toBe("light");
  expect(store.locked()).toEqual(["ui.lite"]);
});

test("a set the host refuses re-reads the settings and shows what the host holds", async () => {
  const { store, wire } = overWire();
  await settled();
  wire.refuse = { code: "settings.locked", message: "locked", retryable: false };
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  try {
    store.set({ theme: "light" });
    expect(store.ui().theme).toBe("light");
    await settled();
    expect(wire.reads).toBe(2);
    expect(store.ui().theme).toBe("dark");
    expect(store.display().theme).toBe("night");
  } finally {
    warn.mockRestore();
  }
});

/* ---------- two windows on one origin, and the store's listeners ---------- */

/** Two windows' stores over one localStorage, each with its own window for storage events. A write in one reaches the
 * other as the browser delivers it: a `storage` event on every other window, never on the writer's own. */
function twoWindows(host: { a?: SettingsHost | null; b?: SettingsHost | null } = {}) {
  const items = new Map<string, string>();
  const windows = [new EventTarget(), new EventTarget()] as const;
  const shown: [Display[], Display[]] = [[], []];
  const make = (at: 0 | 1, settings: SettingsHost | null) =>
    createDisplayStore({
      storage: {
        getItem: (key) => items.get(key) ?? null,
        setItem: (key, value) => {
          const old = items.get(key) ?? null;
          items.set(key, value);
          const other = windows[at === 0 ? 1 : 0];
          other.dispatchEvent(new StorageEvent("storage", { key, oldValue: old, newValue: value }));
        },
      },
      matchMedia: null,
      events: windows[at],
      host: settings,
      start: () => {},
      show: (d) => shown[at].push(d),
    });
  return { a: make(0, host.a ?? null), b: make(1, host.b ?? null), items, windows, shown };
}

test("without a host, a change in one window applies in the other at once, through the storage event", () => {
  const { a, b, shown } = twoWindows();
  const heard = vi.fn();
  b.subscribe(heard);
  a.set({ theme: "dark", language: "hi", font_size: 20, lite: "lite", motion: "reduced" });
  expect(b.ui()).toEqual({
    theme: "dark",
    language: "hi",
    font_size: 20,
    lite: "lite",
    motion: "reduced",
  });
  expect(b.display()).toMatchObject({
    theme: "night",
    lang: "hi",
    textSize: 20,
    render: "lite",
    motion: "reduced",
  });
  expect(shown[1]).toHaveLength(1);
  expect(heard).toHaveBeenCalledTimes(1);
  // and back the other way
  b.set({ theme: "light" });
  expect(a.ui().theme).toBe("light");
  expect(a.display().theme).toBe("day");
});

test("a storage event for another key is ignored, and clearing the storage brings back the defaults", () => {
  const { a, b, windows } = twoWindows();
  a.set({ theme: "dark" });
  windows[1].dispatchEvent(new StorageEvent("storage", { key: "other", newValue: "{}" }));
  expect(b.ui().theme).toBe("dark");
  windows[1].dispatchEvent(new StorageEvent("storage", { key: null }));
  expect(b.ui()).toEqual(DEFAULT_UI);
});

test("an unreadable value from the other window shows the defaults", () => {
  const { b, windows } = twoWindows();
  windows[1].dispatchEvent(
    new StorageEvent("storage", { key: DISPLAY_KEY, newValue: "{not json" }),
  );
  expect(b.ui()).toEqual(DEFAULT_UI);
});

test("with a host the storage event is not heard: settings_changed reaches both windows, once each", async () => {
  const transport = new FakeTransport({
    next: {
      query: (query) => {
        if (query === "get_settings") return { settings: HOST_SETTINGS };
        throw new Error("not scripted");
      },
    },
  });
  const settings = nextHostOf(transport).settings;
  const { a, b, shown } = twoWindows({ a: settings, b: settings });
  await settled();
  const before = shown[1].length;
  const heard = vi.fn();
  b.subscribe(heard);
  // the host's answer, as both windows hear it: each writes it to localStorage as the next start's cache, which fires
  // a storage event in the other window; only the settings_changed applies
  transport.settingsChanged({ ui: { ...HOST_SETTINGS.ui, theme: "light" }, locked: [] });
  expect(a.ui().theme).toBe("light");
  expect(b.ui().theme).toBe("light");
  expect(shown[1].length).toBe(before + 1);
  expect(heard).toHaveBeenCalledTimes(1);
});

test("after dispose the other window's changes no longer arrive", () => {
  const { a, b } = twoWindows();
  b.dispose();
  a.set({ theme: "dark" });
  expect(b.ui().theme).toBe("system");
});

test("subscribe hears each change of the settings or the locked keys, not the display alone, until unsubscribed", async () => {
  const { store, queries } = harness();
  const heard = vi.fn();
  const stop = store.subscribe(heard);
  // theme system to light under a light computer: the display stays day, the settings changed
  store.set({ theme: "light" });
  expect(heard).toHaveBeenCalledTimes(1);
  store.override({ language: "hi" });
  expect(heard).toHaveBeenCalledTimes(2);
  queries.reduced.flip(true);
  store.setPlatform("windows");
  expect(heard).toHaveBeenCalledTimes(2);
  stop();
  store.set({ theme: "dark" });
  expect(heard).toHaveBeenCalledTimes(2);
  // a host's settings_changed that changes only the locked keys
  const { transport, store: hosted } = withHost();
  await settled();
  const onHost = vi.fn();
  hosted.subscribe(onHost);
  transport.settingsChanged({ ui: HOST_SETTINGS.ui, locked: ["ui.theme"] });
  expect(hosted.locked()).toEqual(["ui.theme"]);
  expect(onHost).toHaveBeenCalledTimes(1);
});

test("connectRoom's store hears the page's storage events, and sets data-frame=own", () => {
  const transport = new FakeTransport();
  const store = connectRoom(transport);
  expect(document.documentElement.dataset["frame"]).toBe("own");
  window.dispatchEvent(
    new StorageEvent("storage", { key: DISPLAY_KEY, newValue: JSON.stringify({ font_size: 19 }) }),
  );
  expect(store.ui().font_size).toBe(19);
  expect(document.documentElement.dataset["textSize"]).toBe("19");
  store.dispose();
});
