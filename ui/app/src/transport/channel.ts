// One window's channel, read: each message is a `HostEvent`'s JSON or a fragment of one (R12 contract, "Channel
// messages"). Fragments are joined in index order and the joined text is parsed once. Every message counts as one
// frame, the number `ack` reports so the bridge can free the bytes it holds for unacknowledged frames.
import type { HostEvent } from "./protocol.ts";

/** A piece of an event's JSON text that was over the frame limit. */
export type Fragment = { id: number; index: number; count: number; text: string };

type Pending = { count: number; pieces: (string | undefined)[]; have: number };

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
  #frames = 0;
  readonly #pending = new Map<number, Pending>();
  readonly #onEvent: (event: HostEvent) => void;
  readonly #onError: (problem: string) => void;
  readonly #onFrame: () => void;

  /** `onFrame` runs after every message, once whatever event it completed has been handed on, so a frame that
   * delivers nothing (a fragment, an event for a stream nobody holds, an event that is not patches) still counts. */
  constructor(
    onEvent: (event: HostEvent) => void,
    onError: (problem: string) => void,
    onFrame: () => void = () => {},
  ) {
    this.#onEvent = onEvent;
    this.#onError = onError;
    this.#onFrame = onFrame;
  }

  /** Messages received so far, fragments one each. */
  get frames(): number {
    return this.#frames;
  }

  /** One channel message. */
  receive(raw: unknown): void {
    this.#frames += 1;
    try {
      this.#read(raw);
    } finally {
      this.#onFrame();
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
      this.#onEvent(message as HostEvent);
      return;
    }
    this.#piece(fragment);
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
    this.#onEvent(event as HostEvent);
  }
}
