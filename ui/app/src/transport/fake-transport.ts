// A transport for tests and the room's probe build (room/probe-main.tsx): the same channel reader and subscriptions as
// the Tauri one, with the host replaced by a recorder. `deliver` plays a channel message as the bridge would send it;
// `host` scripts the answers. It counts no messages: that bookkeeping is the desktop transport's own. With `host.next` it also answers the calls the host
// beyond the generated ones (room/host-next.ts), scripted, and `appEvent` plays H4's events as the room reads them.
import type {
  AppEvent,
  NextCommand,
  NextHost,
  NextHostSource,
  NextPart,
  NextOutcome,
  NextQuery,
  NextQueryResult,
  Papers,
  Settings,
} from "../room/host-next.ts";
import { NO_NEXT, WIRE } from "../room/host-next.ts";
import { ChannelReader, streamFailedOf, type ChannelEvent } from "./channel.ts";
import {
  internalError,
  toHostError,
  type BatchSubscriber,
  type DisplayInfo,
  type HostTransport,
} from "./host-transport.ts";
import type {
  AgentChoiceId,
  AgentKind,
  LimitsView,
  RefreshWhy,
  CommandEnvelope,
  CommandOutcome,
  FileRefId,
  HostError,
  HostQuery,
  PermissionRequestId,
  PickedFolder,
  QueryResult,
  RevealTarget,
  SetupCommand,
  SetupEvent,
  StreamId,
  ThreadId,
  UiCount,
  ViewPosition,
} from "./protocol.ts";
import { LimitsFeed, limitsOf, type LimitsSource } from "./limits-feed.ts";
import { Subscriptions } from "./subscriptions.ts";

/** One host call the fake saw, in order. */
export type FakeCall =
  | { call: "subscribe"; stream: StreamId; after: ViewPosition | null }
  | { call: "unsubscribe"; stream: StreamId }
  | { call: "ack"; stream: StreamId; position: ViewPosition }
  | { call: "dispatch"; command: CommandEnvelope }
  | { call: "query"; query: HostQuery }
  | {
      call: "answer_permission";
      thread: ThreadId;
      request: PermissionRequestId;
      option: AgentChoiceId;
    }
  | { call: "pick_folder" }
  | { call: "open_external"; url: string }
  | { call: "open_file"; file: FileRefId }
  | { call: "reveal"; target: RevealTarget }
  | { call: "open_settings" }
  | { call: "next_query"; query: NextQuery }
  | { call: "next_command"; command: NextCommand }
  | { call: "setup"; command: SetupCommand }
  | { call: "count_ui"; event: UiCount };

/** The setup wire, scripted (R20's `setup` and S21's `count_ui`; store/setup.ts's SetupSource). */
export type FakeSetupHost = {
  /** The events a new listener hears first, as the host replays SetupInfo and each agent's state; none when left
   * out. */
  first?: () => readonly SetupEvent[];
  /** Answers each setup command; throwing refuses it. Its events, if any, are the test's to play (`setupEvent`). */
  command?: (command: SetupCommand) => void | Promise<void>;
};

/** Scripted answers to the calls the host doesn't have yet (room/host-next.ts). */
export type FakeNextHost = {
  /** The host rows it has (host-next.ts's NextHost parts); every part when left out. */
  parts?: readonly NextPart[];
  query: (query: NextQuery) => NextQueryResult | Promise<NextQueryResult>;
  /** Answers each command; throwing refuses it. Without it, or returning nothing, the command is done; create_matter
   * gives the outcome it returns. */
  command?: (
    command: NextCommand,
  ) => void | NextOutcome | Promise<void> | Promise<NextOutcome | undefined>;
  /** Optional: the first frames of each app subscription, as the host sends them (each matter's whole thread list,
   * `all: true`), played to the new listener alone once the subscription is made. */
  first?: () => readonly AppEvent[];
};

export type FakeHost = {
  dispatch?: (command: CommandEnvelope) => CommandOutcome | Promise<CommandOutcome>;
  query?: (query: HostQuery) => QueryResult | Promise<QueryResult>;
  pickFolder?: () => PickedFolder | null;
  /** Answer `openExternal`, `openFile` and `reveal`; throwing refuses (a HostError, such as the declined confirm's
   * `transport.unauthorized`). Without one the call succeeds. */
  openExternal?: (url: string) => void | Promise<void>;
  openFile?: (file: FileRefId) => void | Promise<void>;
  reveal?: (target: RevealTarget) => void | Promise<void>;
  /** With it, the fake has the settings window (`openSettings`), and this answers each open; throwing refuses. Without
   * it the fake has no `openSettings`, like a host without the window. Read when the fake is made. */
  openSettings?: () => void | Promise<void>;
  /** Answers each subscribe; throwing refuses it. */
  subscribe?: (stream: StreamId, after: ViewPosition | null) => void;
  platform?: DisplayInfo["platform"];
  /** The release date display_info reports; null, the dev and CI build's, when left out. */
  releaseDate?: DisplayInfo["release_date"];
  /** With it, the fake is a NextHost (room/host-next.ts): the rows the host doesn't have yet, answered from here. */
  next?: FakeNextHost;
  /** With it, the fake carries the setup wire (`onSetup`, `setup`, `countUi`), and the room loads the first run.
   * Without it the fake has none of the three, like a host before R20. Read when the fake is made. */
  setup?: FakeSetupHost;
};

export class FakeTransport implements HostTransport, NextHostSource, LimitsSource {
  readonly kind = "fake";
  readonly calls: FakeCall[] = [];
  /** Problems the channel reader reported. */
  readonly problems: string[] = [];
  /** Bytes of every channel message delivered, as serialized text. */
  bytes = 0;
  host: FakeHost;
  readonly #reader: ChannelReader;
  readonly #subs: Subscriptions;
  readonly #dispatched = new Set<(command: CommandEnvelope) => void>();
  readonly #settingsHeard = new Set<(settings: Settings) => void>();
  readonly #appHeard = new Set<(event: AppEvent) => void>();
  readonly #limits = new LimitsFeed((problem) => this.problems.push(problem));
  /** H14's settings window, when `host.openSettings` scripts it. */
  readonly openSettings: (() => Promise<void>) | undefined;
  /** The setup wire, when `host.setup` scripts it (store/setup.ts's SetupSource). */
  readonly onSetup: ((listener: (event: SetupEvent) => void) => () => void) | undefined;
  readonly setup: ((command: SetupCommand) => Promise<void>) | undefined;
  readonly countUi: ((event: UiCount) => Promise<void>) | undefined;
  readonly #setupHeard = new Set<(event: SetupEvent) => void>();
  readonly #parts: { [P in NextPart]-?: NonNullable<NextHost[P]> } = {
    papers: {
      listPapers: async (matter) => {
        const result = await this.#nextQuery({ [WIRE.listPapers]: { matter } });
        if (!("papers" in result)) throw internalError("list_papers answered without papers");
        return result.papers satisfies Papers;
      },
    },
    settings: {
      getSettings: async () => {
        const result = await this.#nextQuery(WIRE.getSettings);
        if (!("settings" in result)) throw internalError("get_settings answered without settings");
        return result.settings;
      },
      setUiSettings: async (patch) => {
        await this.#nextCommand({ [WIRE.setUiSettings]: { patch } });
      },
      setToolSettings: async (patch) => {
        await this.#nextCommand({ [WIRE.setToolSettings]: { patch } });
      },
      onSettingsChanged: (listener) => {
        this.#settingsHeard.add(listener);
        return () => this.#settingsHeard.delete(listener);
      },
    },
    matters: {
      listMatters: async () => {
        const result = await this.#nextQuery(WIRE.listMatters);
        if (!("matters" in result)) throw internalError("list_matters answered without matters");
        return result.matters;
      },
      listThreads: async (matter) => {
        const result = await this.#nextQuery({ [WIRE.listThreads]: { matter } });
        if (!("threads" in result)) throw internalError("list_threads answered without threads");
        return result.threads;
      },
      command: (command) => this.#nextCommand(command),
    },
    location: {
      getDefaultLocation: async () => {
        const result = await this.#nextQuery(WIRE.getDefaultLocation);
        if (!("default_location" in result))
          throw internalError("get_default_location answered without default_location");
        return result.default_location.display;
      },
    },
    app: {
      subscribeApp: (listener) => {
        this.#appHeard.add(listener);
        const first = this.host.next?.first?.() ?? [];
        if (first.length > 0)
          queueMicrotask(() => {
            for (const event of first) if (this.#appHeard.has(listener)) listener(event);
          });
        return () => this.#appHeard.delete(listener);
      },
    },
    restart: {
      restartAgent: async (thread) => {
        await this.dispatch({
          command_id: crypto.randomUUID(),
          command: { restart_agent: { thread } },
        });
      },
    },
    agents: {
      listAgents: async () => {
        const result = await this.#nextQuery(WIRE.listAgents);
        if (!("agents" in result)) throw internalError("list_agents answered without agents");
        return result.agents;
      },
    },
    retry: {
      retryFrom: async (thread, message) => {
        const outcome = await this.#nextCommand({ [WIRE.retryFrom]: { thread, message } });
        if (typeof outcome === "string" || !("thread_created" in outcome))
          throw internalError("retry_from answered without a thread");
        return outcome.thread_created.thread;
      },
    },
  };

  constructor(host: FakeHost = {}) {
    this.host = host;
    this.openSettings =
      host.openSettings === undefined
        ? undefined
        : async () => {
            this.calls.push({ call: "open_settings" });
            await this.#answer(() => this.host.openSettings?.());
          };
    const setup = host.setup;
    this.onSetup =
      setup === undefined
        ? undefined
        : (listener) => {
            this.#setupHeard.add(listener);
            const first = setup.first?.() ?? [];
            if (first.length > 0)
              queueMicrotask(() => {
                for (const event of first) if (this.#setupHeard.has(listener)) listener(event);
              });
            return () => this.#setupHeard.delete(listener);
          };
    this.setup =
      setup === undefined
        ? undefined
        : async (command) => {
            this.calls.push({ call: "setup", command });
            await this.#answer(() => setup.command?.(command));
          };
    this.countUi =
      setup === undefined
        ? undefined
        : async (event) => {
            this.calls.push({ call: "count_ui", event });
          };
    this.#subs = new Subscriptions(
      {
        subscribe: async (stream, after) => {
          this.calls.push({ call: "subscribe", stream, after });
          this.host.subscribe?.(stream, after);
        },
        unsubscribe: async (stream) => {
          this.calls.push({ call: "unsubscribe", stream });
        },
      },
      (problem) => this.problems.push(problem),
    );
    this.#reader = new ChannelReader(
      (event: ChannelEvent) => {
        const failed = streamFailedOf(event);
        if (failed !== null) this.#subs.fail(failed.stream, toHostError(failed.error));
        else if ("patches" in event && event.patches !== undefined)
          this.#subs.deliver(event.patches);
        else if ("limits_view_changed" in event) {
          const view = limitsOf(event);
          if (view === null) this.problems.push("limits_view_changed carried no view");
          else this.#limits.deliver(view);
        }
      },
      (problem) => this.problems.push(problem),
    );
  }

  /** One channel message, as text (what Rust sends) or as the object the webview parsed. */
  deliver(message: string | object): void {
    this.bytes += new TextEncoder().encode(
      typeof message === "string" ? message : JSON.stringify(message),
    ).length;
    this.#reader.receive(message);
  }

  /** Plays `limits_view_changed` on the channel, as the host sends it after `open_stream` and after each change. */
  limitsChanged(view: LimitsView): void {
    this.deliver({ limits_view_changed: view });
  }

  onLimits(listener: (view: LimitsView) => void): () => void {
    return this.#limits.onLimits(listener);
  }

  /** The `refresh_limits` commands dispatched, in order: the agent (undefined for every agent) and why. */
  refreshes(): { agent: AgentKind | undefined; why: RefreshWhy }[] {
    return this.only("dispatch").flatMap(({ command }) => {
      const refresh = command.command.refresh_limits;
      return refresh === undefined ? [] : [{ agent: refresh.agent, why: refresh.why }];
    });
  }

  /** The bridge's report that a stream failed, as its channel message would say. */
  failStream(stream: StreamId, error: HostError): void {
    this.#subs.fail(stream, error);
  }

  /** The calls the host doesn't have yet, the parts `host.next` scripts (every part unless it names some); each part
   * null without `host.next`. */
  nextHost(): NextHost {
    const next = this.host.next;
    if (next === undefined) return NO_NEXT;
    const has = (part: NextPart) => next.parts === undefined || next.parts.includes(part);
    return {
      papers: has("papers") ? this.#parts.papers : null,
      settings: has("settings") ? this.#parts.settings : null,
      matters: has("matters") ? this.#parts.matters : null,
      location: has("location") ? this.#parts.location : null,
      app: has("app") ? this.#parts.app : null,
      restart: has("restart") ? this.#parts.restart : null,
      agents: has("agents") ? this.#parts.agents : null,
      retry: has("retry") ? this.#parts.retry : null,
    };
  }

  /** Plays `settings_changed` as the host would send it. */
  settingsChanged(settings: Settings): void {
    for (const listener of this.#settingsHeard) listener(settings);
  }

  /** Plays one SetupEvent (R20) as the host's `setup` stream would send it. */
  setupEvent(event: SetupEvent): void {
    for (const listener of this.#setupHeard) listener(event);
  }

  /** The setup commands sent, in order. */
  setups(): SetupCommand[] {
    return this.only("setup").map((c) => c.command);
  }

  /** Plays one app event (H4) as the host would send it to every window; `{ browser_install: { state } }` is the
   * Chrome download's state (room/browser-install.ts). A new app listener hears `host.next.first` first, the catch-up. */
  appEvent(event: AppEvent): void {
    for (const listener of this.#appHeard) listener(event);
  }

  async #nextCommand(command: NextCommand): Promise<NextOutcome> {
    this.calls.push({ call: "next_command", command });
    try {
      return (await this.host.next?.command?.(command)) ?? "done";
    } catch (error) {
      throw toHostError(error);
    }
  }

  async #nextQuery(query: NextQuery): Promise<NextQueryResult> {
    this.calls.push({ call: "next_query", query });
    const next = this.host.next;
    if (next === undefined) throw internalError("the fake host answers no next query");
    try {
      return await next.query(query);
    } catch (error) {
      throw toHostError(error);
    }
  }

  only<C extends FakeCall["call"]>(call: C): Extract<FakeCall, { call: C }>[] {
    return this.calls.filter((c): c is Extract<FakeCall, { call: C }> => c.call === call);
  }

  connected(): Promise<void> {
    return Promise.resolve();
  }

  onDispatch(listener: (command: CommandEnvelope) => void): () => void {
    this.#dispatched.add(listener);
    return () => this.#dispatched.delete(listener);
  }

  async dispatch(command: CommandEnvelope): Promise<CommandOutcome> {
    for (const listener of this.#dispatched) listener(command);
    this.calls.push({ call: "dispatch", command });
    if (this.host.dispatch === undefined) return "done";
    try {
      return await this.host.dispatch(command);
    } catch (error) {
      throw toHostError(error);
    }
  }

  async query(query: HostQuery): Promise<QueryResult> {
    this.calls.push({ call: "query", query });
    if (this.host.query === undefined) throw internalError("the fake host answers no query");
    try {
      return await this.host.query(query);
    } catch (error) {
      throw toHostError(error);
    }
  }

  async answerPermission(
    thread: ThreadId,
    request: PermissionRequestId,
    option: AgentChoiceId,
  ): Promise<void> {
    this.calls.push({ call: "answer_permission", thread, request, option });
  }

  async pickFolder(): Promise<PickedFolder | null> {
    this.calls.push({ call: "pick_folder" });
    return this.host.pickFolder?.() ?? null;
  }

  async openExternal(url: string): Promise<void> {
    this.calls.push({ call: "open_external", url });
    await this.#answer(() => this.host.openExternal?.(url));
  }

  async openFile(file: FileRefId): Promise<void> {
    this.calls.push({ call: "open_file", file });
    await this.#answer(() => this.host.openFile?.(file));
  }

  async reveal(target: RevealTarget): Promise<void> {
    this.calls.push({ call: "reveal", target });
    await this.#answer(() => this.host.reveal?.(target));
  }

  async #answer(run: () => void | Promise<void>): Promise<void> {
    try {
      await run();
    } catch (error) {
      throw toHostError(error);
    }
  }

  async displayInfo(): Promise<DisplayInfo> {
    return { platform: this.host.platform ?? "linux", release_date: this.host.releaseDate ?? null };
  }

  subscribe(stream: StreamId, after: ViewPosition | null, subscriber: BatchSubscriber): () => void {
    return this.#subs.add(stream, after, subscriber);
  }

  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    this.#subs.resubscribe(stream, after);
  }

  ack(stream: StreamId, position: ViewPosition): void {
    this.calls.push({ call: "ack", stream, position });
  }
}
