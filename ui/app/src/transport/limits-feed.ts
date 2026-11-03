// The limits on the window's channel (R16): after `open_stream` the host sends the whole view first, then the whole
// view again after each change, as `limits_view_changed`, in no stream and with no position. The feed keeps the last
// view, so a listener that comes after the first one still starts from it, and hands each new one to every listener.
// Nothing here runs a timer: the host's own timer publishes a view at each shown reset and at the stale mark.
import type { HostEvent, LimitsView } from "./protocol.ts";

/** A transport whose channel carries the limits view: the desktop one and the fake. The room asks for it by duck type
 * (store/limits.ts), so it is not part of HostTransport. */
export interface LimitsSource {
  /** Hears each limits view; a listener that comes once a view is in hears that one at once. Returns the unsubscribe. */
  onLimits(listener: (view: LimitsView) => void): () => void;
}

/** Whether a transport carries the limits view. */
export const isLimitsSource = (t: object): t is LimitsSource =>
  "onLimits" in t && typeof (t as { onLimits: unknown }).onLimits === "function";

/** The view a channel event carries, or null for any other event. */
export function limitsOf(event: HostEvent | object): LimitsView | null {
  if (!("limits_view_changed" in event)) return null;
  const view = (event as { limits_view_changed?: unknown }).limits_view_changed;
  return typeof view === "object" && view !== null && !Array.isArray(view)
    ? (view as LimitsView)
    : null;
}

export class LimitsFeed implements LimitsSource {
  #last: LimitsView | null = null;
  readonly #listeners = new Set<(view: LimitsView) => void>();
  readonly #warn: (message: string) => void;

  constructor(warn: (message: string) => void) {
    this.#warn = warn;
  }

  /** One view from the channel. */
  deliver(view: LimitsView): void {
    this.#last = view;
    for (const listener of this.#listeners) {
      try {
        listener(view);
      } catch (error) {
        this.#warn(`a limits listener threw: ${String(error)}`);
      }
    }
  }

  onLimits(listener: (view: LimitsView) => void): () => void {
    this.#listeners.add(listener);
    if (this.#last !== null) listener(this.#last);
    return () => {
      this.#listeners.delete(listener);
    };
  }
}
