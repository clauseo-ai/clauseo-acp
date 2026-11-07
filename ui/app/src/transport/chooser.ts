// Clauseo's own folder chooser, between a transport and the page. When Windows' folder window would stall on a share
// that is down, the desktop answers `pick_folder_or_choose` with a chooser session (`choose`) at once, before it knows
// where the session may start, and the transport hands it here: `run` resolves once the lawyer has chosen (the folder
// `chooser_choose` registered) or cancelled (null), so every caller of `pickFolder` still awaits a `PickedFolder | null`. One store per transport; the room's
// FolderChooserHost (room/folder-chooser-host.tsx) shows the request it holds. Kept small: it ships in the first load.
import type { ChooserStart, Go, Listing, PickedFolder, StartPoint } from "./protocol.ts";

/** One chooser session's calls: the desktop's `chooser_starts`, `chooser_list`, `chooser_choose` and
 * `chooser_cancel`, the session bound. `starts` lands once the desktop has read the drives (at most 1.5 s);
 * `choose` gives null when the lawyer declines the native confirm; the session stays open then. */
export type ChooserWire = {
  starts(): Promise<StartPoint[]>;
  list(go: Go): Promise<Listing>;
  choose(): Promise<PickedFolder | null>;
  cancel(): Promise<void>;
};

/** A session the page shows: the session, its calls, and how it ends. */
export type ChooserRequest = {
  start: ChooserStart;
  wire: ChooserWire;
  /** The lawyer chose this folder: the pick resolves with it. */
  chosen(folder: PickedFolder): void;
  /** The lawyer cancelled: the session is closed on the desktop, and the pick resolves null. */
  cancel(): void;
};

type Listener = () => void;

/** Whether any chooser is open in the page: the folder picker's line (room/picker-wait.ts) stands down meanwhile. */
const anyOpen = { count: 0, listeners: new Set<Listener>() };

export function chooserShowing(): boolean {
  return anyOpen.count > 0;
}

export function onChooserShowing(listener: Listener): () => void {
  anyOpen.listeners.add(listener);
  return () => anyOpen.listeners.delete(listener);
}

function showing(by: number) {
  anyOpen.count += by;
  for (const listener of anyOpen.listeners) listener();
}

/** One transport's chooser: the request on show, if any, and whether a host is there to show it. */
export class ChooserStore {
  #request: ChooserRequest | null = null;
  #hosts = 0;
  readonly #listeners = new Set<Listener>();

  /** The request on show; the same object until it ends. */
  current = (): ChooserRequest | null => this.#request;

  subscribe = (listener: Listener): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** A host is mounted; the returned function unmounts it. Without one, `run` cancels at once, and a session on show
   * when the last host goes (the room unmounted) is cancelled, unless a host mounts again at once (Strict Mode). */
  host(): () => void {
    this.#hosts += 1;
    return () => {
      this.#hosts -= 1;
      queueMicrotask(() => {
        if (this.#hosts === 0) this.#request?.cancel();
      });
    };
  }

  /** Shows the session and resolves with what the lawyer did there. A new session ends one still on show, as the
   * desktop closes the old one when it opens another. */
  run(start: ChooserStart, wire: ChooserWire): Promise<PickedFolder | null> {
    if (this.#hosts === 0) return wire.cancel().then(nothing, nothing);
    this.#request?.cancel();
    return new Promise((resolve) => {
      let done = false;
      const end = (folder: PickedFolder | null) => {
        if (done) return;
        done = true;
        if (this.#request === request) {
          this.#request = null;
          showing(-1);
          this.#tell();
        }
        resolve(folder);
      };
      const request: ChooserRequest = {
        start,
        wire,
        chosen: (folder) => end(folder),
        cancel: () => {
          end(null);
          // the desktop closes the session; an answer that it was closed already changes nothing
          wire.cancel().catch(nothing);
        },
      };
      this.#request = request;
      showing(1);
      this.#tell();
    });
  }

  #tell() {
    for (const listener of this.#listeners) listener();
  }
}

const nothing = (): null => null;

const stores = new WeakMap<object, ChooserStore>();

/** The chooser of `transport`: one per transport, made when first asked for. */
export function chooserOf(transport: object): ChooserStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = new ChooserStore();
    stores.set(transport, store);
  }
  return store;
}
