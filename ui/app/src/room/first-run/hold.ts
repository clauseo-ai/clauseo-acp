// What the first run asks of the composer, in the first load: the only first-run code there. The first run's lazy
// chunk (slot.tsx) sets the hold; the composer reads it (thread/work-composer.tsx): the gate in the field's row while
// the conversation's plan isn't signed in, the install's working line, and the room's line under the field. Until the
// chunk sets one there is none, so a lawyer whose plan is signed in sends without waiting for the chunk, and a host
// whose setup events have not arrived never holds a question back.
import { useSyncExternalStore, type ReactNode } from "react";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import type { NoticeCard } from "@clauseo/components/clauseo/limit-note.tsx";

export type Hold = { gate?: ReactNode; working?: WorkingState | null; line?: ReactNode };

/** `read` gives the hold for the setup store's state now (the first run sets it, null before); `heard` hears each change,
 * the store's own emits forwarded, so the composer renders in the same commit as the first run. */
type Holder = { read: () => Hold | null; heard: Set<() => void> };

const holders = new WeakMap<object, Holder>();

/** The transport's one holder, made on first ask. */
export function holderOf(transport: object): Holder {
  let holder = holders.get(transport);
  if (holder === undefined)
    holders.set(transport, (holder = { read: () => null, heard: new Set() }));
  return holder;
}

/** The hold the composer stands under now; null with none, or with no transport. Left out of the compiler's
 * memoising ("use no memo"): it is a few bytes of the first load, and its subscription is cheap to make again. */
export function useHold(transport: object | null): Hold | null {
  "use no memo";
  // with no transport (a composer mounted alone), a holder no first run ever sets
  const holder = holderOf(transport ?? holders);
  const read = () => holder.read();
  return useSyncExternalStore(
    (heard) => {
      holder.heard.add(heard);
      return () => holder.heard.delete(heard);
    },
    read,
    read,
  );
}

/** Whether the transport carries the setup wire (store/setup.ts's SetupSource): only then does the first run load. */
export const hasSetup = (transport: object): boolean =>
  typeof (transport as { onSetup?: unknown }).onSetup === "function";

/** UD2's expired sign-in card, which the first run's chunk gives as it loads (it holds the setup store and the gate the
 * card shows), so the thread's chunk, which places it, shares no chunk with the first run's; null before. */
export type NoticeFrame = typeof NoticeCard;
export type ExpiredCardProps = {
  transport: object;
  agent: string;
  failedAt: number;
  /** The note's frame, the thread chunk's NoticeCard: the card's own chunk shares none of the thread's. */
  frame: NoticeFrame;
};
/** Makes the card's element: the component itself stays the first run's. */
export type ExpiredCardOf = (props: ExpiredCardProps) => ReactNode;
let expiredCard: ExpiredCardOf | null = null;
const cardHeard = new Set<() => void>();

export function provideExpiredCard(card: ExpiredCardOf): void {
  expiredCard = card;
  for (const heard of cardHeard) heard();
}

const readCard = () => expiredCard;
const hearCard = (heard: () => void) => {
  cardHeard.add(heard);
  return () => void cardHeard.delete(heard);
};

/** The card, once the first run's chunk is in. */
export const useExpiredCard = (): ExpiredCardOf | null =>
  useSyncExternalStore(hearCard, readCard, readCard);
