// The folder picker's wait, for its line: a pick that runs PICKER_LINE_MS or longer shows "Opening the folder picker…"
// where the caller draws it, and the line goes the moment the pick resolves, picked, cancelled or errored. A picker that
// answers at once shows nothing, so no line flashes. Shared by New matter (folder-check.tsx), the first screen's Open
// the folder, and a moved folder's Locate (room.tsx). Where the desktop opens Clauseo's own chooser instead
// (transport/chooser.ts), the line covers its check before and goes once the chooser shows.
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { chooserShowing, onChooserShowing } from "../transport/chooser.ts";

/** How long a pick may run before its line shows. */
export const PICKER_LINE_MS = 300;

/** `opening` is true while a pick has run PICKER_LINE_MS; `track` runs a pick and gives its answer, or rethrows its
 * error, so the caller's own handling is unchanged. */
export function usePickWait() {
  const [opening, setOpening] = useState(false);
  const picks = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const track = async <T>(pick: () => Promise<T>): Promise<T> => {
    if (picks.current === 0) timer.current = setTimeout(() => setOpening(true), PICKER_LINE_MS);
    picks.current += 1;
    try {
      return await pick();
    } finally {
      picks.current -= 1;
      if (picks.current === 0) {
        clearTimeout(timer.current);
        setOpening(false);
      }
    }
  };
  // Clauseo's own chooser, once it shows, is the answer the line waited for
  const choosing = useSyncExternalStore(onChooserShowing, chooserShowing, chooserShowing);
  return { opening: opening && !choosing, track };
}
