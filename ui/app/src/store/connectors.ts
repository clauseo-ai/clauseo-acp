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
//   useOutOfCredit(store, conversation)          the out-of-credit connector's id, unless hidden there
//   store.dropHints(matter), store.hide(key)     "Don't show again" (after its command), "Hide in this conversation"
//   store.turnStarted(thread, turn), turnEnded   the turn of a thread that runs, while it runs
//   useLowCredit(store, thread, turn)            whether the running-low line shows under that turn
import { useCallback, useSyncExternalStore } from "react";
import { nextHostOf, type AppEvent, type CaseLawHint } from "../room/host-next.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { ConnectorView, ConnectorsState, MatterId } from "../transport/protocol.ts";

/** The one connector at launch, Indian Kanoon through Clauseo's case-law service. */
export const CASE_LAW = "case-law";

/** Before any state: nothing listed, so nothing shows. */
export const NO_CONNECTORS: ConnectorsState = { available: false, locked: false, connectors: [] };

export type ConnectorsStore = {
  /** Hears each change; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
  /** The latest state: the same object until one that differs by value arrives. */
  state: () => ConnectorsState;
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

/** Whether a listed connector is out of credit while connectors are on: when the note shows. */
const outOf = (state: ConnectorsState): ConnectorView | undefined =>
  state.available
    ? state.connectors.find((c) => typeof c.status === "object" && "out_of_credit" in c.status)
    : undefined;

/** The case-law connector's credit while connectors are on: "low" while signed in with `low`, or out of credit, so
 * a top-up that leaves it low is no change; "ok" signed in and not low; null otherwise, which is no change either. */
const creditOf = (state: ConnectorsState): "low" | "ok" | null => {
  const c = state.available ? state.connectors.find((v) => v.id === CASE_LAW) : undefined;
  if (c === undefined || typeof c.status !== "object") return null;
  if ("out_of_credit" in c.status) return "low";
  if ("signed_in" in c.status) return c.status.signed_in.low ? "low" : "ok";
  return null;
};

/** Whether the running-low line may show: connectors on, the case-law connector signed in and low, and no out-of-credit
 * note, which comes first. */
const lowShows = (state: ConnectorsState) =>
  state.available &&
  outOf(state) === undefined &&
  state.connectors.some(
    (c) =>
      c.id === CASE_LAW &&
      typeof c.status === "object" &&
      "signed_in" in c.status &&
      c.status.signed_in.low,
  );

/** Whether a hint may show: connectors on, and one listed and not added. */
const hintsShow = (state: ConnectorsState) =>
  state.available && state.connectors.some((c) => c.status === "not_added");

/** A store over a transport's app feed; a transport with none (NullTransport) gives a store that never hears. */
export function createConnectorsStore(transport: HostTransport): ConnectorsStore {
  let state = NO_CONNECTORS;
  let key = "";
  let heard = false;
  const hints = new Map<string, CaseLawHint>();
  const hid = new Set<string>();
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
    if (outOf(next) === undefined) hid.clear();
    const now = creditOf(next);
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
    heard: () => heard,
    hint: (thread, turn) => hints.get(hintKey(thread, turn)),
    dropHints: (matter) => {
      for (const [k, hint] of hints) if (hint.matter === matter) hints.delete(k);
      tell();
    },
    hide: (conversation) => {
      if (outOf(state) === undefined || hid.has(conversation)) return;
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

/** The id of the connector out of credit, while connectors are on and `conversation` hasn't hidden the note; null
 * otherwise. */
export function useOutOfCredit(store: ConnectorsStore, conversation: string | null): string | null {
  const get = useCallback(() => {
    const out = outOf(store.state());
    if (out === undefined) return null;
    return conversation !== null && store.hidden(conversation) ? null : out.id;
  }, [store, conversation]);
  return useSyncExternalStore(store.subscribe, get, get);
}

/** Whether the running-low line shows under a thread's turn: `low` turned true while that turn ran, and it is still
 * low, with connectors on and no out-of-credit note. */
export function useLowCredit(store: ConnectorsStore, thread: string, turn: string | null): boolean {
  const get = useCallback(
    () => turn !== null && lowShows(store.state()) && store.lowTurn(thread) === turn,
    [store, thread, turn],
  );
  return useSyncExternalStore(store.subscribe, get, get);
}
