// The open matter's papers for the wall (H8's ListPapers), listed again whenever the host says they changed
// (`papers_changed` on the app feed): a paper came, went or was renamed in the matter's folder (decision record 0055),
// or the background tracked-changes scan found Tracked changes in a .docx the last listing showed without them. The
// host's listing answers at once from what the scan has read, so a re-list is cheap. They are listed again too when
// the window comes back to the front, which covers a folder the host doesn't watch (Windows, or one the OS refused) and
// anything a watch missed. A listing that finds the folder gone, or not the matter's, empties the wall: the moved
// folder's line says why (moved.tsx), and its Locate folder brings the papers back.
import { useEffect, useState } from "react";
import type { MatterId } from "../transport/protocol.ts";
import { locatable } from "./act-refusals.ts";
import type { AppHost, Papers, PapersHost } from "./host-next.ts";
import type { ListedPapers } from "./open.ts";

/** The code of a failed read, when it has one. */
const codeOf = (error: unknown): string | null =>
  typeof error === "object" && error !== null && "code" in error && typeof error.code === "string"
    ? error.code
    : null;

/** The matter's papers from H8, or null: before they arrive, without H8, when the read fails, or once the folder is
 * gone. `app`, when there is one, re-lists them on each `papers_changed` for the matter; a later answer replaces an
 * earlier one, never the reverse; without it the list follows the window's focus only. */
export function usePapers(
  next: PapersHost | null,
  app: AppHost | null,
  matter: MatterId | null,
): ListedPapers {
  const [listed, setListed] = useState<{ matter: MatterId; papers: Papers | null } | null>(null);
  useEffect(() => {
    if (next === null || matter === null) return;
    let live = true;
    // each listing's number: an answer older than the last one shown is dropped
    let asked = 0;
    let shown = 0;
    const list = () => {
      asked += 1;
      const mine = asked;
      next.listPapers(matter).then(
        (papers) => {
          if (!live || mine < shown) return;
          shown = mine;
          setListed({ matter, papers });
        },
        (error: unknown) => {
          console.warn(`list_papers failed: ${JSON.stringify(error)}`);
          if (!live || mine < shown || !locatable(codeOf(error))) return;
          shown = mine;
          setListed({ matter, papers: null });
        },
      );
    };
    list();
    const stop = app?.subscribeApp((event) => {
      if ("papers_changed" in event && event.papers_changed.matter === matter) list();
    });
    window.addEventListener("focus", list);
    return () => {
      live = false;
      stop?.();
      window.removeEventListener("focus", list);
    };
  }, [next, app, matter]);
  return listed !== null && listed.matter === matter && listed.papers !== null
    ? { matter, papers: listed.papers }
    : null;
}
