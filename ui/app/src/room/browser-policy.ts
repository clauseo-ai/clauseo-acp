// The re-read of the firm's browser policy the browser row offers when Clauseo couldn't check it, app-wide: the one call
// that asks for it (`recheck_browser_policy`, sent through dispatch) and the host's `browser_policy` events, which say
// what the read found: the host answers `done` at once, its `checking` and then `unreadable`, `blocked` or `clear`
// reach every window, and the latest is in every catch-up. A running conversation keeps the browser it started with:
// after `clear` the next conversation gets it, and the row says so (Ready for new conversations). On `blocked` the host re-words the header itself (`browser_blocked_by_policy`).
// A second ask while a read runs is joined by the host, so the row asks once and waits.
import type { BrowserPolicyState } from "../transport/protocol.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { nextHostOf, type AppEvent } from "./host-next.ts";

/** What the last read found, as the row reads it; null before any. */
export type BrowserPolicy = { state: BrowserPolicyState | null };

const STATES: ReadonlySet<string> = new Set<BrowserPolicyState>([
  "checking",
  "unreadable",
  "blocked",
  "clear",
]);

/** The re-read's state for one transport, heard from its app events. */
export class BrowserPolicyStore {
  #state: BrowserPolicy = { state: null };
  readonly #heard = new Set<() => void>();
  readonly #transport: HostTransport;

  constructor(transport: HostTransport) {
    this.#transport = transport;
    const app = nextHostOf(transport).app;
    if (!app) return;
    // the app's first frames, the catch-up, carry the latest state when there is one
    app.subscribeApp((event) => this.#hear(event));
  }

  get = (): BrowserPolicy => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#heard.add(listener);
    return () => this.#heard.delete(listener);
  };

  /** Try again: asks the host once and waits; nothing while a read runs. A refused command is no answer: the row goes
   * back to where it stood before the press, unless the host's events have said where it stands since. */
  recheck(): void {
    const before = this.#state;
    if (before.state === "checking") return;
    const pressed: BrowserPolicy = { state: "checking" };
    this.#set(pressed);
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { recheck_browser_policy: {} } })
      .catch(() => {
        if (this.#state === pressed) this.#set(before);
      });
  }

  #hear(event: AppEvent): void {
    if (!("browser_policy" in event)) return;
    const state: unknown = event.browser_policy.state;
    if (typeof state === "string" && STATES.has(state))
      this.#set({ state: state as BrowserPolicyState });
  }

  #set(state: BrowserPolicy): void {
    this.#state = state;
    for (const listener of this.#heard) listener();
  }
}

const stores = new WeakMap<HostTransport, BrowserPolicyStore>();

/** The one policy store of a transport, made (and listening) on first ask. */
export function browserPolicyOf(transport: HostTransport): BrowserPolicyStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = new BrowserPolicyStore(transport);
    stores.set(transport, store);
  }
  return store;
}
