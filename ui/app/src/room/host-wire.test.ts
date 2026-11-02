// H7 over the desktop host's wire: the room's spellings to the wire's and back, one feed for every listener, and the
// read after the subscribe.
import { expect, test } from "vitest";
import type { AppFeed } from "../transport/host-transport.ts";
import type {
  CommandEnvelope,
  HostEvent,
  HostQuery,
  QueryResult,
  SettingsView,
} from "../transport/protocol.ts";
import type { UiLite, UiMotion } from "./host-next.ts";
import { wireSettingsHost, type WireSettingsSource } from "./host-wire.ts";

const VIEW: SettingsView = {
  ui: { theme: "dark", font_size: 18, language: "hi", lite: "on", motion: "reduce" },
  locked: ["ui.theme", "matters.default_location"],
};

/** A source whose subscribe answers when the test says, recording the calls in order. */
function fakeSource(view: () => SettingsView = () => VIEW) {
  const calls: string[] = [];
  const commands: CommandEnvelope[] = [];
  const listeners = new Set<(event: HostEvent) => void>();
  let answer: () => void = () => {};
  let feeds = 0;
  const source: WireSettingsSource = {
    query: async (query: HostQuery): Promise<QueryResult> => {
      calls.push(`query ${JSON.stringify(query)}`);
      return { settings: view() };
    },
    dispatch: async (command) => {
      calls.push("dispatch");
      commands.push(command);
      return "done";
    },
    onAppEvent(listener): AppFeed {
      feeds += 1;
      listeners.add(listener);
      calls.push("subscribe");
      const ready = new Promise<void>((done) => {
        answer = () => {
          calls.push("subscribed");
          done();
        };
      });
      return { ready, stop: () => void listeners.delete(listener) };
    },
  };
  const emit = (settings: SettingsView) => {
    for (const listener of listeners) listener({ settings_changed: settings });
  };
  return { source, calls, commands, emit, answer: () => answer(), feeds: () => feeds, listeners };
}

const LITES: [UiLite, string][] = [
  ["automatic", "automatic"],
  ["full", "off"],
  ["lite", "on"],
];
const MOTIONS: [UiMotion, string][] = [
  ["system", "system"],
  ["full", "full"],
  ["reduced", "reduce"],
];

test("get_settings maps every wire lite and motion to the room's, and keeps the locked keys as they came", async () => {
  for (const [lite, wireLite] of LITES) {
    for (const [motion, wireMotion] of MOTIONS) {
      const view = {
        ...VIEW,
        ui: { ...VIEW.ui, lite: wireLite, motion: wireMotion },
      } as SettingsView;
      const host = wireSettingsHost(fakeSource(() => view).source);
      await expect(host.getSettings()).resolves.toEqual({
        ui: { theme: "dark", font_size: 18, language: "hi", lite, motion },
        locked: ["ui.theme", "matters.default_location"],
      });
    }
  }
});

test("set_ui_settings sends only the keys given, each lite and motion in the wire's spelling, with a fresh id", async () => {
  const { source, commands } = fakeSource();
  const host = wireSettingsHost(source);
  for (const [lite, wireLite] of LITES) {
    await host.setUiSettings({ lite });
    expect(commands.at(-1)?.command).toEqual({ set_ui_settings: { patch: { lite: wireLite } } });
  }
  for (const [motion, wireMotion] of MOTIONS) {
    await host.setUiSettings({ motion });
    expect(commands.at(-1)?.command).toEqual({
      set_ui_settings: { patch: { motion: wireMotion } },
    });
  }
  await host.setUiSettings({ theme: "light", font_size: 20, language: "en" });
  expect(commands.at(-1)?.command).toEqual({
    set_ui_settings: { patch: { theme: "light", font_size: 20, language: "en" } },
  });
  expect(new Set(commands.map((c) => c.command_id)).size).toBe(commands.length);
});

test("a refused set rejects with the host's error", async () => {
  const error = { code: "settings.locked", message: "locked", retryable: false };
  const { source } = fakeSource();
  const host = wireSettingsHost({ ...source, dispatch: () => Promise.reject(error) });
  await expect(host.setUiSettings({ theme: "light" })).rejects.toEqual(error);
});

test("getSettings waits for the feed's subscribe before it reads", async () => {
  const fake = fakeSource();
  const host = wireSettingsHost(fake.source);
  host.onSettingsChanged(() => {});
  const read = host.getSettings();
  await Promise.resolve();
  expect(fake.calls).toEqual(["subscribe"]);
  fake.answer();
  await read;
  expect(fake.calls).toEqual(["subscribe", "subscribed", 'query "get_settings"']);
});

test("one feed serves every listener, mapped to the room's spelling; the last unsubscribe stops it", async () => {
  const fake = fakeSource();
  const host = wireSettingsHost(fake.source);
  const heard: unknown[] = [];
  const offOne = host.onSettingsChanged((s) => heard.push(["one", s.ui.lite, s.locked]));
  const offTwo = host.onSettingsChanged((s) => heard.push(["two", s.ui.motion]));
  expect(fake.feeds()).toBe(1);
  fake.emit({ ...VIEW, ui: { ...VIEW.ui, lite: "off", motion: "system" }, locked: ["ui.lite"] });
  expect(heard).toEqual([
    ["one", "full", ["ui.lite"]],
    ["two", "system"],
  ]);
  offOne();
  expect(fake.listeners.size).toBe(1);
  offTwo();
  expect(fake.listeners.size).toBe(0);
});
