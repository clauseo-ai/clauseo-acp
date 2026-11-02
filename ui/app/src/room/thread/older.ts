// Older entries, paged in when the lawyer scrolls to the top of the thread and the view says there are more
// (`hasOlder`): page_entries before the oldest entry shown, 50 at a time. The pages live here, in the thread's own
// state, never in the view store; they render above the live rows, which keep their place on the screen.
//
// Pages hold only while they join the live window: a Reset (a host restart, or a subscriber that fell behind, in the
// same epoch or a new one) can start the window later than it started when the pages were taken, and the entries
// between would be in neither. So pages belong to the stream, the epoch and the window's first entry as it was when
// they went in; when the window starts later, or the epoch changes, they are dropped and paging starts again from the
// new window.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useStreamPart } from "../../store/hooks.ts";
import type { ViewStore } from "../../store/store.ts";
import { orderOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId, ViewEntry, ViewEpoch } from "../../transport/protocol.ts";

/** Entries a page asks for. */
export const PAGE = 50;
/** How close to the top, in px, counts as the top. */
const TOP = 8;

type Pages = {
  stream: StreamId;
  epoch: ViewEpoch | null;
  /** The live window's first order when the pages went in: they join it. */
  anchor: number;
  entries: readonly ViewEntry[];
  done: boolean;
};

/** Whether an element scrolls itself, or leaves it to the page (the room below 1000 px). */
const scrolls = (el: HTMLElement) => getComputedStyle(el).overflowY !== "visible";

const pickEpoch = (v: StreamView | undefined) => v?.epoch ?? null;
const pickFirst = (v: StreamView | undefined): number | null => {
  const id = v?.ids[0];
  const entry = id === undefined ? undefined : v!.entries.get(id);
  return entry === undefined ? null : orderOf(entry);
};

export type Older = {
  readonly entries: readonly ViewEntry[];
  /** A page is on its way. */
  readonly loading: boolean;
};

export function useOlder(
  transport: HostTransport,
  store: ViewStore,
  stream: StreamId | null,
  hasOlder: boolean,
  scroller: () => HTMLElement | null,
): Older {
  const [pages, setPages] = useState<Pages | null>(null);
  const [loading, setLoading] = useState(false);
  const epoch = useStreamPart(store, stream, pickEpoch);
  const first = useStreamPart(store, stream, pickFirst);
  const own =
    pages !== null &&
    pages.stream === stream &&
    pages.epoch === epoch &&
    first !== null &&
    first <= pages.anchor
      ? pages
      : null;
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
    const anchor = own?.anchor ?? first;
    if (oldest === undefined || anchor === null) return;
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
            epoch,
            anchor,
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
  }, [transport, store, stream, epoch, first, place, hasOlder, own, scroller]);

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
    const look = () => {
      if ((self ? el.scrollTop : window.scrollY) > TOP) refusedAt.current = null;
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
  }, [scroller, stream, hasOlder, own, more]);

  return { entries: own?.entries ?? NONE, loading };
}

const NONE: readonly ViewEntry[] = [];
