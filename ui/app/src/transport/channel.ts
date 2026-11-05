// One window's channel, read: each message is a `HostEvent`'s JSON or a fragment of one (R12 contract, "Channel
// messages"). Fragments are joined in index order and the joined text is parsed once. Every message counts, fragments
// one each: the count the desktop transport acks so the bridge can free the bytes it holds for unprocessed messages.
//
// The e2e and marks builds wrap an event that fits in one message as `{"timed":{"received_ms":…,"event":{…}}}`, with
// the bridge's receive time (ms since the Unix epoch; R13 contract §2). The reader unwraps it only while a timed hook
// is installed (`setTimedHook`, which main.tsx does only when the build's init script set `__CLAUSEO_PERF__`), hands
// the event on as if it had come plain (one message, counted once), then reports it with its time to the hook. A
// release page never installs the hook and never gets a timed message, so its only cost is one null check per event.
import type { HostEvent, StreamFailed, StreamFailedMessage } from "./protocol.ts";

/** What a channel message carries once whole: a host event, or the bridge's report that it could not restart a
 * stream. */
export type ChannelEvent = HostEvent | StreamFailedMessage;

/** The stream failure an event reports, or null for a host event. */
export const streamFailedOf = (event: ChannelEvent): StreamFailed | null =>
  "stream_failed" in event && event.stream_failed !== undefined ? event.stream_failed : null;

/** A piece of an event's JSON text that was over the frame limit. */
export type Fragment = { id: number; index: number; count: number; text: string };

/** An event still arriving: its pieces, and the message count before its first piece. */
type Pending = { count: number; pieces: (string | undefined)[]; have: number; start: number };

/** Hears each unwrapped timed event, after it was handed on, with the bridge's receive time (epoch ms). */
export type TimedHook = (event: ChannelEvent, receivedMs: number) => void;

/** The installed timed hook: null in release. */
let timedHook: TimedHook | null = null;

/** Installs (or with null removes) the timed hook. While one is installed, `timed` messages are unwrapped. */
export function setTimedHook(hook: TimedHook | null): void {
  timedHook = hook;
}

/** The bridge's `timed` wrapper (contract §2). Like a plain event, it is trusted as the bridge sent it. */
type Timed = { timed?: { received_ms: number; event: ChannelEvent } };

function fragmentOf(message: unknown): Fragment | null {
  if (typeof message !== "object" || message === null || !("fragment" in message)) return null;
  const f = (message as { fragment: unknown }).fragment;
  if (typeof f !== "object" || f === null) return null;
  const { id, index, count, text } = f as Record<string, unknown>;
  const whole = (n: unknown): n is number => typeof n === "number" && Number.isInteger(n) && n >= 0;
  if (!whole(id) || !whole(index) || !whole(count) || typeof text !== "string") return null;
  if (count === 0 || index >= count) return null;
  return { id, index, count, text };
}

/** Decodes a message the webview got as text or bytes; an object is already parsed. */
function parsed(message: unknown): unknown {
  if (typeof message === "string") return JSON.parse(message);
  if (message instanceof ArrayBuffer) return JSON.parse(new TextDecoder().decode(message));
  if (message instanceof Uint8Array) return JSON.parse(new TextDecoder().decode(message));
  return message;
}

export class ChannelReader {
  #messages = 0;
  #eventStart = 0;
  readonly #pending = new Map<number, Pending>();
  readonly #onEvent: (event: ChannelEvent) => void;
  readonly #onError: (problem: string) => void;
  readonly #onMessage: () => void;

  /** `onMessage` runs after every message, once whatever event it completed has been handed to `onEvent`, so a
   * message that delivers nothing (a fragment, a malformed message) still reaches the caller. */
  constructor(
    onEvent: (event: ChannelEvent) => void,
    onError: (problem: string) => void,
    onMessage: () => void = () => {},
  ) {
    this.#onEvent = onEvent;
    this.#onError = onError;
    this.#onMessage = onMessage;
  }

  /** Messages received so far, fragments one each. */
  get messages(): number {
    return this.#messages;
  }

  /** For the event `onEvent` is handling: the message count before its first message (its first piece, if it came in
   * pieces). */
  get eventStart(): number {
    return this.#eventStart;
  }

  /** The message count before the first piece of the oldest event still arriving; null when none is. Messages from
   * there on carry an event nobody has had yet. */
  get unfinishedSince(): number | null {
    let since: number | null = null;
    for (const p of this.#pending.values())
      since = since === null ? p.start : Math.min(since, p.start);
    return since;
  }

  /** One channel message. */
  receive(raw: unknown): void {
    this.#messages += 1;
    try {
      this.#read(raw);
    } finally {
      this.#onMessage();
    }
  }

  #read(raw: unknown): void {
    let message: unknown;
    try {
      message = parsed(raw);
    } catch (error) {
      this.#onError(`a channel message is not JSON: ${String(error)}`);
      return;
    }
    const fragment = fragmentOf(message);
    if (fragment === null) {
      if (typeof message === "object" && message !== null && "fragment" in message) {
        this.#onError("a fragment is malformed");
        return;
      }
      if (timedHook !== null) {
        this.#timed(message, timedHook);
        return;
      }
      this.#eventStart = this.#messages - 1;
      this.#onEvent(message as ChannelEvent);
      return;
    }
    this.#piece(fragment);
  }

  /** A whole message while a timed hook is installed: a `timed` wrapper is unwrapped and its event handed on, then
   * reported to `hook`; anything else is handed on as it is. */
  #timed(message: unknown, hook: TimedHook): void {
    this.#eventStart = this.#messages - 1;
    const timed = (message as Timed | null)?.timed;
    if (timed === undefined) {
      this.#onEvent(message as ChannelEvent);
      return;
    }
    this.#onEvent(timed.event);
    hook(timed.event, timed.received_ms);
  }

  #piece(f: Fragment): void {
    let pending = this.#pending.get(f.id);
    if (pending === undefined) {
      // Fragment ids increase per window and pieces arrive in order, so an older unfinished event never completes.
      for (const id of this.#pending.keys()) {
        if (id < f.id) {
          this.#pending.delete(id);
          this.#onError(`fragment ${id} never completed`);
        }
      }
      pending = {
        count: f.count,
        pieces: Array.from<string | undefined>({ length: f.count }),
        have: 0,
        start: this.#messages - 1,
      };
      this.#pending.set(f.id, pending);
    }
    if (pending.count !== f.count || pending.pieces[f.index] !== undefined) {
      this.#pending.delete(f.id);
      this.#onError(`fragment ${f.id} has inconsistent pieces`);
      return;
    }
    pending.pieces[f.index] = f.text;
    pending.have += 1;
    if (pending.have < pending.count) return;
    this.#pending.delete(f.id);
    let event: unknown;
    try {
      event = JSON.parse(pending.pieces.join(""));
    } catch (error) {
      this.#onError(`fragment ${f.id} does not join to JSON: ${String(error)}`);
      return;
    }
    this.#eventStart = pending.start;
    this.#onEvent(event as ChannelEvent);
  }
}
