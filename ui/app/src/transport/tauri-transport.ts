// The transport inside the desktop app: Tauri's IPC. The only module that imports `@tauri-apps/api` (with the generated
// bindings beside it). One channel per page load, opened once by the module singleton, never per React effect.
import { Channel, isTauri } from "@tauri-apps/api/core";
import { commands, type StreamMessage } from "./bindings.ts";
import { ChannelReader } from "./channel.ts";
import {
  NullTransport,
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
  HostQuery,
  PermissionRequestId,
  QueryResult,
  StreamId,
  ThreadId,
  ViewEpoch,
  ViewPosition,
} from "./protocol.ts";
import { Subscriptions } from "./subscriptions.ts";

type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: unknown };

/** One command's answer as a promise of its data. An error rejects as a `HostError`: the host's own, or `internal` for
 * a Tauri string rejection (which the bindings type as a `HostError` but is a string) or a thrown `Error`. */
async function call<T>(answer: Promise<Answer<T>>): Promise<T> {
  let settled: Answer<T>;
  try {
    settled = await answer;
  } catch (error) {
    throw toHostError(error);
  }
  if (settled.status === "ok") return settled.data;
  throw toHostError(settled.error);
}

export class TauriTransport implements HostTransport {
  readonly kind = "tauri";
  readonly #reader: ChannelReader;
  readonly #subs: Subscriptions;
  readonly #opened: Promise<void>;

  constructor(warn: (message: string) => void = (m) => console.warn(m)) {
    this.#subs = new Subscriptions(
      {
        subscribe: async (stream, after) => {
          await this.#opened;
          await call(commands.subscribe(stream, after));
        },
        unsubscribe: async (stream) => {
          await this.#opened;
          await call(commands.unsubscribe(stream));
        },
      },
      (stream, error) =>
        warn(`subscription to ${stream} failed: ${JSON.stringify(toHostError(error))}`),
    );
    this.#reader = new ChannelReader((event) => {
      if (event.patches !== undefined) this.#subs.deliver(event.patches);
    }, warn);
    const channel = new Channel<StreamMessage>((message) => this.#reader.receive(message));
    this.#opened = call(commands.openStream(channel)).then(() => undefined);
    this.#opened.catch((e: unknown) => warn(`open_stream failed: ${JSON.stringify(e)}`));
  }

  dispatch(command: CommandEnvelope): Promise<CommandOutcome> {
    return call(commands.dispatch(command));
  }
  query(query: HostQuery): Promise<QueryResult> {
    return call(commands.query(query));
  }
  async answerPermission(
    thread: ThreadId,
    request: PermissionRequestId,
    option: AgentChoiceId,
  ): Promise<void> {
    await call(commands.answerPermission(thread, request, option));
  }
  pickFolder(): Promise<FolderRefId | null> {
    return call(commands.pickFolder());
  }
  displayInfo(): Promise<DisplayInfo> {
    return call(commands.displayInfo());
  }
  subscribe(stream: StreamId, after: ViewPosition | null, listener: BatchListener): () => void {
    return this.#subs.add(stream, after, listener);
  }
  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    this.#subs.resubscribe(stream, after);
  }
  async ack(stream: StreamId, epoch: ViewEpoch, cursor: Cursor, frames: number): Promise<void> {
    await call(commands.ack(stream, epoch, cursor, frames));
  }
  frames(): number {
    return this.#reader.frames;
  }
}

let shared: HostTransport | null = null;

/** The page's transport: Tauri's inside the desktop app, made once per page load; a `NullTransport` elsewhere (the
 * dev server in a browser). */
export function pageTransport(): HostTransport {
  shared ??= isTauri() ? new TauriTransport() : new NullTransport();
  return shared;
}
