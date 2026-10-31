// A transport for tests: the same channel reader and subscriptions as the Tauri one, with the host replaced by a
// recorder. `deliver` plays a channel message as the bridge would send it; `respond` scripts command answers.
import { ChannelReader } from "./channel.ts";
import {
  internalError,
  toHostError,
  type BatchListener,
  type DisplayInfo,
  type HostTransport,
} from "./host-transport.ts";
import type {
  AgentChoiceId,
  CommandEnvelope,
  CommandOutcome,
  Cursor,
  FolderRefId,
  HostEvent,
  HostQuery,
  PermissionRequestId,
  QueryResult,
  StreamId,
  ThreadId,
  ViewEpoch,
  ViewPosition,
} from "./protocol.ts";
import { Subscriptions } from "./subscriptions.ts";

/** One host call the fake saw, in order. */
export type FakeCall =
  | { call: "subscribe"; stream: StreamId; after: ViewPosition | null }
  | { call: "unsubscribe"; stream: StreamId }
  | { call: "ack"; stream: StreamId; epoch: ViewEpoch; cursor: Cursor; frames: number }
  | { call: "dispatch"; command: CommandEnvelope }
  | { call: "query"; query: HostQuery }
  | {
      call: "answer_permission";
      thread: ThreadId;
      request: PermissionRequestId;
      option: AgentChoiceId;
    }
  | { call: "pick_folder" };

export type FakeHost = {
  dispatch?: (command: CommandEnvelope) => CommandOutcome | Promise<CommandOutcome>;
  query?: (query: HostQuery) => QueryResult | Promise<QueryResult>;
  pickFolder?: () => FolderRefId | null;
  platform?: DisplayInfo["platform"];
};

export class FakeTransport implements HostTransport {
  readonly kind = "fake";
  readonly calls: FakeCall[] = [];
  /** Problems the channel reader reported. */
  readonly problems: string[] = [];
  /** Bytes of every channel message delivered, as serialized text. */
  bytes = 0;
  host: FakeHost;
  readonly #reader: ChannelReader;
  readonly #subs: Subscriptions;

  constructor(host: FakeHost = {}) {
    this.host = host;
    this.#subs = new Subscriptions(
      {
        subscribe: async (stream, after) => {
          this.calls.push({ call: "subscribe", stream, after });
        },
        unsubscribe: async (stream) => {
          this.calls.push({ call: "unsubscribe", stream });
        },
      },
      (_stream, error) => this.problems.push(String(error)),
    );
    this.#reader = new ChannelReader(
      (event: HostEvent) => {
        if ("patches" in event && event.patches !== undefined) this.#subs.deliver(event.patches);
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

  only<C extends FakeCall["call"]>(call: C): Extract<FakeCall, { call: C }>[] {
    return this.calls.filter((c): c is Extract<FakeCall, { call: C }> => c.call === call);
  }

  async dispatch(command: CommandEnvelope): Promise<CommandOutcome> {
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

  async pickFolder(): Promise<FolderRefId | null> {
    this.calls.push({ call: "pick_folder" });
    return this.host.pickFolder?.() ?? null;
  }

  async displayInfo(): Promise<DisplayInfo> {
    return { platform: this.host.platform ?? "linux" };
  }

  subscribe(stream: StreamId, after: ViewPosition | null, listener: BatchListener): () => void {
    return this.#subs.add(stream, after, listener);
  }

  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    this.#subs.resubscribe(stream, after);
  }

  async ack(stream: StreamId, epoch: ViewEpoch, cursor: Cursor, frames: number): Promise<void> {
    this.calls.push({ call: "ack", stream, epoch, cursor, frames });
  }

  frames(): number {
    return this.#reader.frames;
  }
}
