// The Chrome download the browser row offers, app-wide: what the host says about installing Chrome for Clauseo's own
// browser, and the one call that starts it. None of it is the host's yet: the types live here, not in bindings.ts,
// only FakeTransport plays the reports (its `appEvent`, as `browser_install`) and takes the call (`installBrowser`),
// and the desktop transport does neither, so the real app has no state and shows no download.
//
// The planned host event says a failure as `Failed { message }`. The app shows words by code, never a host's message,
// so this model takes `failed { code }`, and the host is asked for a code in its place.
import { useSyncExternalStore } from "react";
import type { HostTransport } from "../transport/host-transport.ts";
import { nextHostOf, type AppEvent } from "./host-next.ts";

/** Why the download failed: no room (about 1 GB needed), a host the network refused, a size or signature that
 * didn't match, or anything else. */
export type InstallFailCode = "disk" | "refused" | "check" | "other";

/** What the host reports about installing Chrome: whether it can download it (and its size), or not at all (Windows
 * on ARM64, where the lawyer installs Google Chrome); then the download's own events. */
export type BrowserInstallReport =
  | { offer: "download"; bytes: number }
  | { offer: "none" }
  | { downloading: { bytes: number; total: number } }
  | "unpacking"
  | "done"
  | { failed: { code: InstallFailCode } };

/** Where a download stands, once one is asked for. */
export type InstallProgress =
  | { downloading: { bytes: number; total: number } }
  | "unpacking"
  | "done"
  | { failed: { code: InstallFailCode } };

/** The install as the browser row reads it: the offer the host last reported, and the download's progress. Null in
 * the store until the host says anything. */
export type BrowserInstall = {
  offer: { offer: "download"; bytes: number } | { offer: "none" } | null;
  progress: InstallProgress | null;
};

/** A transport whose host takes the install call. Only FakeTransport is one; asked for by duck type. */
export interface BrowserInstallSource {
  installBrowser(): Promise<void>;
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
    nextHostOf(transport).app?.subscribeApp((event) => this.#hear(event));
  }

  get = (): BrowserInstall | null => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#heard.add(listener);
    return () => this.#heard.delete(listener);
  };

  /** The lawyer's consent: Download Chrome or Try again. Asks the host once; nothing while a download runs. */
  install(): void {
    const state = this.#state;
    if (state === null || installRunning(state) || !installs(this.#transport)) return;
    const total = state.offer?.offer === "download" ? state.offer.bytes : 0;
    this.#set({ ...state, progress: { downloading: { bytes: 0, total } } });
    this.#transport.installBrowser().catch(() => {
      this.#set({ ...(this.#state ?? state), progress: { failed: { code: "other" } } });
    });
  }

  #hear(event: AppEvent): void {
    if (!("browser_install" in event)) return;
    const report = event.browser_install;
    const was = this.#state ?? { offer: null, progress: null };
    if (typeof report === "object" && "offer" in report) this.#set({ ...was, offer: report });
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
