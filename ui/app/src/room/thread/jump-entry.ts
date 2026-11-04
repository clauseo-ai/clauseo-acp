// Opening a conversation at one of its entries (U16a: a Find result found in something said): the entry's `order`, as
// the host's search gives it and page_entries pages by. Find asks with `jumpTo`; the open thread's list (list.tsx)
// reads the ask for its stream, pages back (older.ts, `reach`) until the entry is among the rows, then scrolls it into
// view and lets the ask go. An entry folded into a step group has no row of its own: the nearest row before it stands in.
// There is one ask at a time, and it never outlives its open: the next Find result replaces it (a title's clears it),
// opening any other conversation clears it (`useJumpFollows`), and so does paging back to the thread's start without
// finding the entry. Nothing runs at rest: the ask is a value, read on the commits that bring rows in.
import { useEffect, useLayoutEffect, useSyncExternalStore } from "react";
import { useStreamPart } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import { entryIdOf, orderOf, streamOf, type StreamView } from "../../store/view.ts";
import type { StreamId, ThreadId, ViewEntry } from "../../transport/protocol.ts";

type Jump = { stream: StreamId; order: number };

let asked: Jump | null = null;
const listeners = new Set<() => void>();

function put(jump: Jump | null): void {
  asked = jump;
  for (const listener of listeners) listener();
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => void listeners.delete(listener);
};

/** Asks the thread `stream`, once it is open, to show its entry `order`. A later ask replaces it. */
export function jumpTo(stream: StreamId, order: number): void {
  put({ stream, order });
}

/** Lets the ask go, if there is one: a Find result found by its title opens its conversation where it rests. */
export function clearJump(): void {
  if (asked !== null) put(null);
}

/** The ask, for tests. */
export const jumpAsked = (): Jump | null => asked;

/** Lets the ask go once another conversation than the one asked is open (`thread`), or a draft (`drafting`). While
 * a matter's list loads (no thread, no draft) the ask waits. */
export function useJumpFollows(thread: ThreadId | null, drafting: boolean): void {
  useEffect(() => {
    if (asked === null) return;
    if (drafting || (thread !== null && asked.stream !== streamOf(thread))) put(null);
  }, [thread, drafting]);
}

/** The order of the entry `stream` is asked to show; null with no ask for it. */
export function useJumpOrder(stream: StreamId | null): number | null {
  return useSyncExternalStore(
    subscribe,
    () => (asked !== null && asked.stream === stream ? asked.order : null),
    () => null,
  );
}

const pickIds = (v: StreamView | undefined) => v?.ids;

/**
 * Once the entry `order` of `stream` is among the rows (the older pages or the live window), scrolls its row (or the
 * nearest row before it) into view and lets the ask go. Fonts settling can move the thread back to its resting place
 * (thread.tsx), so the row is brought into view again then. Paged back to the thread's start (`start`) with no entry at
 * or before it, the ask goes with nothing scrolled.
 */
export function useJumpLanding(
  store: ViewStore,
  stream: StreamId | null,
  order: number | null,
  older: readonly ViewEntry[],
  scroller: () => HTMLElement | null,
  start = false,
): void {
  const ids = useStreamPart(store, stream, pickIds);
  useLayoutEffect(() => {
    if (stream === null || order === null || ids === undefined) return;
    const view = store.view(stream);
    const entries = [...older, ...ids.flatMap((id) => view?.entries.get(id) ?? [])];
    const mine = asked?.stream === stream && asked.order === order;
    if (!entries.some((e) => orderOf(e) <= order)) {
      if (start && mine) put(null);
      return;
    }
    const root = scroller() ?? document;
    const row = entries
      .filter((e) => orderOf(e) <= order)
      .sort((a, b) => orderOf(b) - orderOf(a))
      .map((e) => root.querySelector(`[data-entry-id="${CSS.escape(entryIdOf(e))}"]`))
      .find((el) => el !== null);
    if (row === undefined) return;
    const show = () => row.scrollIntoView({ block: "center" });
    show();
    void document.fonts?.ready.then(show);
    if (mine) put(null);
  }, [store, stream, order, older, ids, scroller, start]);
}
