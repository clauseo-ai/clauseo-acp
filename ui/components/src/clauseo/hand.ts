// What the lawyer has in hand. One small store per room, so a tile, a row, a reference, the
// excerpt card and the provenance line each read only their own part and re-render alone: a press on a reference
// repaints its tile, not the room. Two rooms on one page each keep their own.
import { useSyncExternalStore } from "react";
import type { CitationRef } from "./matter.ts";

/** What a card speaks for: a paper or a judgment by its id. A paper put in hand from a reference carries the
 * citation the reference named; from its tile, none. The tile is in hand either way. */
export type Target = { kind: "paper" | "cite"; id: string; citation?: CitationRef | undefined };
/** Where the card was asked for: a reference in the answer, a tile on the wall, a citation row. */
export type Source = "ref" | "tile" | "row";

/** The excerpt card's whole state. pointed: shown for as long as the pointer or the focus stays. pinned: put in hand
 * by a click, a tap or Enter, until the same press, a press elsewhere, Escape or a scroll puts it down. */
export type Hand =
  | { state: "none" }
  | {
      state: "pointed" | "pinned";
      target: Target;
      source: Source;
      trigger: HTMLElement;
      keyboard: boolean;
    };

/** Who has the prepared draft in hand: the Ready card or the hint pointed at or focused, its tile pointed at, or the
 * page turned to it. Any one lifts its tile off the wall in full yellow. */
export type DraftHolder = "card" | "hint" | "tile" | "turn";

export type HandStore = {
  get: () => Hand;
  getDraft: () => ReadonlySet<DraftHolder>;
  /** Every change to what's in hand, for parts that draw from measured boxes outside React. */
  subscribe: (l: () => void) => () => void;
  /** Pointing at something that has a card: after 90 ms it shows, at once when a card is already up. */
  point: (trigger: HTMLElement, target: Target, source: Source) => void;
  /** Focus by the keyboard shows the card at once; focus put back by the card itself, after Escape, doesn't. */
  focusOn: (trigger: HTMLElement, target: Target, source: Source) => void;
  /** Puts the focus back on the words, leaving the card down until the focus moves on. */
  returnFocus: (trigger: HTMLElement) => void;
  /** The pointer or the focus leaving: the card goes 120 ms later, unless it's pinned or the pointer reaches it. */
  leave: () => void;
  /** The pointer reaching the card keeps it up. */
  stay: () => void;
  /** A click, a tap or Enter: in hand, in place. The same press again puts it down. keyboard: focus goes on into
   * the card, to Open. */
  press: (trigger: HTMLElement, target: Target, source: Source, keyboard: boolean) => void;
  /** Down: nothing in hand. */
  put: () => void;
  /** The draft in hand, or let go, by one holder. */
  holdDraft: (by: DraftHolder, on: boolean) => void;
  /** Stops both timers: the room is leaving the page. */
  dispose: () => void;
};

const NONE: Hand = { state: "none" };
/** The same paper or judgment, whichever citation of it. */
const same = (a: Target, b: Target) => a.kind === b.kind && a.id === b.id;

/** The words that opened a card, ready for the focus: the trigger itself, or the link or button inside it. */
export function focusableOf(trigger: HTMLElement): HTMLElement {
  if (trigger instanceof HTMLAnchorElement || trigger instanceof HTMLButtonElement) return trigger;
  const walk = trigger.ownerDocument.createTreeWalker(trigger, NodeFilter.SHOW_ELEMENT);
  for (let n = walk.nextNode(); n !== null; n = walk.nextNode()) {
    if (n instanceof HTMLAnchorElement || n instanceof HTMLButtonElement) return n;
  }
  return trigger;
}

export function createHand(): HandStore {
  let hand: Hand = NONE;
  let draft: ReadonlySet<DraftHolder> = new Set();
  let openTimer = 0;
  let closeTimer = 0;
  let quiet = false;
  const listeners = new Set<() => void>();
  const emit = () => {
    for (const l of listeners) l();
  };
  const set = (next: Hand) => {
    hand = next;
    emit();
  };
  const clear = () => {
    window.clearTimeout(openTimer);
    window.clearTimeout(closeTimer);
  };
  const put = () => {
    clear();
    if (hand.state !== "none") set(NONE);
  };
  return {
    get: () => hand,
    getDraft: () => draft,
    subscribe: (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    point: (trigger, target, source) => {
      clear();
      if (hand.state === "pinned") return;
      if (hand.state === "pointed" && hand.trigger === trigger) return;
      const show = () => set({ state: "pointed", target, source, trigger, keyboard: false });
      if (hand.state === "pointed") show();
      else openTimer = window.setTimeout(show, 90);
    },
    focusOn: (trigger, target, source) => {
      if (quiet) return;
      clear();
      if (hand.state !== "none" && hand.trigger === trigger) return;
      set({ state: "pointed", target, source, trigger, keyboard: false });
    },
    returnFocus: (trigger) => {
      quiet = true;
      try {
        focusableOf(trigger).focus({ preventScroll: true });
      } finally {
        quiet = false;
      }
    },
    leave: () => {
      window.clearTimeout(openTimer);
      if (hand.state !== "pointed") return;
      window.clearTimeout(closeTimer);
      closeTimer = window.setTimeout(put, 120);
    },
    stay: () => window.clearTimeout(closeTimer),
    press: (trigger, target, source, keyboard) => {
      clear();
      if (hand.state === "pinned" && hand.trigger === trigger) {
        put();
        return;
      }
      set({ state: "pinned", target, source, trigger, keyboard });
    },
    put,
    holdDraft: (by, on) => {
      if (draft.has(by) === on) return;
      const next = new Set(draft);
      if (on) next.add(by);
      else next.delete(by);
      draft = next;
      emit();
    },
    dispose: clear,
  };
}

/** The whole excerpt card state. */
export function useHandState(store: HandStore): Hand {
  return useSyncExternalStore(store.subscribe, store.get, store.get);
}

/** Whether this tile or row is in hand: its card is up, from anywhere. */
export function useInHand(store: HandStore, target: Target): boolean {
  const snap = () => {
    const h = store.get();
    return h.state !== "none" && same(h.target, target);
  };
  return useSyncExternalStore(store.subscribe, snap, snap);
}

/** Whether this tile or row has its card grown over it on the wall: on a wide window, whenever it's in hand. */
export function useCarded(store: HandStore, target: Target, onWall: boolean): boolean {
  const snap = () => {
    const h = store.get();
    return onWall && h.state !== "none" && same(h.target, target);
  };
  return useSyncExternalStore(store.subscribe, snap, snap);
}

/** Who holds the prepared draft. */
export function useDraftHolders(store: HandStore): ReadonlySet<DraftHolder> {
  return useSyncExternalStore(store.subscribe, store.getDraft, store.getDraft);
}
