// R23: the room's update notice (decision record 0037), one RailNotice: at the rail's foot above Scratch matter where
// the rail stands, at the head of the wall where it folds away (room.tsx places it). Ready: "Clauseo 0.4.1 is ready to
// install.", in auto mode with when it installs by itself, and Restart to update; while a turn runs in any matter,
// also After this answer and that restarting now stops the answer. Scheduled: it restarts after the answer, with
// Cancel. Installing: said, with no button, since Clauseo is about to quit. Then once after an update: what it was
// updated from and to, with Got it, kept per version (update-seen.ts). Nothing for idle, checking, downloading, failed
// or managed: Settings' Updates says those. Only this window sends install_update and cancel_scheduled_update.
import { useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { RailNotice } from "@clauseo/components/clauseo/rail-notice.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { uw, type UpdateKey } from "@clauseo/components/lib/words-update.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { updaterOf, updateStoreOf, useUpdate } from "../store/update.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { AllThreads } from "./all-threads.ts";
import { useTurnRunning } from "./turn-running.ts";
import { cancelScheduledUpdate, installUpdate } from "./update-commands.ts";
import { markUpdateSeen, seenUpdate } from "./update-seen.ts";

export function UpdateNotice({
  transport,
  threads,
}: {
  transport: HostTransport;
  /** Every matter's threads, for whether a turn runs anywhere. */
  threads: AllThreads | null;
}) {
  const { lang } = useDisplay();
  const store = updateStoreOf(transport);
  const { view, sending } = useUpdate(store);
  const running = useTurnRunning(threads);
  const [seen, setSeen] = useState(seenUpdate);
  if (view == null) return null;
  const { name, version } = updaterOf(view.state);
  const say = (key: UpdateKey, slots?: Record<string, string>) => <Say>{uw(lang, key, slots)}</Say>;
  const button = (key: UpdateKey, kind: "tone" | "quiet", testId: string, press: () => void) => (
    <Button
      kind={kind}
      size="small"
      disabled={sending !== null}
      data-testid={testId}
      onClick={press}
    >
      <span className="deva-nudge">{uw(lang, key)}</span>
    </Button>
  );
  const label = uw(lang, "Updates");
  if (name === "ready" && version !== null) {
    const lines = [
      ...(view.mode === "auto" ? [uw(lang, "It installs the next time Clauseo opens.")] : []),
      ...(running ? [uw(lang, "Restarting now stops the answer in progress.")] : []),
    ];
    const now = () => void installUpdate(transport, store, "now");
    return (
      <RailNotice
        label={label}
        state="ready"
        className="update-notice"
        live
        title={say("Clauseo {version} is ready to install.", { version })}
        {...(lines.length === 0 ? {} : { line: <Say>{lines.join(" ")}</Say> })}
        actions={
          running ? (
            <>
              {button("After this answer", "tone", "update-after", () => {
                void installUpdate(transport, store, "after_turn");
              })}
              {button("Restart to update", "quiet", "update-restart", now)}
            </>
          ) : (
            button("Restart to update", "tone", "update-restart", now)
          )
        }
      />
    );
  }
  if (name === "scheduled")
    return (
      <RailNotice
        label={label}
        state="scheduled"
        className="update-notice"
        live
        title={say("Clauseo will restart to update after this answer.")}
        actions={button("Cancel", "quiet", "update-cancel", () => {
          void cancelScheduledUpdate(transport, store);
        })}
      />
    );
  if (name === "installing")
    return (
      <RailNotice
        label={label}
        state="installing"
        className="update-notice"
        live
        title={say("Installing the update…")}
      />
    );
  const from = view.updated_from;
  if (from !== undefined && seen !== view.current)
    return (
      <RailNotice
        label={label}
        state="updated"
        className="update-notice"
        title={say("Clauseo was updated from {from} to {current}.", {
          from,
          current: view.current,
        })}
        actions={button("Got it", "quiet", "update-got-it", () => {
          markUpdateSeen(view.current);
          setSeen(view.current);
        })}
      />
    );
  return null;
}
