// H7, H8, R15 and H3 to H6 over the desktop host's wire: the room's spellings to the wire's and back, one feed for every
// listener, the read after the subscribe, the matters' reads and commands, and the app events.
import { expect, test } from "vitest";
import type { AppFeed } from "../transport/host-transport.ts";
import type {
  CommandEnvelope,
  CommandOutcome,
  HostEvent,
  HostQuery,
  MatterSummary,
  QueryResult,
  SettingsView,
  ThreadSummary,
} from "../transport/protocol.ts";
import { rowWords } from "./conversation-status.ts";
import type { AgentSummary, AppEvent, MattersCommand, UiLite, UiMotion } from "./host-next.ts";
import { nextHostOf, type Papers } from "./host-next.ts";
import {
  roomMatterOf,
  wireAgentsHost,
  wireAppHost,
  wireMattersHost,
  wirePapersHost,
  wireSettingsHost,
  type WireSettingsSource,
} from "./host-wire.ts";
import type { HostTransport } from "../transport/host-transport.ts";

const VIEW: SettingsView = {
  ui: { theme: "dark", font_size: 18, language: "hi", lite: "on", motion: "reduce" },
  tools: { web_search: true, browser: false },
  privacy: { counts: true, crash_reports: false, counts_started: false },
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

test("get_settings maps every wire lite and motion to the room's, and keeps the tool switches and the locked keys as they came", async () => {
  for (const [lite, wireLite] of LITES) {
    for (const [motion, wireMotion] of MOTIONS) {
      const view = {
        ...VIEW,
        ui: { ...VIEW.ui, lite: wireLite, motion: wireMotion },
      } as SettingsView;
      const host = wireSettingsHost(fakeSource(() => view).source);
      await expect(host.getSettings()).resolves.toEqual({
        ui: { theme: "dark", font_size: 18, language: "hi", lite, motion },
        tools: { web_search: true, browser: false },
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

test("set_tool_settings sends only the switches given, as the wire spells them, with a fresh id", async () => {
  const { source, commands } = fakeSource();
  const host = wireSettingsHost(source);
  await host.setToolSettings({ browser: true });
  expect(commands.at(-1)?.command).toEqual({ set_tool_settings: { patch: { browser: true } } });
  await host.setToolSettings({ web_search: false, browser: false });
  expect(commands.at(-1)?.command).toEqual({
    set_tool_settings: { patch: { web_search: false, browser: false } },
  });
  expect(new Set(commands.map((c) => c.command_id)).size).toBe(2);
});

test("a settings_changed carries the tool switches to the listeners", async () => {
  const fake = fakeSource();
  const host = wireSettingsHost(fake.source);
  const heard: unknown[] = [];
  host.onSettingsChanged((s) => heard.push(s.tools));
  fake.emit({ ...VIEW, tools: { web_search: false, browser: true } });
  expect(heard).toEqual([{ web_search: false, browser: true }]);
});

test("a refused tool switch rejects with the host's error", async () => {
  const error = { code: "settings.locked", message: "locked", retryable: false };
  const { source } = fakeSource();
  const host = wireSettingsHost({ ...source, dispatch: () => Promise.reject(error) });
  await expect(host.setToolSettings({ browser: true })).rejects.toEqual(error);
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

test("a transport with the app feed (the desktop's) gives settings, papers, agents, matters and app, and no other part", () => {
  const { source } = fakeSource();
  const next = nextHostOf(source as unknown as HostTransport);
  expect(next.settings).not.toBeNull();
  expect(next.papers).not.toBeNull();
  expect(next.agents).not.toBeNull();
  expect(next.matters).not.toBeNull();
  expect(next.app).not.toBeNull();
  expect([next.location, next.restart, next.retry]).toEqual([null, null, null]);
});

/* ---------- matters and the app events ---------- */

const M1 = "0190a1b2-c3d4-7e5f-8a9b-000000000001";
const M2 = "0190a1b2-c3d4-7e5f-8a9b-000000000002";
const T1 = "0190a1b2-c3d4-7e5f-8a9b-000000000011";

/** A wire row with every detail, and one from an older host with none. */
const FULL: MatterSummary = {
  matter: M1,
  name: "Acme Ltd v. State",
  kind: "folder",
  last_activity: 5,
  client: "Asha Rao",
  next_date: "2026-11-12",
  pinned: true,
  last_opened: 4,
  status: { stopped: { at: 3 } },
  folder_display: "Documents/Acme",
  folder_state: "missing",
};
const BARE: MatterSummary = { matter: M2, name: "Sharma v. State of Delhi", kind: "folder" };
const BARE_ROOM = { ...BARE, pinned: false, status: "idle", folder_state: "found" };

test("a matter row: every detail as it came, and each missing one as its binding reads it", () => {
  expect(roomMatterOf(FULL)).toEqual(FULL);
  expect(roomMatterOf(BARE)).toEqual(BARE_ROOM);
  // a folder state from a newer host reads as found
  expect(roomMatterOf({ ...BARE, folder_state: "unknown" }).folder_state).toBe("found");
});

test("matters: list_matters is asked, and its rows come back as the room reads them", async () => {
  const asked: HostQuery[] = [];
  const host = wireMattersHost({
    query: async (query) => {
      asked.push(query);
      return { matters: [FULL, BARE] };
    },
    dispatch: async () => "done",
  });
  await expect(host.listMatters()).resolves.toEqual([FULL, BARE_ROOM]);
  expect(asked).toEqual(["list_matters"]);
});

test("matters: list_threads names the matter, and a thread with no seen comes back without one and says no answer is ready", async () => {
  const unseen: ThreadSummary = {
    thread: T1,
    matter: M1,
    agent: "claude-acp",
    status: "idle",
    seen: false,
    last_stop: { reason: "end_turn", at: 7 },
  };
  const { seen: _, ...older } = unseen;
  const asked: HostQuery[] = [];
  const host = wireMattersHost({
    query: async (query) => {
      asked.push(query);
      return { threads: [unseen, older] };
    },
    dispatch: async () => "done",
  });
  const threads = await host.listThreads(M1);
  expect(threads).toEqual([unseen, older]);
  expect(asked).toEqual([{ list_threads: { matter: M1 } }]);
  // missing reads as seen (an older host's answer): only a false seen says the answer is ready
  expect(threads.map((t) => rowWords(t).state)).toEqual(["ready", "idle"]);
});

test("matters: another answer to a read rejects as internal, and the host's refusal as itself", async () => {
  const refused = { code: "matter.not_found", message: "gone", retryable: false };
  const other = wireMattersHost({
    query: async () => ({ agents: [] }),
    dispatch: async () => "done",
  });
  await expect(other.listMatters()).rejects.toMatchObject({ code: "internal" });
  await expect(other.listThreads(M1)).rejects.toMatchObject({ code: "internal" });
  const refusing = wireMattersHost({
    query: () => Promise.reject(refused),
    dispatch: async () => "done",
  });
  await expect(refusing.listMatters()).rejects.toEqual(refused);
  await expect(refusing.listThreads(M1)).rejects.toEqual(refused);
});

/** A matters host whose dispatch answers `outcome`, recording each envelope. */
function commanding(outcome: CommandOutcome | (() => Promise<never>)) {
  const sent: CommandEnvelope[] = [];
  const host = wireMattersHost({
    query: async () => ({ matters: [] }),
    dispatch: async (envelope) => {
      sent.push(envelope);
      return typeof outcome === "function" ? outcome() : outcome;
    },
  });
  return { host, sent };
}

test("create_matter goes as it is, with a fresh id, and answers its matter_created", async () => {
  const { host, sent } = commanding({ matter_created: { matter: M1 } });
  const create: MattersCommand = {
    create_matter: {
      name: "Acme",
      source: "default_location",
      client: "Asha",
      next_date: "2026-11-12",
    },
  };
  await expect(host.command(create)).resolves.toEqual({ matter_created: { matter: M1 } });
  await host.command(create);
  expect(sent.map((s) => s.command)).toEqual([create, create]);
  expect(new Set(sent.map((s) => s.command_id)).size).toBe(2);
});

test("create_matter answered done, or anything but matter_created, rejects as internal", async () => {
  const create: MattersCommand = { create_matter: { name: "Acme", source: "default_location" } };
  await expect(commanding("done").host.command(create)).rejects.toMatchObject({ code: "internal" });
  await expect(
    commanding({ thread_created: { thread: T1 } }).host.command(create),
  ).rejects.toMatchObject({ code: "internal" });
});

const DONE_COMMANDS: MattersCommand[] = [
  { update_matter: { matter: M1, pinned: true } },
  { open_matter: { matter: M1 } },
  { forget_matter: { matter: M1 } },
  { relocate_matter: { matter: M1, folder: "0190a1b2-c3d4-7e5f-8a9b-000000000021" } },
  { mark_seen: { thread: T1 } },
];

for (const command of DONE_COMMANDS) {
  const name = Object.keys(command)[0];
  test(`${name} goes as it is and answers done; another answer rejects as internal`, async () => {
    const { host, sent } = commanding("done");
    await expect(host.command(command)).resolves.toBe("done");
    expect(sent.map((s) => s.command)).toEqual([command]);
    await expect(
      commanding({ matter_created: { matter: M1 } }).host.command(command),
    ).rejects.toMatchObject({ code: "internal" });
  });
}

test("a refused command rejects with the host's error", async () => {
  const refused = { code: "matter.folder_missing", message: "gone", retryable: false };
  const { host } = commanding(() => Promise.reject(refused));
  await expect(host.command({ open_matter: { matter: M1 } })).rejects.toEqual(refused);
});

/** An app feed the test plays events into. */
function appFeed() {
  const listeners = new Set<(event: HostEvent) => void>();
  let feeds = 0;
  let refuse: (error: unknown) => void = () => {};
  const source = {
    onAppEvent(listener: (event: HostEvent) => void): AppFeed {
      feeds += 1;
      listeners.add(listener);
      const ready = new Promise<void>((_, fail) => (refuse = fail));
      return { ready, stop: () => void listeners.delete(listener) };
    },
  };
  const play = (event: HostEvent) => {
    for (const listener of listeners) listener(event);
  };
  return { source, play, listeners, feeds: () => feeds, refuse: (e: unknown) => refuse(e) };
}

test("matters_changed reaches every listener with its rows read as the room reads them, and its removed and all", () => {
  const feed = appFeed();
  const app = wireAppHost(feed.source);
  const heard: AppEvent[] = [];
  app.subscribeApp((e) => heard.push(e));
  feed.play({ matters_changed: { matters: [BARE], removed: [M1], all: false } });
  feed.play({ matters_changed: { matters: [FULL, BARE], removed: [], all: true } });
  expect(heard).toEqual([
    { matters_changed: { matters: [BARE_ROOM], removed: [M1], all: false } },
    { matters_changed: { matters: [FULL, BARE_ROOM], removed: [], all: true } },
  ]);
});

test("threads_changed reaches the listener as it came, a thread with no seen too", () => {
  const feed = appFeed();
  const heard: AppEvent[] = [];
  wireAppHost(feed.source).subscribeApp((e) => heard.push(e));
  const change = {
    matter: M1,
    threads: [{ thread: T1, matter: M1, agent: "codex", status: "idle" as const }],
    removed: [],
    all: true,
  };
  feed.play({ threads_changed: change });
  expect(heard).toEqual([{ threads_changed: change }]);
});

test("the app host passes no other app event: settings_changed is the settings host's", () => {
  const feed = appFeed();
  const heard: AppEvent[] = [];
  wireAppHost(feed.source).subscribeApp((e) => heard.push(e));
  feed.play({ settings_changed: VIEW });
  expect(heard).toEqual([]);
});

test("one app feed serves every listener; the last unsubscribe stops it, and a refused subscribe keeps the listener", async () => {
  const feed = appFeed();
  const app = wireAppHost(feed.source);
  const heard: string[] = [];
  const offOne = app.subscribeApp(() => heard.push("one"));
  const offTwo = app.subscribeApp(() => heard.push("two"));
  expect(feed.feeds()).toBe(1);
  feed.refuse({ code: "internal", message: "open_stream failed", retryable: true });
  await new Promise((resolve) => setTimeout(resolve, 0));
  feed.play({ matters_changed: { matters: [], removed: [M1], all: false } });
  expect(heard).toEqual(["one", "two"]);
  offOne();
  expect(feed.listeners.size).toBe(1);
  offTwo();
  expect(feed.listeners.size).toBe(0);
  app.subscribeApp(() => {});
  expect(feed.feeds()).toBe(2);
});
