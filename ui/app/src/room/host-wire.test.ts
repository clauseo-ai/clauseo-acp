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
import type { AgentSummary, UiLite, UiMotion } from "./host-next.ts";
import { nextHostOf, type Papers } from "./host-next.ts";
import {
  wireAgentsHost,
  wirePapersHost,
  wireSettingsHost,
  type WireSettingsSource,
} from "./host-wire.ts";
import type { HostTransport } from "../transport/host-transport.ts";

const VIEW: SettingsView = {
  ui: { theme: "dark", font_size: 18, language: "hi", lite: "on", motion: "reduce" },
  tools: { web_search: true, browser: false },
  locked: ["ui.theme", "matters.default_location"],
};

/** A source whose subscribe answers when the test says, recording the calls in order. */
function fakeSource(view: () => SettingsView = () => VIEW) {
  const calls: string[] = [];
  const commands: CommandEnvelope[] = [];
  const listeners = new Set<(event: HostEvent) => void>();
  let answer: () => void = () => {};
  let refuse: (error: unknown) => void = () => {};
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
      const ready = new Promise<void>((done, fail) => {
        answer = () => {
          calls.push("subscribed");
          done();
        };
        refuse = fail;
      });
      return { ready, stop: () => void listeners.delete(listener) };
    },
  };
  const emit = (settings: SettingsView) => {
    for (const listener of listeners) listener({ settings_changed: settings });
  };
  return {
    source,
    calls,
    commands,
    emit,
    answer: () => answer(),
    refuse: (error: unknown) => refuse(error),
    feeds: () => feeds,
    listeners,
  };
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

test("a refused subscribe keeps the listener, so the events come once the transport subscribes again", async () => {
  const fake = fakeSource();
  const host = wireSettingsHost(fake.source);
  const heard: unknown[] = [];
  host.onSettingsChanged((s) => heard.push(s.ui.theme));
  fake.refuse({ code: "internal", message: "open_stream failed", retryable: true });
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(fake.listeners.size).toBe(1);
  // the transport's re-subscribe after its channel reopens: the same listener hears the next change
  fake.emit({ ...VIEW, ui: { ...VIEW.ui, theme: "light" } });
  expect(heard).toEqual(["light"]);
  expect(fake.feeds()).toBe(1);
});

const PAPERS: Papers = {
  papers: [
    {
      file: "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
      path: "drafts/lease.docx",
      size: 10,
      modified: 1,
    },
  ],
  skipped: [],
  unread: [],
  truncated: false,
};

test("papers: list_papers names the matter, and the answer's papers come back as they are", async () => {
  const asked: HostQuery[] = [];
  const host = wirePapersHost({
    query: async (query) => {
      asked.push(query);
      return { papers: PAPERS };
    },
  });
  await expect(host.listPapers("m-1")).resolves.toEqual(PAPERS);
  expect(asked).toEqual([{ list_papers: { matter: "m-1" } }]);
});

test("papers: another answer rejects as internal, and the host's refusal as itself", async () => {
  const refused = { code: "matter.folder_missing", message: "gone", retryable: false };
  await expect(
    wirePapersHost({ query: async () => ({ matters: [] }) }).listPapers("m-1"),
  ).rejects.toMatchObject({ code: "internal" });
  await expect(
    wirePapersHost({ query: () => Promise.reject(refused) }).listPapers("m-1"),
  ).rejects.toEqual(refused);
});

const AGENTS: AgentSummary[] = [
  { id: "claude-acp", kind: "claude" },
  { id: "codex-acp", kind: "codex" },
  { id: "fake-agent" },
];

test("agents: list_agents is asked, and the answer's agents come back as they are, a kind-less one too", async () => {
  const asked: HostQuery[] = [];
  const host = wireAgentsHost({
    query: async (query) => {
      asked.push(query);
      return { agents: AGENTS };
    },
  });
  await expect(host.listAgents()).resolves.toEqual(AGENTS);
  expect(asked).toEqual(["list_agents"]);
});

test("agents: another answer rejects as internal, and the host's refusal as itself", async () => {
  const refused = { code: "internal", message: "no settings", retryable: false };
  await expect(
    wireAgentsHost({ query: async () => ({ matters: [] }) }).listAgents(),
  ).rejects.toMatchObject({ code: "internal" });
  await expect(
    wireAgentsHost({ query: () => Promise.reject(refused) }).listAgents(),
  ).rejects.toEqual(refused);
});

test("a transport with the app feed (the desktop's) gives settings, papers and agents, and no other part", () => {
  const { source } = fakeSource();
  const next = nextHostOf(source as unknown as HostTransport);
  expect(next.settings).not.toBeNull();
  expect(next.papers).not.toBeNull();
  expect(next.agents).not.toBeNull();
  expect([next.matters, next.location, next.app, next.restart, next.retry]).toEqual([
    null,
    null,
    null,
    null,
    null,
  ]);
});
