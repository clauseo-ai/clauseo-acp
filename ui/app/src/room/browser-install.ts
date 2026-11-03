// The Chrome download the browser row offers, app-wide: what the host says about installing Chrome for Clauseo's own
// browser, and the one call that starts it. None of it is the host's yet: the types live here, not in bindings.ts,
// only FakeTransport plays the reports (its `appEvent`, as `browser_install`) and takes the call (`installBrowser`),
// and the desktop transport does neither, so the real app has no state and shows no download.
//
// A failure comes as `Failed { code, message }` (stage E): the app shows words by `code`, an open enum it reads as
// `unknown` when it doesn't know the value, and `message` is the host's, for the log only, never on screen. A second
// InstallBrowser while one runs is refused as a command, not reported as a failure.
import { useSyncExternalStore } from "react";
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

/** A failure as the app keeps it: the code it has words for, and the host's message, for the log only. */
export type InstallFailure = { code: InstallFailCode; message: string };

/** The host's code, read: one this version doesn't know is `unknown`. */
export const failCodeOf = (code: unknown): InstallFailCode =>
  typeof code === "string" && FAIL_CODES.has(code) ? (code as InstallFailCode) : "unknown";

/** What the host reports about installing Chrome: whether it can download it (and its size), or not at all (Windows
 * on ARM64, where the lawyer installs Google Chrome); then the download's own events. */
export type BrowserInstallReport =
  | { offer: "download"; bytes: number }
  | { offer: "none" }
  | { downloading: { bytes: number; total: number } }
  | "unpacking"
  | "done"
  | { failed: { code: string; message: string } };

/** Where a download stands, once one is asked for. */
export type InstallProgress =
  | { downloading: { bytes: number; total: number } }
  | "unpacking"
  | "done"
  | { failed: InstallFailure };

/** The install as the browser row reads it: the offer the host last reported, and the download's progress. Null in
 * the store until the host says anything. */
export type BrowserInstall = {
  offer: { offer: "download"; bytes: number } | { offer: "none" } | null;
  progress: InstallProgress | null;
};

/** A transport whose host takes the install call. Only FakeTransport is one; asked for by duck type. */
export interface BrowserInstallSource {
  installBrowser(): Promise<void>;
  /** The reports played before this store was made, which it reads first: the room makes it only once the tool
   * line's chunk is in (tool-line-load.ts), after the reports a thread's first view brought. */
  installReports?(): readonly BrowserInstallReport[];
}

const installs = (transport: object): transport is BrowserInstallSource =>
  typeof (transport as Partial<BrowserInstallSource>).installBrowser === "function";

/** Whether the download is under way, so a second can't start. */
export const installRunning = (state: BrowserInstall | null): boolean => {
  const p = state?.progress ?? null;
  return p === "unpacking" || (typeof p === "object" && p !== null && "downloading" in p);
};

/** The install's state for one transport, heard from its app events. */
export class BrowserInstallStore {
  #state: BrowserInstall | null = null;
  readonly #heard = new Set<() => void>();
  readonly #transport: HostTransport;

  constructor(transport: HostTransport) {
    this.#transport = transport;
    const app = nextHostOf(transport).app;
    if (!app) return;
    const played = (transport as Partial<BrowserInstallSource>).installReports?.() ?? [];
    for (const report of played) this.#hear({ browser_install: report });
    app.subscribeApp((event) => this.#hear(event));
  }

  get = (): BrowserInstall | null => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#heard.add(listener);
    return () => this.#heard.delete(listener);
  };

  /** The lawyer's consent: Download Chrome or Try again. Asks the host once; nothing while a download runs. A
   * refused command (one already runs, as another window may have started it) is no failure: the row goes back to
   * where it stood before the press, unless the host's events have said where it stands since. */
  install(): void {
    const state = this.#state;
    if (state === null || installRunning(state) || !installs(this.#transport)) return;
    const total = state.offer?.offer === "download" ? state.offer.bytes : 0;
    const pressed: InstallProgress = { downloading: { bytes: 0, total } };
    this.#set({ ...state, progress: pressed });
    this.#transport.installBrowser().catch(() => {
      const now = this.#state;
      if (now !== null && now.progress === pressed) this.#set({ ...now, progress: state.progress });
    });
  }

  #hear(event: AppEvent): void {
    if (!("browser_install" in event)) return;
    const report = event.browser_install;
    const was = this.#state ?? { offer: null, progress: null };
    if (typeof report === "object" && "offer" in report) this.#set({ ...was, offer: report });
    else if (typeof report === "object" && "failed" in report)
      this.#set({
        ...was,
        progress: {
          failed: {
            code: failCodeOf(report.failed.code),
            message: typeof report.failed.message === "string" ? report.failed.message : "",
          },
        },
      });
    else this.#set({ ...was, progress: report });
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

/** The install's state as it changes; null until the host reports one. */
export function useBrowserInstall(store: BrowserInstallStore): BrowserInstall | null {
  return useSyncExternalStore(store.subscribe, store.get, store.get);
}
