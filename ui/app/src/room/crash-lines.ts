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

/** Moves focus from `from`, inside a notice about to go, to the first control of the place the notice stands in (the
 * rail's foot, else the rail, else the wall) outside the notice; to that place itself when it has none. */
export function focusBeside(from: HTMLElement): void {
  const notice = from.closest(".rail-notice");
  const place = from.closest<HTMLElement>(".room-rail-foot, .room-rail, .room-pane");
  if (place === null) return;
  const next = [...place.querySelectorAll<HTMLElement>(FOCUSABLE)].find(
    (el) => notice === null || !notice.contains(el),
  );
  if (next !== undefined) {
    next.focus();
    return;
  }
  if (!place.hasAttribute("tabindex")) place.tabIndex = -1;
  place.focus();
}
