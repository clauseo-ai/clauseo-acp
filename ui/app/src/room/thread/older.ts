// Older entries, paged in when the lawyer scrolls to the top of the thread and the view says there are more
// (`hasOlder`): page_entries before the oldest entry shown, 50 at a time. The pages live here, in the thread's own
// state, never in the view store; they render above the live rows, which keep their place on the screen.
//
// Pages hold only while they join the live window: a Reset (a host restart, or a subscriber that fell behind, in the
// same epoch or a new one) can start the window later than it started when the pages were taken, and the entries
// between would be in neither. So pages belong to the stream and to the window as it was when they went in
// (`moves`); when the epoch changes, or a Reset starts the window later, they are dropped and paging starts again
// from the new window. A `remove` of the window's first entries starts it later too, but loses nothing: the pages
// stay. The two are told apart by the new first entry: a remove leaves it the very object the window held before,
// among its first entries; a Reset brings every entry as a new object. (Should one of those entries have been
// replaced by an upsert since the id list last changed, a remove reads as a Reset: the pages are asked for again,
// nothing lost.)
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useStreamPart } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import { entryIdOf, orderOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, StreamId, ViewEntry, ViewEpoch } from "../../transport/protocol.ts";

/** Entries a page asks for. */
export const PAGE = 50;
/** How close to the top, in px, counts as the top. */
const TOP = 8;
/** How many of the window's first entries the picture keeps: a remove of more at once reads as a Reset. */
const HEAD = 16;

type Pages = {
  stream: StreamId;
  /** The window's moves when they went in (`moves`): they join the window while it hasn't moved since. */
  moved: number;
  entries: readonly ViewEntry[];
  done: boolean;
};

/** Whether an element scrolls itself, or leaves it to the page (the room below 1000 px). */
const scrolls = (el: HTMLElement) => getComputedStyle(el).overflowY !== "visible";

const pickEpoch = (v: StreamView | undefined) => v?.epoch ?? null;
const firstOf = (v: StreamView): ViewEntry | undefined =>
  v.ids[0] === undefined ? undefined : v.entries.get(v.ids[0]);

/** A pick counting the times the live window moved away from what pages joined: a new epoch, or a Reset that starts
 * it later. It keeps its picture of the window between calls and reads a view again only when its id list or epoch
 * is new, so asked twice of one view it answers the same. */
function moves(): (v: StreamView | undefined) => number {
  let count = 0;
  let ids: readonly EntryId[] | null = null;
  let epoch: ViewEpoch | null = null;
  let first: number | null = null;
  let head = new Map<EntryId, ViewEntry>();
  return (v) => {
    if (v === undefined || (v.ids === ids && v.epoch === epoch)) return count;
    const entry = firstOf(v);
    const order = entry === undefined ? null : orderOf(entry);
    const later = order !== null && first !== null && order > first;
    const kept = entry !== undefined && head.get(entryIdOf(entry)) === entry;
    if (ids !== null && (v.epoch !== epoch || (later && !kept))) count += 1;
    ids = v.ids;
    epoch = v.epoch;
    first = order;
    head = new Map();
    for (const id of v.ids.slice(0, HEAD)) {
      const e = v.entries.get(id);
      if (e !== undefined) head.set(id, e);
    }
    return count;
  };
}

export type Older = {
  readonly entries: readonly ViewEntry[];
  /** A page is on its way. */
  readonly loading: boolean;
  /** The thread's first entry is held: no older page is left to ask for. */
  readonly start: boolean;
};

export function useOlder(
  transport: HostTransport,
  store: ViewStore,
  stream: StreamId | null,
  hasOlder: boolean,
  scroller: () => HTMLElement | null,
  /** An entry's order to page back to (jump-entry.ts, a Find result): pages are asked for, one after another, until
   * the oldest entry held is at or before it. Null asks for none. */
  reach: number | null = null,
): Older {
  const [pages, setPages] = useState<Pages | null>(null);
  const [loading, setLoading] = useState(false);
  const epoch = useStreamPart(store, stream, pickEpoch);
  const [pickMoved] = useState(moves);
  const moved = useStreamPart(store, stream, pickMoved);
  const own = pages !== null && pages.stream === stream && pages.moved === moved ? pages : null;
  const asking = useRef(false);
  /** Where a page was refused: no asking again there until the epoch changes or the view leaves the top. */
  const refusedAt = useRef<string | null>(null);
  const place = `${stream}|${epoch}`;
  /** The scroller's height before a page went in, to keep the rows in place after it. */
  const before = useRef<number | null>(null);

  const more = useCallback(() => {
    if (stream === null || !hasOlder || own?.done === true || asking.current) return;
    if (refusedAt.current === place) return;
    const view = store.view(stream);
    const oldest =
      own?.entries[0] ?? (view?.ids[0] === undefined ? undefined : view.entries.get(view.ids[0]));
    if (oldest === undefined) return;
    asking.current = true;
    setLoading(true);
    transport
      .query({ page_entries: { stream, before_order: orderOf(oldest), limit: PAGE } })
      .then(
        (result) => {
          const got = result.entries ?? [];
          const el = scroller();
          before.current =
            el === null
              ? null
              : scrolls(el)
                ? el.scrollHeight
                : document.documentElement.scrollHeight;
          setPages({
            stream,
            moved,
            entries: [...got, ...(own?.entries ?? [])],
            done: got.length < PAGE,
          });
        },
        (error: unknown) => {
          refusedAt.current = place;
          console.warn(`page_entries failed: ${JSON.stringify(error)}`);
        },
      )
      .finally(() => {
        asking.current = false;
        setLoading(false);
      });
  }, [transport, store, stream, moved, place, hasOlder, own, scroller]);

  // the rows below the new page stay where the lawyer was reading
  useLayoutEffect(() => {
    const was = before.current;
    const el = scroller();
    before.current = null;
    if (was === null || el === null) return;
    if (scrolls(el)) el.scrollTop += el.scrollHeight - was;
    else window.scrollBy(0, document.documentElement.scrollHeight - was);
  }, [own, scroller]);

  useEffect(() => {
    const el = scroller();
    if (el === null || stream === null || !hasOlder || own?.done === true) return;
    // one listener, on whichever scrolls: the thread itself, or the page below 1000 px; asked again on a resize
    let self = scrolls(el);
    let target: HTMLElement | Window = self ? el : window;
    // a jump to an entry above what is held (reach) asks for the next page wherever the view is, as each lands
    const view = store.view(stream);
    const oldest = own?.entries[0] ?? (view === undefined ? undefined : firstOf(view));
    const jumping = reach !== null && oldest !== undefined && orderOf(oldest) > reach;
    const look = () => {
      if (jumping) more();
      else if ((self ? el.scrollTop : window.scrollY) > TOP) refusedAt.current = null;
      else more();
    };
    // at most one look a frame, however many scroll events the frame brings
    let frame = 0;
    const onScroll = () => {
      if (frame === 0)
        frame = requestAnimationFrame(() => {
          frame = 0;
          look();
        });
    };
    const onResize = () => {
      const now = scrolls(el);
      if (now === self) return;
      target.removeEventListener("scroll", onScroll);
      self = now;
      target = self ? el : window;
      target.addEventListener("scroll", onScroll, { passive: true });
    };
    target.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onResize, { passive: true });
    // a thread that opens at its top, or too short to scroll, is at the top already
    look();
    return () => {
      cancelAnimationFrame(frame);
      target.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onResize);
    };
  }, [scroller, stream, hasOlder, own, more, reach, store, epoch]);

  return { entries: own?.entries ?? NONE, loading, start: !hasOlder || own?.done === true };
}

const NONE: readonly ViewEntry[] = [];
