// The room's notice after Clauseo closed unexpectedly, one RailNotice placed where the update notice goes (room.tsx):
// at the rail's foot above Scratch matter where the rail stands, at the head of the wall where it folds away. The host's
// LastExit, read once with `last_exit`, says whether the last run ended without its quit and which file actions it left
// unfinished (an undo, a restore, accepting or rejecting tracked changes, each recorded before it touched a file).
// "Clauseo closed unexpectedly." with "Your conversations are kept.", then a line per unfinished action, by count and
// by matter where the room knows its name, never a file name, and "Versions has the earlier copies." Got it hides it
// for this run (crash-seen.ts). Nothing when the last run quit cleanly, or while the query is refused.
import { useEffect, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { RailNotice } from "@clauseo/components/clauseo/rail-notice.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { uw } from "@clauseo/components/lib/words-update.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../transport/host-transport.ts";
import type { LastExit, MatterSummary } from "../transport/protocol.ts";
import { actionLine, carriesLastExit } from "./crash-lines.ts";
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
  if (last === null || !last.unclean) return null;
  const run = String(last.run_started);
  if (seen === run) return null;
  const interrupted = last.interrupted ?? [];
  const lines = [
    uw(lang, "Your conversations are kept."),
    ...interrupted.map((action) => actionLine(lang, action, matters)),
    ...(interrupted.length > 0 ? [uw(lang, "Versions has the earlier copies.")] : []),
  ];
  return (
    <RailNotice
      label={uw(lang, "Last session")}
      state="closed-unexpectedly"
      className="crash-notice"
      title={<Say>{uw(lang, "Clauseo closed unexpectedly.")}</Say>}
      line={<Say>{lines.join(" ")}</Say>}
      actions={
        <Button
          kind="quiet"
          size="small"
          data-testid="crash-got-it"
          onClick={() => {
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
