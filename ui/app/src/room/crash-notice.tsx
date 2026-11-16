// The room's notice after Clauseo closed unexpectedly, one RailNotice placed where the update notice goes (room.tsx,
// rail-shown.ts's useNoticePlace): the rail's foot, the work's head (one column, the welcome) or the wall's. The host's
// LastExit, read once with `last_exit`, says whether the last run ended without its quit and which file actions it left
// unfinished (an undo, a restore, accepting or rejecting tracked changes, each recorded before it touched a file).
// "Clauseo closed unexpectedly." with "Your conversations are kept.", then a line per unfinished action, by count and
// by matter where the room knows its name, never a file name, and "Versions has the earlier copies." Got it hides it
// for this run (crash-seen.ts). After a clean quit with an action still running (an idle Quit, a logoff) only the
// action lines and Versions' line, no title. Nothing when the last run quit cleanly with nothing left, or while the
// query is refused.
import { useEffect, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { RailNotice } from "@clauseo/components/clauseo/rail-notice.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { uw } from "@clauseo/components/lib/words-update.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../transport/host-transport.ts";
import type { LastExit, MatterSummary } from "../transport/protocol.ts";
import { actionLine, carriesLastExit, focusBeside } from "./crash-lines.ts";
import { markCrashSeen, seenCrash } from "./crash-seen.ts";

export function CrashNotice({
  transport,
  matters,
}: {
  transport: HostTransport;
  /** The room's matters, for the names of those an action was in. */
  matters: readonly MatterSummary[];
}) {
  const { lang } = useDisplay();
  const [last, setLast] = useState<LastExit | null>(null);
  const [seen, setSeen] = useState(seenCrash);
  useEffect(() => {
    if (!carriesLastExit(transport)) return;
    let live = true;
    void (async () => {
      try {
        const answer = (await transport.query("last_exit")).last_exit;
        if (live && answer !== undefined) setLast(answer);
      } catch {
        // refused or not scripted: nothing shows
      }
    })();
    return () => {
      live = false;
    };
  }, [transport]);
  const interrupted = last?.interrupted ?? [];
  // unclean is the desktop's word alone: after a clean quit (an idle Quit, a logoff) an action still
  // running is named without saying Clauseo closed
  if (last === null || (!last.unclean && interrupted.length === 0)) return null;
  const run = String(last.run_started);
  if (seen === run) return null;
  const lines = [
    ...(last.unclean ? [uw(lang, "Your conversations are kept.")] : []),
    ...interrupted.map((action) => actionLine(lang, action, matters)),
    ...(interrupted.length > 0 ? [uw(lang, "Versions has the earlier copies.")] : []),
  ];
  return (
    <RailNotice
      label={uw(lang, "Last session")}
      state={last.unclean ? "closed-unexpectedly" : "unfinished"}
      className="crash-notice"
      live
      {...(last.unclean ? { title: <Say>{uw(lang, "Clauseo closed unexpectedly.")}</Say> } : {})}
      line={<Say>{lines.join(" ")}</Say>}
      actions={
        <Button
          kind="quiet"
          size="small"
          data-testid="crash-got-it"
          onClick={(event) => {
            // the card goes: focus stays in the rail's foot or the wall it stood in, not on the page
            focusBeside(event.currentTarget);
            markCrashSeen(run);
            setSeen(run);
          }}
        >
          <span className="deva-nudge">{uw(lang, "Got it")}</span>
        </Button>
      }
    />
  );
}
