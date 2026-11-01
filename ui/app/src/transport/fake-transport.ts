// A transport for tests and the room's probe build (room/probe-main.tsx): the same channel reader and subscriptions as
// the Tauri one, with the host replaced by a recorder. `deliver` plays a channel message as the bridge would send it;
// `host` scripts the answers. It counts no messages: that bookkeeping is the desktop transport's own. With `host.next` it also answers the calls the host
// doesn't have yet (room/host-next.ts), which the desktop transport never sends.
import type {
  NextCommand,
  NextHost,
  NextHostSource,
  NextQuery,
  NextQueryResult,
  Papers,
  Settings,
} from "../room/host-next.ts";
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
  CommandEnvelope,
  CommandOutcome,
  HostError,
  HostQuery,
  PermissionRequestId,
  PickedFolder,
  QueryResult,
  StreamId,
  ThreadId,
  ViewPosition,
} from "./protocol.ts";
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
  | { call: "next_query"; query: NextQuery }
  | { call: "next_command"; command: NextCommand };

/** Scripted answers to the calls the host doesn't have yet (room/host-next.ts). */
export type FakeNextHost = {
  query: (query: NextQuery) => NextQueryResult | Promise<NextQueryResult>;
  /** Answers each command; throwing refuses it. Without it every command is done. */
  command?: (command: NextCommand) => void | Promise<void>;
};

export type FakeHost = {
  dispatch?: (command: CommandEnvelope) => CommandOutcome | Promise<CommandOutcome>;
  query?: (query: HostQuery) => QueryResult | Promise<QueryResult>;
  pickFolder?: () => PickedFolder | null;
  /** Answers each subscribe; throwing refuses it. */
  subscribe?: (stream: StreamId, after: ViewPosition | null) => void;
  platform?: DisplayInfo["platform"];
  /** With it, the fake is a NextHost (room/host-next.ts): H7's and H8's calls, answered from here. */
  next?: FakeNextHost;
};

export class FakeTransport implements HostTransport, NextHostSource {
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
  readonly #next: NextHost = {
    listPapers: async (matter) => {
      const result = await this.#nextQuery({ list_papers: { matter } });
      if (!("papers" in result)) throw internalError("list_papers answered without papers");
      return result.papers satisfies Papers;
    },
    getSettings: async () => {
      const result = await this.#nextQuery("get_settings");
      if (!("settings" in result)) throw internalError("get_settings answered without settings");
      return result.settings;
    },
    setUiSettings: async (patch) => {
      const command: NextCommand = { set_ui_settings: { patch } };
      this.calls.push({ call: "next_command", command });
      try {
        await this.host.next?.command?.(command);
      } catch (error) {
        throw toHostError(error);
      }
    },
    onSettingsChanged: (listener) => {
      this.#settingsHeard.add(listener);
      return () => this.#settingsHeard.delete(listener);
    },
  };

  constructor(host: FakeHost = {}) {
    this.host = host;
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

  /** The bridge's report that a stream failed, as its channel message would say. */
  failStream(stream: StreamId, error: HostError): void {
    this.#subs.fail(stream, error);
  }

  /** The calls the host doesn't have yet, when `host.next` scripts them; null otherwise. */
  nextHost(): NextHost | null {
    return this.host.next === undefined ? null : this.#next;
  }

  /** Plays `settings_changed` as the host would send it. */
  settingsChanged(settings: Settings): void {
    for (const listener of this.#settingsHeard) listener(settings);
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

  async displayInfo(): Promise<DisplayInfo> {
    return { platform: this.host.platform ?? "linux" };
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
