// The connectors' state in a window (U15): the latest `ConnectorsState` from the app feed's `connectors_changed`
// (R24), which the host sends whenever it changes and again in every catch-up, so a window that subscribes, or
// subscribes again after its channel reopened, hears the state at once. One store per transport, for either window:
// the settings window's Connectors section reads it, and so do the room's case-law hint and out-of-credit note. The
// store sends nothing: each window's commands live with that window (settings/connector-commands.ts for Settings,
// room/connectors/main-commands.ts for the room), so the room has no way here to send a settings-only command. No
// timers: the host publishes, the store hears.
//
// The room's part, in memory for the window's run only: the hints heard (`case_law_hint`, never logged, so a restart
// or a reopened window has none), by thread and turn, dropped when dismissed, when a state lists the connector added,
// or when `available` turns false; and the conversations that hid the out-of-credit note, emptied whenever no listed
// connector is out of credit, so a hidden note stays hidden until credit returns and runs out again. And the
// running-low line under a turn: the thread list says which turn of each open thread runs (turnStarted, turnEnded),
// and when the case-law connector's `low` goes from false to true, each turn running then keeps the line; it goes for
// all once `low` is false again. Low already when a turn starts, or before the first state, is no change: no line.
//
//   const store = connectorsStoreOf(transport)   one store per transport, listening from the first ask
//   useConnectors(store)                         the state; NO_CONNECTORS until the first one arrives
//   useConnector(store, id)                      one connector's view, undefined while it isn't listed
//   store.heard()                                whether any state has arrived yet
//   useCaseLawHint(store, thread, turn)          the hint for that turn, while the state lets it show
//   useOutOfCredit(store, conversation)          Case Law's credit while out (case-law-credit.ts), unless hidden there
//   useCaseLawCredit(store)                      Case Law's credit by the one rule, for every place that shows it
//   store.dropHints(matter), store.hide(key)     "Don't show again" (after its command), "Hide in this conversation"
//   store.turnStarted(thread, turn), turnEnded   the turn of a thread that runs, while it runs
//   useLowCredit(store, thread, turn)            whether the running-low line shows under that turn
//   useLowBeforeSend(store, thread)              whether the same line shows by the composer, before sending
//   useCaseLawOffer(store, listed)               whether the home card offering Indian Kanoon shows
import { useCallback, useSyncExternalStore } from "react";
import { nextHostOf, type AppEvent, type CaseLawHint } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { ConnectorView, ConnectorsState, MatterId } from "../transport/protocol.ts";
import { CASE_LAW, caseLawCredit, sameCredit, type CaseLawCredit } from "./case-law-credit.ts";

export { CASE_LAW } from "./case-law-credit.ts";

/** Before any state: nothing listed, so nothing shows. */
export const NO_CONNECTORS: ConnectorsState = { available: false, locked: false, connectors: [] };

export type ConnectorsStore = {
  /** Hears each change; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
  /** The latest state: the same object until one that differs by value arrives. */
  state: () => ConnectorsState;
  /** Clauseo Case Law's credit by the one rule (case-law-credit.ts): the same object until it says otherwise. */
  credit: () => CaseLawCredit | null;
  /** Whether a state has arrived since the store was made. */
  heard: () => boolean;
  /** The hint heard for a thread's turn, whatever the state says. */
  hint: (thread: string, turn: string) => CaseLawHint | undefined;
  /** Drops every hint of a matter: "Don't show again". */
  dropHints: (matter: MatterId) => void;
  /** Hides the out-of-credit note in a conversation until credit returns and runs out again. */
  hide: (conversation: string) => void;
  /** Whether a conversation hid the note. */
  hidden: (conversation: string) => boolean;
  /** A thread's turn started running, or was seen running; one per thread. */
  turnStarted: (thread: string, turn: string) => void;
  /** That turn ended, or is no longer watched. */
  turnEnded: (thread: string, turn: string) => void;
  /** The thread's turn that was running when `low` turned true, while it stays true. */
  lowTurn: (thread: string) => string | undefined;
};

const hintKey = (thread: string, turn: string) => `${thread} ${turn}`;

/** The credit when it is out (case-law-credit.ts's rule): when the out-of-credit note shows. */
export type OutCredit = Extract<CaseLawCredit, { kind: "out" }>;
const outOf = (credit: CaseLawCredit | null): OutCredit | null =>
  credit?.kind === "out" ? credit : null;

/** For the running-low line: "low" while low or out, so a top-up that leaves it low is no change; "ok" signed in with
 * plenty; null otherwise, which is no change either. */
const creditOf = (credit: CaseLawCredit | null): "low" | "ok" | null =>
  credit === null ? null : credit.kind === "ok" ? "ok" : "low";

/** Whether the running-low line may show: low by the host's mark with a search's worth left; out comes first. */
const lowShows = (credit: CaseLawCredit | null) => credit?.kind === "low";

/** Whether a hint may show: connectors on, and one listed and not added. */
const hintsShow = (state: ConnectorsState) =>
  state.available && state.connectors.some((c) => c.status === "not_added");

/** Case Law's status in Skills and connectors' list (list_addons), as the home card reads it: null
 * until the list has answered. */
export type CaseLawListed = "needs_setup" | "unreachable" | "other" | null;

/** Whether the home card offering Clauseo Case Law shows: connectors on, the case-law connector not
 * added, and the list saying it can be set up ("needs_setup", which is also "not yet checked") or
 * that it had no answer. Not offered (not live yet, or no connectors on this install) gives no card,
 * and nor does a list not yet read, so the card never flashes. Not listed in the connectors' state
 * counts as not added. Signing in, signed in, out of credit, signed out, a refused key or an error
 * all mean it was added or is being added, and Settings' words say what is wrong, so no card. The one
 * place a dismissal of the card would join. */
const offerShows = (state: ConnectorsState, listed: CaseLawListed) =>
  (listed === "needs_setup" || listed === "unreachable") &&
  state.available &&
  !state.connectors.some((c) => c.id === CASE_LAW && c.status !== "not_added");

/** A store over a transport's app feed; a transport with none (NullTransport) gives a store that never hears. */
export function createConnectorsStore(transport: HostTransport): ConnectorsStore {
  let state = NO_CONNECTORS;
  let key = "";
  let heard = false;
  const hints = new Map<string, CaseLawHint>();
  const hid = new Set<string>();
  // the credit by the one rule
  let shown: CaseLawCredit | null = null;
  // the running-low line: the credit last heard, each thread's running turn, and the turns that keep the line
  let credit: "low" | "ok" | null = null;
  const running = new Map<string, string>();
  const lows = new Map<string, string>();
  const listeners = new Set<() => void>();
  const tell = () => {
    for (const listener of listeners) listener();
  };
  const hear = (event: AppEvent) => {
    if ("case_law_hint" in event) {
      const hint = event.case_law_hint;
      hints.set(hintKey(hint.thread, hint.turn), hint);
      tell();
      return;
    }
    if (!("connectors_changed" in event)) return;
    const next = event.connectors_changed;
    const nextKey = JSON.stringify(next);
    const first = !heard;
    heard = true;
    // a repeat (the catch-up after a reconnect, a balance read that changed nothing) keeps the object
    if (nextKey === key && !first) return;
    key = nextKey;
    state = next;
    // a hint goes once the connector is added, or connectors are off; a hidden note shows again after credit returns
    if (!next.available || next.connectors.some((c) => c.status !== "not_added")) hints.clear();
    const nextCredit = caseLawCredit(next);
    if (!sameCredit(shown, nextCredit)) shown = nextCredit;
    if (outOf(shown) === null) hid.clear();
    const now = creditOf(shown);
    if (now === "low" && credit === "ok")
      for (const [thread, turn] of running) lows.set(thread, turn);
    if (now !== "low") lows.clear();
    credit = now;
    tell();
  };
  nextHostOf(transport).app?.subscribeApp(hear);
  return {
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    state: () => state,
    credit: () => shown,
    heard: () => heard,
    hint: (thread, turn) => hints.get(hintKey(thread, turn)),
    dropHints: (matter) => {
      for (const [k, hint] of hints) if (hint.matter === matter) hints.delete(k);
      tell();
    },
    hide: (conversation) => {
      if (outOf(shown) === null || hid.has(conversation)) return;
      hid.add(conversation);
      tell();
    },
    hidden: (conversation) => hid.has(conversation),
    turnStarted: (thread, turn) => {
      running.set(thread, turn);
    },
    turnEnded: (thread, turn) => {
      if (running.get(thread) === turn) running.delete(thread);
    },
    lowTurn: (thread) => lows.get(thread),
  };
}

const stores = new WeakMap<HostTransport, ConnectorsStore>();

/** The transport's one connectors store, made (and listening) on first ask. */
export function connectorsStoreOf(transport: HostTransport): ConnectorsStore {
  let store = stores.get(transport);
  if (store === undefined) {
    store = createConnectorsStore(transport);
    stores.set(transport, store);
  }
  return store;
}

/** The state as it changes. */
export function useConnectors(store: ConnectorsStore): ConnectorsState {
  return useSyncExternalStore(store.subscribe, store.state, store.state);
}

/** Whether a state has arrived, as it changes. */
export function useConnectorsHeard(store: ConnectorsStore): boolean {
  return useSyncExternalStore(store.subscribe, store.heard, store.heard);
}

/** One connector's view while the state lists it. */
export function useConnector(store: ConnectorsStore, id: string): ConnectorView | undefined {
  const get = useCallback(() => store.state().connectors.find((c) => c.id === id), [store, id]);
  return useSyncExternalStore(store.subscribe, get, get);
}

/** The hint under a thread's turn while connectors are on and the connector isn't added; undefined otherwise. */
export function useCaseLawHint(
  store: ConnectorsStore,
  thread: string,
  turn: string | null,
): CaseLawHint | undefined {
  const get = useCallback(
    () => (turn !== null && hintsShow(store.state()) ? store.hint(thread, turn) : undefined),
    [store, thread, turn],
  );
  return useSyncExternalStore(store.subscribe, get, get);
}

/** Clauseo Case Law's credit while it is out (case-law-credit.ts) and `conversation` hasn't hidden the note; null
 * otherwise. */
export function useOutOfCredit(
  store: ConnectorsStore,
  conversation: string | null,
): OutCredit | null {
  const get = useCallback(() => {
    const out = outOf(store.credit());
    if (out === null) return null;
    return conversation !== null && store.hidden(conversation) ? null : out;
  }, [store, conversation]);
  return useSyncExternalStore(store.subscribe, get, get);
}

/** Whether the running-low line shows under a thread's turn: `low` turned true while that turn ran, and it is still
 * low, with connectors on and no out-of-credit note. */
export function useLowCredit(store: ConnectorsStore, thread: string, turn: string | null): boolean {
  const get = useCallback(
    () => turn !== null && lowShows(store.credit()) && store.lowTurn(thread) === turn,
    [store, thread, turn],
  );
  return useSyncExternalStore(store.subscribe, get, get);
}

/** Whether the home card offering Indian Kanoon shows, as each `connectors_changed` arrives: it goes the moment a
 * state lists the connector added, with no restart. */
export function useCaseLawOffer(store: ConnectorsStore, listed: CaseLawListed): boolean {
  const get = useCallback(() => offerShows(store.state(), listed), [store, listed]);
  return useSyncExternalStore(store.subscribe, get, get);
}

/** Whether the running-low line shows by the composer, before a question is sent (24 Nov ruling): the case-law
 * connector low by the host's flag, connectors on and no out-of-credit note, unless the line already shows under a
 * turn of `thread` (null for a draft, which has none). */
export function useLowBeforeSend(store: ConnectorsStore, thread: string | null): boolean {
  const get = useCallback(
    () => lowShows(store.credit()) && (thread === null || store.lowTurn(thread) === undefined),
    [store, thread],
  );
  return useSyncExternalStore(store.subscribe, get, get);
}

/** Clauseo Case Law's credit by the one rule, as it changes: what Settings, the library and the composer all say. */
export function useCaseLawCredit(store: ConnectorsStore): CaseLawCredit | null {
  return useSyncExternalStore(store.subscribe, store.credit, store.credit);
}
