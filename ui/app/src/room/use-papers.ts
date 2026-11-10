// The open matter's papers for the wall (H8's ListPapers), listed again whenever the host says they changed
// (`papers_changed` on the app feed): a paper came, went or was renamed in the matter's folder (decision record
// 0055), or the background tracked-changes scan found Tracked changes in a .docx the last listing showed without
// them. They are listed again too when the window comes back to the front, which covers a folder the host doesn't
// watch (Windows, or one the OS refused) and anything a watch missed. A listing that finds the folder gone empties
// the wall: the moved folder's line says why (moved.tsx), and its Locate folder brings the papers back. They are
// listed again, too, whenever the matter's stream gains a newest entry (a turn's version, an undo, a restore), so a
// file the agent made shows without leaving the matter even where no watch saw it (D-031).
import { useEffect, useRef, useState } from "react";
import { useStreamPart, useWatch } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { AppHost, Papers, PapersHost } from "./host-next.ts";
import { matterStreamOf } from "./matter-stream.ts";
import type { ListedPapers } from "./open.ts";

/** The refusal of a listing whose folder is gone. */
const FOLDER_MISSING = "matter.folder_missing";

/** How long a listing may run before another may start: a reply that never comes (lost across a host restart)
 * must not freeze the wall. */
export const READ_WAIT_MS = 30_000;

/** The code of a failed read, when it has one. */
const codeOf = (error: unknown): string | null =>
  typeof error === "object" && error !== null && "code" in error && typeof error.code === "string"
    ? error.code
    : null;

/** The matter stream's newest entry, by id: a new version or marker changes it, streamed text elsewhere never does. */
const newestOf = (view: StreamView | undefined): string | null =>
  view === undefined ? null : `${view.epoch}:${view.ids.at(-1) ?? ""}`;

/** How many times the open matter's stream has gained a newest entry since the matter opened, holding the stream
 * while papers can be listed; its first window, which says nothing changed, counts nothing. */
export function useMatterMoves(
  store: ViewStore,
  next: PapersHost | null,
  matter: MatterId | null,
): number {
  const stream = next === null || matter === null ? null : matterStreamOf(matter);
  useWatch(store, stream);
  const newest = useStreamPart(store, stream, newestOf);
  const [moves, setMoves] = useState({ matter, newest, n: 0 });
  if (moves.matter !== matter) setMoves({ matter, newest, n: 0 });
  else if (moves.newest !== newest)
    setMoves({ matter, newest, n: moves.newest === null ? moves.n : moves.n + 1 });
  return moves.matter === matter ? moves.n : 0;
}

/** The matter's papers from H8, or null: before they arrive, without H8, when the read fails, or once the folder is
 * gone. `app`, when there is one, re-lists them on each `papers_changed` for the matter, one listing at a time;
 * without it the list follows the window's focus only. A change of `moved` (useMatterMoves) lists them again too. */
export function usePapers(
  next: PapersHost | null,
  app: AppHost | null,
  matter: MatterId | null,
  moved = 0,
): ListedPapers {
  const [listed, setListed] = useState<{ matter: MatterId; papers: Papers | null } | null>(null);
  // the open matter's listing, for a move of its stream: it keeps to one listing at a time
  const relist = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (next === null || matter === null) return;
    let live = true;
    // one listing at a time: a change while one runs marks the papers dirty, and they are
    // listed once more after it, so a burst of changes is two listings, never a queue of them
    let running = false;
    let dirty = false;
    // each listing's number: only the latest one's answer is shown
    let asked = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const list = () => {
      if (running) {
        dirty = true;
        return;
      }
      running = true;
      dirty = false;
      const mine = (asked += 1);
      let over = false;
      const done = () => {
        if (over) return;
        over = true;
        clearTimeout(timer);
        running = false;
        if (live && dirty) list();
      };
      timer = setTimeout(done, READ_WAIT_MS);
      next.listPapers(matter).then(
        (papers) => {
          if (live && mine === asked) setListed({ matter, papers });
          done();
        },
        (error: unknown) => {
          console.warn(`list_papers failed: ${JSON.stringify(error)}`);
          // only a folder that is gone empties the wall: the moved folder's line says why
          if (live && mine === asked && codeOf(error) === FOLDER_MISSING)
            setListed({ matter, papers: null });
          done();
        },
      );
    };
    relist.current = list;
    list();
    const stop = app?.subscribeApp((event) => {
      if ("papers_changed" in event && event.papers_changed.matter === matter) list();
    });
    window.addEventListener("focus", list);
    return () => {
      live = false;
      relist.current = null;
      clearTimeout(timer);
      stop?.();
      window.removeEventListener("focus", list);
    };
  }, [next, app, matter]);
  useEffect(() => {
    if (moved > 0) relist.current?.();
  }, [moved]);
  return listed !== null && listed.matter === matter && listed.papers !== null
    ? { matter, papers: listed.papers }
    : null;
}
