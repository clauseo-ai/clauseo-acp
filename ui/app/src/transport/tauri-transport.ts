// The transport inside the desktop app: Tauri's IPC. The only module that imports `@tauri-apps/api` (with the generated
// bindings beside it). One channel per page load, opened once by the module singleton, never per React effect.
//
// Acknowledgement. The bridge holds the bytes of every channel message the window has not processed, and frees them on
// `ack(positions, messages)`: the positions the store applied, and how many channel messages the window has processed
// since `open_stream`. A message counts as processed once what it carried was applied (the store's ack after its
// flush says so), or at once when it carried nothing for a subscriber (a fragment piece, an event for a stream nobody
// holds, an event that is not patches, a stream_failed report). The store's acks of one flush and the message count go to the bridge as one
// call, a microtask after the flush; messages that carried nothing and no flush followed are acked on the next
// animation frame with no positions.
import { Channel, isTauri } from "@tauri-apps/api/core";
import { commands, type Ack, type StreamMessage } from "./bindings.ts";
import { ChannelReader, streamFailedOf, type ChannelEvent } from "./channel.ts";
import {
  NullTransport,
  toHostError,
  type BatchSubscriber,
  type DisplayInfo,
  type HostTransport,
} from "./host-transport.ts";
import type {
  AgentChoiceId,
  CommandEnvelope,
  CommandOutcome,
  FolderRefId,
  HostQuery,
  PermissionRequestId,
  QueryResult,
  StreamId,
  ThreadId,
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

export type TauriOptions = {
  warn?: (message: string) => void;
  /** Runs `run` on the next animation frame; a test passes its own. */
  nextFrame?: (run: () => void) => void;
};

export class TauriTransport implements HostTransport {
  readonly kind = "tauri";
  readonly #reader: ChannelReader;
  readonly #subs: Subscriptions;
  readonly #warn: (message: string) => void;
  readonly #nextFrame: (run: () => void) => void;
  #opened: Promise<void> | null = null;
  /** Positions the store acked since the last call, one per stream (the bridge refuses an ack naming a stream twice),
   * sent with the next. */
  #positions = new Map<StreamId, Ack>();
  /** The message count the bridge last heard. */
  #ackedMessages = 0;
  /** Messages before the first one since the store's last ack that handed a batch to a subscriber; null when there
   * is none. Messages from it on wait for the store's next ack. */
  #processedBefore: number | null = null;
  /** Whether the message being read handed a batch to a subscriber. */
  #carried = false;
  #sendQueued = false;
  #frameQueued = false;

  constructor(options: TauriOptions = {}) {
    const warn = options.warn ?? ((m: string) => console.warn(m));
    this.#warn = warn;
    this.#nextFrame = options.nextFrame ?? ((run) => requestAnimationFrame(() => run()));
    this.#subs = new Subscriptions(
      {
        subscribe: async (stream, after) => {
          await this.#open();
          await call(commands.subscribe(stream, after));
        },
        unsubscribe: async (stream) => {
          await this.#open();
          await call(commands.unsubscribe(stream));
        },
      },
      warn,
    );
    this.#reader = new ChannelReader(
      (event: ChannelEvent) => {
        const failed = streamFailedOf(event);
        if (failed !== null) {
          this.#subs.fail(failed.stream, toHostError(failed.error));
        } else if ("patches" in event && event.patches !== undefined) {
          this.#carried = this.#subs.deliver(event.patches) > 0;
        }
      },
      warn,
      () => this.#onMessage(),
    );
    void this.#open().catch(() => {});
  }

  /** Registers the channel, once; after a failed attempt the next host call tries again. */
  #open(): Promise<void> {
    if (this.#opened === null) {
      const channel = new Channel<StreamMessage>((message) => this.#reader.receive(message));
      const opened = call(commands.openStream(channel)).then(() => undefined);
      this.#opened = opened;
      opened.catch((e: unknown) => {
        this.#warn(`open_stream failed: ${JSON.stringify(toHostError(e))}`);
        if (this.#opened === opened) this.#opened = null;
      });
    }
    return this.#opened;
  }

  #onMessage(): void {
    if (this.#carried) {
      this.#processedBefore ??= this.#reader.eventStart;
      this.#carried = false;
      return;
    }
    if (this.#frameQueued) return;
    this.#frameQueued = true;
    this.#nextFrame(() => {
      this.#frameQueued = false;
      this.#send();
    });
  }

  /** One bridge call with the positions waiting and the messages processed, when there is anything new. */
  #send(): void {
    const messages = Math.min(
      this.#processedBefore ?? this.#reader.messages,
      this.#reader.unfinishedSince ?? this.#reader.messages,
    );
    if (this.#positions.size === 0 && messages <= this.#ackedMessages) return;
    const positions = [...this.#positions.values()];
    this.#positions.clear();
    this.#ackedMessages = Math.max(this.#ackedMessages, messages);
    call(commands.ack(positions, messages)).catch((e: unknown) =>
      this.#warn(`ack failed: ${JSON.stringify(toHostError(e))}`),
    );
  }

  ack(stream: StreamId, position: ViewPosition): void {
    this.#positions.set(stream, { stream, epoch: position.epoch, cursor: position.cursor });
    // The store acks after a flush, which applied every batch handed to it so far.
    this.#processedBefore = null;
    if (this.#sendQueued) return;
    this.#sendQueued = true;
    queueMicrotask(() => {
      this.#sendQueued = false;
      this.#send();
    });
  }

  /** Resolves once the window's channel is registered (`open_stream` answered); the perf build's `interactive` mark
   * waits for it. A failed attempt rejects, and the next call tries again. */
  connected(): Promise<void> {
    return this.#open();
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
  subscribe(stream: StreamId, after: ViewPosition | null, subscriber: BatchSubscriber): () => void {
    return this.#subs.add(stream, after, subscriber);
  }
  resubscribe(stream: StreamId, after: ViewPosition | null): void {
    this.#subs.resubscribe(stream, after);
  }
}

let shared: HostTransport | null = null;

/** The page's transport: Tauri's inside the desktop app, made once per page load; a `NullTransport` elsewhere (the
 * dev server in a browser). */
export function pageTransport(): HostTransport {
  shared ??= isTauri() ? new TauriTransport() : new NullTransport();
  return shared;
}
