// The Chrome download the browser row offers, app-wide: the one call that starts it (`install_browser`, sent through
// dispatch) and the host's `browser_install` events, which say where it stands. The host's events carry no offer: a
// machine with no download (Linux, Windows on ARM64) says so by `failed` with `unsupported`, and a machine before any
// download hears nothing, so the row offers Download Chrome until a state says otherwise.
//
// A failure comes as `failed { code, message }`: the app shows words by `code`, an open enum it reads as `unknown` when
// it doesn't know the value, and `message` is the host's, for the log only, never on screen. `cancelled` is the answer
// to a cancel, not a failure: the row goes back to the offer, with no line. A second install while one runs is answered
// `done` by the host and starts nothing; the running download's events already say where it stands.
import { useSyncExternalStore } from "react";
import type { BrowserInstallState } from "../transport/protocol.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { nextHostOf, type AppEvent } from "./host-next.ts";

/** Why the download failed: the connection dropped, a size or signature that didn't match, under about 1 GB free, a
 * host the network refused, an archive that wouldn't unpack, a computer that can't download it (no system curl,
 * ditto or tar, or Windows on ARM64), a download stopped; `unknown` for any value this version doesn't know. */
export type InstallFailCode =
  | "network"
  | "size_mismatch"
  | "disk_space"
  | "host_refused"
  | "signature"
  | "unpack"
  | "unsupported"
  | "cancelled"
  | "unknown";

const FAIL_CODES: ReadonlySet<string> = new Set<InstallFailCode>([
  "network",
  "size_mismatch",
  "disk_space",
  "host_refused",
  "signature",
  "unpack",
  "unsupported",
  "cancelled",
]);

/** A failure the screen shows: its code has words (never `cancelled`, which is no failure), and the host's message,
 * for the log only. */
export type InstallFailure = {
  code: Exclude<InstallFailCode, "cancelled">;
  message: string;
};

/** The host's code, read: one this version doesn't know is `unknown`. */
export const failCodeOf = (code: unknown): InstallFailCode =>
  typeof code === "string" && FAIL_CODES.has(code) ? (code as InstallFailCode) : "unknown";

/** Where a download stands, as the row shows it: a download under way, unpacking, done, or failed. Null (no value)
 * is the offer: no download has started, or the last one was cancelled. */
export type InstallProgress =
  | { downloading: { bytes: number; total: number } }
  | "unpacking"
  | "done"
  | { failed: InstallFailure };

/** The install as the browser row reads it. */
export type BrowserInstall = { progress: InstallProgress | null };

/** The host's state as the row reads it: a cancelled download is idle (the offer), not a failure. */
export function progressOf(state: BrowserInstallState): InstallProgress | null {
  if (state === "unpacking" || state === "done") return state;
  if ("downloading" in state && state.downloading !== undefined) {
    const { bytes, total } = state.downloading;
    return { downloading: { bytes, total } };
  }
  if ("failed" in state && state.failed !== undefined) {
    const code = failCodeOf(state.failed.code);
    if (code === "cancelled") return null;
    return {
      failed: {
        code,
        message: typeof state.failed.message === "string" ? state.failed.message : "",
      },
    };
  }
  return null;
}

/** Whether the download is under way, so a second can't start. */
export const installRunning = (state: BrowserInstall): boolean => {
  const p = state.progress;
  return p === "unpacking" || (p !== null && typeof p === "object" && "downloading" in p);
};

/** The install's state for one transport, heard from its app events: the offer until one says otherwise. */
export class BrowserInstallStore {
  #state: BrowserInstall = { progress: null };
  readonly #heard = new Set<() => void>();
  readonly #transport: HostTransport;

  constructor(transport: HostTransport) {
    this.#transport = transport;
    const app = nextHostOf(transport).app;
    if (!app) return;
    // the app's first frames, the catch-up, carry the latest state when there is one
    app.subscribeApp((event) => this.#hear(event));
  }

  get = (): BrowserInstall => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#heard.add(listener);
    return () => this.#heard.delete(listener);
  };

  /** The lawyer's consent: Download Chrome or Try again. Asks the host once; nothing while a download runs. Until the
   * host's first Downloading the total is not known, so the bar reads empty. A refused command is no failure: the row
   * goes back to where it stood before the press, unless the host's events have said where it stands since. */
  install(): void {
    const state = this.#state;
    if (installRunning(state)) return;
    const pressed: InstallProgress = { downloading: { bytes: 0, total: 0 } };
    this.#set({ progress: pressed });
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { install_browser: {} } })
      .catch(() => {
        const now = this.#state;
        if (now.progress === pressed) this.#set({ progress: state.progress });
      });
  }

  #hear(event: AppEvent): void {
    if (!("browser_install" in event)) return;
    this.#set({ progress: progressOf(event.browser_install.state) });
  }

  #set(state: BrowserInstall): void {
    this.#state = state;
    for (const listener of this.#heard) listener();
  }
}

const stores = new WeakMap<HostTransport, BrowserInstallStore>();

/** The one install store of a transport, made (and listening) on first ask. */
export function browserInstallOf(transport: HostTransport): BrowserInstallStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = new BrowserInstallStore(transport);
    stores.set(transport, store);
  }
  return store;
}

/** The install's state as it changes. */
export function useBrowserInstall(store: BrowserInstallStore): BrowserInstall {
  return useSyncExternalStore(store.subscribe, store.get, store.get);
}
