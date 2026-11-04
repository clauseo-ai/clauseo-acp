// The morning sweep (U16b, H-triage): the host marks done each conversation read, with nothing waiting, last active
// before today, when the app sends `sweep_done { day_start }` with the lawyer's local midnight. The host knows no zone,
// so the app sends it at launch and each time the window comes back (focus, or shown again), whenever the lawyer's
// clock reads 05:00 or later; before 05:00 it sends nothing. No timer: a window left open overnight sweeps when the
// lawyer comes back to it. The command is the host's to make idempotent, so every such moment sends it. The rail
// (rail.tsx) is the main window's alone, so the Settings window never sends it.
import { useEffect } from "react";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { Timestamp } from "../transport/protocol.ts";
import { atLocal, wallOf } from "./local-time.ts";

/** 05:00, in minutes: the sweep's first moment each day. */
const FROM = 5 * 60;

/** Today's local midnight in `zone`, once the clock reads 05:00; null before. */
export function sweepStart(now: Timestamp, zone: string): Timestamp | null {
  const { day, minute } = wallOf(now, zone);
  return minute < FROM ? null : atLocal(day, 0, zone);
}

/** Sends the sweep at mount and each time the window comes back; `onBack` hears each return with the clock's now, so
 * the rail draws again with the time (a snooze that ran out comes back then). */
export function useSweep(
  transport: HostTransport,
  zone: string,
  onBack: (now: Timestamp) => void,
): void {
  useEffect(() => {
    const sweep = (now: Timestamp) => {
      const start = sweepStart(now, zone);
      if (start === null) return;
      transport
        .dispatch({
          command_id: crypto.randomUUID(),
          command: { sweep_done: { day_start: start } },
        })
        .catch((error: unknown) =>
          console.warn(`sweep_done failed: ${JSON.stringify(toHostError(error).code)}`),
        );
    };
    const back = () => {
      const now = Date.now();
      onBack(now);
      sweep(now);
    };
    // the window's own focus, not a control's inside it
    const focused = (e: FocusEvent) => {
      if (!(e.target instanceof Node)) back();
    };
    const shown = () => {
      if (document.visibilityState === "visible") back();
    };
    sweep(Date.now());
    window.addEventListener("focus", focused);
    document.addEventListener("visibilitychange", shown);
    return () => {
      window.removeEventListener("focus", focused);
      document.removeEventListener("visibilitychange", shown);
    };
  }, [transport, zone, onBack]);
}
