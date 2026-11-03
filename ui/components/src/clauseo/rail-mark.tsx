// The mark that leads a matter's status line on the rail and ends a conversation's line: one 14 px icon per status, in
// one place for every status (rail-marks.ts). Where the matter waits on the lawyer's hand (a question, a permission,
// the plan's limit with its choice still open) the icon stands on a court-yellow disc, as the prepared draft's yellow
// waits; everywhere else it takes the status line's own tone. Hidden from assistive technology: the status's words
// name it.
import type { Mark } from "./rail-marks.ts";

/** One status's mark: its icon at 14 px, on the yellow disc where it waits on the lawyer. */
export function RailMark({ mark, className }: { mark: Mark; className?: string }) {
  const Glyph = mark.icon;
  return (
    <span
      aria-hidden="true"
      className={`room-mark${className === undefined ? "" : ` ${className}`}`}
      data-waits={mark.waits ? "" : undefined}
    >
      <Glyph size={mark.waits ? 11 : 14} />
    </span>
  );
}
