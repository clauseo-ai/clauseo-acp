// The one place Skills and connectors reads from: the list (addons-list.ts, stage D's list_addons),
// and Clauseo Case Law's facts from the connectors' state (its credits and price, whether connectors
// are on), and the two things the place can ask of the host: its Retry (the list read again, which
// checks Case Law's reach again) and today's add flow, Settings at Connectors.
import { useSyncExternalStore } from "react";
import { quietly } from "../room/open.ts";
import { caseLawCredit, type CaseLawCredit } from "../store/case-law-credit.ts";
import { CASE_LAW, connectorsStoreOf } from "../store/connectors.ts";
import { balanceQuestions, packQuestions } from "../store/research-questions.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { ConnectorsState } from "../transport/protocol.ts";
import type { AddonView } from "./addons.ts";
import { addonsList } from "./addons-list.ts";

/** The list as read: null until the first answer; complete once every source has been heard. */
export type ListState = { items: readonly AddonView[] | null; complete: boolean };

/** A reader of the list. */
export type ListReader = {
  subscribe: (listener: () => void) => () => void;
  state: () => ListState;
  read: () => void;
};

/** Clauseo Case Law's facts beyond its list item, from the connectors' state. */
export type CaseLawFacts = {
  /** Whether the connectors' state has arrived; before it, the card waits. */
  heard: boolean;
  /** Connectors turned off: by the lawyer, or by the firm; null while on. */
  off: "lawyer" | "firm" | null;
  /** The smallest pack: its gross price in paise, its credits and what it buys in the server's words
   * (store/research-questions.ts); null while none is listed. */
  pack: { paise: number; credits: number; questions: string | null } | null;
  /** The credits left, from the last account read; null before one, or while not added. */
  credits: number | null;
  /** What the credits left buy, in the server's words; null when it sent none. */
  questions: string | null;
  /** Out of credit by the one rule (store/case-law-credit.ts), with the host's checked pay link when it made one;
   * null while a search goes through. */
  out: { payUrl: string | null } | null;
  /** Added once, but not signed in now: the sign-in ended or its key was revoked ("ended"), a
   * sign-in is under way ("signing_in"), the last one failed ("failed", also the host's answer while the saved
   * sign-in can't be read), or it couldn't be saved ("error"); null otherwise. */
  signIn: "ended" | "signing_in" | "failed" | "error" | null;
};

export type AddonsState = ListState & { caseLaw: CaseLawFacts };

/** Case Law's Retry: reads the list again, which checks its reach again. */
export type Recheck = { run: () => void };

export type AddonsSource = {
  subscribe: (listener: () => void) => () => void;
  state: () => AddonsState;
  /** Reads the list again: when the place opens. */
  read: () => void;
  /** Case Law's Retry; null where it can't be. */
  recheck: Recheck | null;
  /** Today's add flow and the connectors' switch: Settings at Connectors; null without it. */
  openConnectors: (() => void) | null;
};

const NO_FACTS: CaseLawFacts = {
  heard: false,
  off: null,
  pack: null,
  credits: null,
  questions: null,
  out: null,
  signIn: null,
};

/** Case Law's facts from the connectors' state, and its credit by the one rule (store/case-law-credit.ts). */
export function caseLawFacts(
  state: ConnectorsState,
  heard: boolean,
  credit: CaseLawCredit | null = caseLawCredit(state),
): CaseLawFacts {
  if (!heard) return NO_FACTS;
  const c = state.connectors.find((v) => v.id === CASE_LAW);
  const smallest = [...(c?.packs ?? [])].sort((a, b) => a.paise - b.paise)[0];
  const s = c?.status;
  const out = credit?.kind === "out" ? credit : null;
  const credits =
    out !== null
      ? out.credits
      : typeof s !== "object"
        ? null
        : "signed_in" in s
          ? (s.signed_in.balance?.credits ?? null)
          : null;
  const signIn =
    s === "signed_out" || s === "key_revoked"
      ? ("ended" as const)
      : s === "signing_in"
        ? ("signing_in" as const)
        : typeof s === "object" && "error" in s
          ? s.error.kind === "sign_in_failed"
            ? ("failed" as const)
            : ("error" as const)
          : null;
  return {
    heard,
    signIn,
    off: state.available ? null : state.locked ? "firm" : "lawyer",
    pack:
      smallest === undefined
        ? null
        : { paise: smallest.paise, credits: smallest.credits, questions: packQuestions(smallest) },
    credits,
    // the balance's words only beside a figure: never while out of credit
    questions:
      out === null && typeof s === "object" && "signed_in" in s
        ? balanceQuestions(s.signed_in.balance)
        : null,
    out: out === null ? null : { payUrl: out.payUrl },
  };
}

/** The source: a list reader, with Case Law's facts and the host's two asks. */
export function sourceOver(transport: HostTransport, list: ListReader): AddonsSource {
  const connectors = connectorsStoreOf(transport);
  let state: AddonsState = { ...list.state(), caseLaw: NO_FACTS };
  let key = "";
  const compose = () => {
    const facts = caseLawFacts(connectors.state(), connectors.heard(), connectors.credit());
    const next = list.state();
    const nextKey = JSON.stringify(facts);
    if (next.items === state.items && next.complete === state.complete && nextKey === key) return;
    key = nextKey;
    state = { ...next, caseLaw: facts };
  };
  compose();
  const open = transport.openSettings;
  const openConnectors =
    open === undefined
      ? null
      : () => void quietly("open_settings", open.call(transport, "connectors"));
  return {
    subscribe: (listener) => {
      const tell = () => {
        compose();
        listener();
      };
      const offList = list.subscribe(tell);
      const offConnectors = connectors.subscribe(tell);
      return () => {
        offList();
        offConnectors();
      };
    },
    state: () => {
      compose();
      return state;
    },
    read: list.read,
    recheck: { run: list.read },
    openConnectors,
  };
}

/** The list's source over a transport, listening from the moment it is made. */
export function createAddonsSource(transport: HostTransport): AddonsSource {
  return sourceOver(transport, addonsList(transport));
}

const sources = new WeakMap<HostTransport, AddonsSource>();

/** The transport's one source, made on first ask: the room asks at its start, so no catch-up goes
 * unheard. */
export function addonsSourceOf(transport: HostTransport): AddonsSource {
  let source = sources.get(transport);
  if (source === undefined) {
    source = createAddonsSource(transport);
    sources.set(transport, source);
  }
  return source;
}

/** Tests and the copy review: `source` stands in for the transport's own, given before the room
 * mounts. */
export function provideAddons(transport: HostTransport, source: AddonsSource): void {
  sources.set(transport, source);
}

/** A source that lists `items` with `caseLaw`'s facts and never changes: the review page's and the
 * shots'. */
export function fixedAddons(
  items: readonly AddonView[],
  caseLaw: CaseLawFacts = {
    heard: true,
    off: null,
    pack: null,
    credits: null,
    questions: null,
    out: null,
    signIn: null,
  },
): AddonsSource {
  const state: AddonsState = { items, complete: true, caseLaw };
  return {
    subscribe: () => () => {},
    state: () => state,
    read: () => {},
    recheck: { run: () => {} },
    openConnectors: () => {},
  };
}

/** The list as it changes. */
export function useAddons(source: AddonsSource): AddonsState {
  return useSyncExternalStore(source.subscribe, source.state, source.state);
}
