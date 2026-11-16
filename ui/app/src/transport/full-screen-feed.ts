// Whether the main window is in the OS's own full screen, as the desktop reads it (`window_events.rs`): after
// `subscribe_app` the app feed carries `{"window_full_screen": <bool>}`, the current value first, then each change.
// A notched Mac puts a full-screen window below the camera band, so the window is shorter than its screen and the
// page's own size read says "windowed"; the desktop's word stands in for it (useFullScreen's `given`). The feed keeps
// the last value, so a listener that comes later starts from it.

/** A transport whose app feed carries the desktop's full-screen value: the desktop one and the fake. The room asks for
 * it by duck type, so it is not part of HostTransport. */
export interface FullScreenSource {
  /** Hears each value; a listener that comes once a value is in hears that one at once. Returns the unsubscribe. */
  onFullScreen(listener: (fullScreen: boolean) => void): () => void;
}

/** Whether a transport carries the full-screen value. */
export const isFullScreenSource = (t: object): t is FullScreenSource =>
  "onFullScreen" in t && typeof (t as { onFullScreen: unknown }).onFullScreen === "function";

/** The value a channel message carries: undefined for a message that is not `window_full_screen`, null for one whose
 * value is not a boolean. */
export function fullScreenOf(event: object): boolean | null | undefined {
  if (!("window_full_screen" in event)) return undefined;
  const value = (event as { window_full_screen?: unknown }).window_full_screen;
  return typeof value === "boolean" ? value : null;
}

export class FullScreenFeed implements FullScreenSource {
  #last: boolean | null = null;
  readonly #listeners = new Set<(fullScreen: boolean) => void>();
  readonly #warn: (message: string) => void;

  constructor(warn: (message: string) => void) {
    this.#warn = warn;
  }

  /** How many listeners it has: the desktop transport subscribes the app feed again for them on a new channel. */
  get listeners(): number {
    return this.#listeners.size;
  }

  /** Takes a channel message that is `window_full_screen`: delivers its value, or warns when it has none. Returns
   * whether the message was one, so the reader hands no other arm a message this took. */
  take(event: object): boolean {
    const fullScreen = fullScreenOf(event);
    if (fullScreen === undefined) return false;
    if (fullScreen === null) this.#warn("window_full_screen carried no boolean");
    else this.deliver(fullScreen);
    return true;
  }

  /** One value from the channel. */
  deliver(fullScreen: boolean): void {
    this.#last = fullScreen;
    for (const listener of this.#listeners) {
      try {
        listener(fullScreen);
      } catch (error) {
        this.#warn(`a full-screen listener threw: ${String(error)}`);
      }
    }
  }

  onFullScreen(listener: (fullScreen: boolean) => void): () => void {
    this.#listeners.add(listener);
    if (this.#last !== null) listener(this.#last);
    return () => {
      this.#listeners.delete(listener);
    };
  }
}
