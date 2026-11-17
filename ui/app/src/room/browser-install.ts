// Clauseo's own browser (Chrome for Testing), which the browser row offers on a Mac (and Linux, for development) with
// no Chromium browser on it, app-wide: the two calls that start and stop the download (`install_browser`,
// `cancel_browser_install`, sent through dispatch; the host answers `done` to each at once) and the host's
// `browser_install` events, which say where it stands; the latest is in every catch-up. A machine before any download
// hears nothing, so the row offers the download until a state says otherwise.
//
// A failure comes as `failed { code, message }`: the row reads only `code`. `firm_forbids`, the firm's policy, is the
// one after which nothing is offered. `tls`, `host_refused` (a refused address), `server_refused` (an HTTP error from
// the server), `disk_space` and `network` (the host's retries ran out) are the offer again, with a line saying why.
// Every other failure, `cancelled` included, is the offer again, with no words of its own. `message` is the host's,
// for the log only, never on screen. A second install while one runs is answered `done` by the host and starts nothing.
import type { BrowserInstallFailure, BrowserInstallState } from "../transport/protocol.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { nextHostOf, type AppEvent } from "./host-next.ts";

/** Where the download stands, as the row shows it: under way (`behind`: the copy on the computer is more than one
 * security release behind, so this is its update), unpacking, waiting for the network, the firm's policy forbidding
 * it, or done. The failures with a line of their own (`tls`, `host_refused`, `server_refused`, `disk_space`,
 * `network`) are the offer with that line above it. Null is the offer: no download yet, or the last one cancelled or
 * failed with no line. */
export type InstallProgress =
  | { downloading: { bytes: number; total: number; behind: boolean } }
  | "unpacking"
  | "offline"
  | "forbidden"
  | "tls"
  | "host_refused"
  | "server_refused"
  | "disk_space"
  | "network"
  | "done";

/** The download as the row reads it: where it stands; whether the lawyer pressed the offer and the host has not said
 * yet; and the last download's size and whether it was an update, which Unpacking still shows. */
export type BrowserInstall = {
  progress: InstallProgress | null;
  pressed: boolean;
  total: number;
  behind: boolean;
};

const IDLE: BrowserInstall = { progress: null, pressed: false, total: 0, behind: false };

const num = (n: unknown): number => (typeof n === "number" && Number.isFinite(n) && n > 0 ? n : 0);

/** A failure's progress: the firm's policy, the failures with a line of their own, or null (the plain offer, for every
 * other code, and a code from a newer host). */
function failureOf(code: BrowserInstallFailure): InstallProgress | null {
  switch (code) {
    case "firm_forbids":
      return "forbidden";
    case "tls":
    case "host_refused":
    case "server_refused":
    case "disk_space":
    case "network":
      return code;
    default:
      return null;
  }
}

/** The host's state as the row reads it; null for the plain offer (a failure with no line, and a state this version
 * doesn't know). */
export function progressOf(state: BrowserInstallState): InstallProgress | null {
  if (state === "unpacking" || state === "done" || state === "offline") return state;
  if (typeof state !== "object" || state === null) return null;
  if ("downloading" in state && state.downloading !== undefined) {
    const { bytes, total, behind } = state.downloading;
    return { downloading: { bytes: num(bytes), total: num(total), behind: behind === true } };
  }
  if ("failed" in state && state.failed !== undefined) return failureOf(state.failed.code);
  return null;
}

/** Whether a download is under way or waiting for the network, so the row offers Cancel and no second start. */
export const installRunning = (state: BrowserInstall): boolean => {
  const p = state.progress;
  return p === "unpacking" || p === "offline" || (typeof p === "object" && p !== null);
};

/** The download's state for one transport, heard from its app events: the offer until one says otherwise. */
export class BrowserInstallStore {
  #state: BrowserInstall = IDLE;
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

  /** The lawyer's consent: Get a browser for Clauseo. Asks the host once; nothing while a download runs or a press
   * waits for its answer. A refused command is no failure: the offer stands again. */
  install(): void {
    const before = this.#state;
    if (before.pressed || installRunning(before) || before.progress === "forbidden") return;
    const pressed: BrowserInstall = { ...before, pressed: true };
    this.#set(pressed);
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { install_browser: {} } })
      .catch(() => {
        if (this.#state === pressed) this.#set(before);
      });
  }

  /** Cancel: asks the host to stop; its `failed { cancelled }` brings the offer back. */
  cancel(): void {
    if (!installRunning(this.#state)) return;
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { cancel_browser_install: {} } })
      .catch(() => {});
  }

  #hear(event: AppEvent): void {
    if (!("browser_install" in event)) return;
    const progress = progressOf(event.browser_install.state);
    const now = this.#state;
    const down = typeof progress === "object" && progress !== null ? progress.downloading : null;
    this.#set({
      progress,
      pressed: false,
      // Unpacking keeps the size and the update's words of the download before it
      total: down !== null ? down.total : progress === "unpacking" ? now.total : 0,
      behind: down !== null ? down.behind : progress === "unpacking" ? now.behind : false,
    });
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
