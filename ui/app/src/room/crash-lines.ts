// The crash notice's lines (crash-notice.tsx), kept apart from the component: what an unfinished action was, its
// sentence, whether a transport answers `last_exit` at all, and where focus goes when the notice is dismissed.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { uw } from "@clauseo/components/lib/words-update.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { InterruptedAction, MatterSummary } from "../transport/protocol.ts";

/** What the action was, as the line names it: "Undo of 3 files". */
export function actionWords(lang: Lang, action: InterruptedAction): string {
  const count = action.files;
  switch (action.kind) {
    case "undo":
      return count === 1 || count === undefined
        ? uw(lang, "Undo of 1 file")
        : uw(lang, "Undo of {count} files", { count: String(count) });
    case "restore":
      if (count === undefined) return uw(lang, "A restore");
      return count === 1
        ? uw(lang, "Restore of 1 file")
        : uw(lang, "Restore of {count} files", { count: String(count) });
    case "accept_changes":
      return uw(lang, "Accepting tracked changes");
    case "reject_changes":
      return uw(lang, "Rejecting tracked changes");
  }
}

/** One unfinished action's sentence, with its matter's name when the room has it and it isn't the Scratch matter. A
 * restore has sentences of its own, the same in English, whose Hindi agrees with बहाली. */
export function actionLine(
  lang: Lang,
  action: InterruptedAction,
  matters: readonly MatterSummary[],
): string {
  const what = actionWords(lang, action);
  const matter = matters.find((m) => m.matter === action.matter);
  const named = matter !== undefined && matter.kind !== "scratch" ? matter.name : null;
  if (action.kind === "restore")
    return named === null
      ? uw(lang, "{restore} may not have finished.", { restore: what })
      : uw(lang, "{restore} in {matter} may not have finished.", { restore: what, matter: named });
  return named === null
    ? uw(lang, "{action} may not have finished.", { action: what })
    : uw(lang, "{action} in {matter} may not have finished.", { action: what, matter: named });
}

/** Whether the transport answers `last_exit`: the desktop's does; a fake only when its test or review state sets
 * `lastExitWire`, so the room over any other fake sends no query it doesn't expect. */
export const carriesLastExit = (transport: HostTransport): boolean =>
  (transport as { lastExitWire?: unknown }).lastExitWire === true;

const FOCUSABLE =
  'button:not([disabled]), a[href], input:not([disabled]), [tabindex]:not([tabindex="-1"])';

// The places a notice stands in, nearest first: the rail's foot, then the rest of the rail; the work's
// head, then the rest of the work (the welcome's buttons, the composer); the wall.
const PLACES = ".room-rail-foot, .room-rail, .room-notices, .room-centre, .room-pane";

/** Moves focus from `from`, inside a notice about to go, to the first control outside the notice in
 * the nearest place it stands in that has one (PLACES, from the notice outward); to the nearest place
 * itself when none has. */
export function focusBeside(from: HTMLElement): void {
  const notice = from.closest(".rail-notice");
  const outside = (el: HTMLElement) => notice === null || !notice.contains(el);
  let nearest: HTMLElement | null = null;
  for (let place = from.closest<HTMLElement>(PLACES); place !== null;) {
    nearest ??= place;
    const next = [...place.querySelectorAll<HTMLElement>(FOCUSABLE)].find(outside);
    if (next !== undefined) {
      next.focus();
      return;
    }
    place = place.parentElement?.closest<HTMLElement>(PLACES) ?? null;
  }
  if (nearest === null) return;
  if (!nearest.hasAttribute("tabindex")) nearest.tabIndex = -1;
  nearest.focus();
}
