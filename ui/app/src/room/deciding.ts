// Whether the room's first screen is still undecided: the welcome (first run, no matter yet) or the room itself. The
// welcome waits on Matters' first list (matters.ts's phase), and on a slow start (WebView2 cold after a reboot, a fresh
// profile) that list lands well after the room's first render; until then the room would paint its composer layout,
// then swap it for the welcome. While undecided the room stays mounted, its reads and effects running as before, but
// unseen, and the page's mark stands in its place (first-mark.tsx). Only where the host has the setup wire, since only
// there can the welcome show.
import { useEffect, useState } from "react";
import type { MattersView } from "./matters.ts";

/** How long the mark may stand while the list hasn't come: then the room shows, as it did before (nothing times the
 * list itself out, so a host that never answers would otherwise leave the mark up for good). 15 s, the first status
 * check's limit (#748): after a reboot or on a fresh profile the VM built the main window at 4–5 s and the host's first
 * answers came later still, so a shorter wait would bring the composer back before the welcome in just the starts this
 * is for. A refused list doesn't wait: the room shows at once. */
export const DECIDE_MS = 15_000;

/** True while the first screen isn't known yet: the setup wire is there, the list is loading, and DECIDE_MS hasn't
 * passed. A failed list decides it (the room, as before). */
export function useDeciding(setup: boolean, phase: MattersView["phase"]): boolean {
  const [late, setLate] = useState(false);
  const waiting = setup && phase === "loading";
  useEffect(() => {
    if (!waiting) return;
    const timer = setTimeout(() => setLate(true), DECIDE_MS);
    return () => clearTimeout(timer);
  }, [waiting]);
  return waiting && !late;
}
