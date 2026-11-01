// Older entries, paged in when the lawyer scrolls to the top of the thread and the view says there are more
// (`hasOlder`): page_entries before the oldest entry shown, 50 at a time. The pages live here, in the thread's own
// state, never in the view store; they render above the live rows, which keep their place on the screen.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ViewStore } from "../../store/store.ts";
import { orderOf } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId, ViewEntry } from "../../transport/protocol.ts";

/** Entries a page asks for. */
export const PAGE = 50;
/** How close to the top, in px, counts as the top. */
const TOP = 8;

type Pages = { stream: StreamId; entries: readonly ViewEntry[]; done: boolean };

/** Whether an element scrolls itself, or leaves it to the page (the room below 1000 px). */
const scrolls = (el: HTMLElement) => getComputedStyle(el).overflowY !== "visible";

export function useOlder(
  transport: HostTransport,
  store: ViewStore,
  stream: StreamId | null,
  hasOlder: boolean,
  scroller: () => HTMLElement | null,
): readonly ViewEntry[] {
  const [pages, setPages] = useState<Pages | null>(null);
  const own = pages !== null && pages.stream === stream ? pages : null;
  const loading = useRef(false);
  /** The scroller's height before a page went in, to keep the rows in place after it. */
  const before = useRef<number | null>(null);

  const more = useCallback(() => {
    if (stream === null || !hasOlder || own?.done === true || loading.current) return;
    const view = store.view(stream);
    const oldest =
      own?.entries[0] ?? (view?.ids[0] === undefined ? undefined : view.entries.get(view.ids[0]));
    if (oldest === undefined) return;
    loading.current = true;
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
          setPages({ stream, entries: [...got, ...(own?.entries ?? [])], done: got.length < PAGE });
        },
        (error: unknown) => console.warn(`page_entries failed: ${JSON.stringify(error)}`),
      )
      .finally(() => {
        loading.current = false;
      });
  }, [transport, store, stream, hasOlder, own, scroller]);

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
    const look = () => {
      if ((scrolls(el) ? el.scrollTop : window.scrollY) <= TOP) more();
    };
    el.addEventListener("scroll", look, { passive: true });
    window.addEventListener("scroll", look, { passive: true });
    return () => {
      el.removeEventListener("scroll", look);
      window.removeEventListener("scroll", look);
    };
  }, [scroller, stream, hasOlder, own, more]);

  return own?.entries ?? NONE;
}

const NONE: readonly ViewEntry[] = [];
