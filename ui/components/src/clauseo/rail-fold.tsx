// A fold on the rail (U16b): the Snoozed shelf at the list's foot and the Done fold at the end of a matter's
// conversations. Its heading is a button with the fold's chevron, its name and a count; pressed, it opens what the
// room draws inside (loaded later), and folded it shows its `folded` line, if any. Open, its explanation's info icon
// (RailInfo) stands in the heading's row, after its name, beside the heading rather than in it.
import { useState, type ReactNode } from "react";
import { IconFold } from "../icons/icons.tsx";
import { FoldInfo } from "./rail-fold-info.ts";
import { Heading } from "./heading.tsx";
import { Say } from "./say.tsx";

export function Fold({
  name,
  count,
  folded,
  children,
  className,
  testId,
}: {
  name: string;
  count: number;
  /** Optional: a line under the folded heading. */
  folded?: string | undefined;
  /** What stands under the opened heading. */
  children: () => ReactNode;
  className: string;
  testId?: string | undefined;
}) {
  const [open, setOpen] = useState(false);
  const [info, setInfo] = useState<HTMLElement | null>(null);
  return (
    <section className={className} data-testid={testId}>
      <div className="room-fold-head">
        <Heading level={2} depth={1} className="room-fold-h">
          <button
            type="button"
            className="room-fold"
            aria-expanded={open}
            onClick={() => setOpen(!open)}
          >
            <IconFold size={13} className="room-fold-icon" />
            <span className="room-fold-name deva-nudge">{name}</span>
            <span className="room-fold-count">{count}</span>
          </button>
        </Heading>
        <span ref={setInfo} className="room-fold-slot" />
      </div>
      {open ? (
        <FoldInfo value={info}>{children()}</FoldInfo>
      ) : (
        folded !== undefined && (
          <p className="room-fold-line">
            <Say>{folded}</Say>
          </p>
        )
      )}
    </section>
  );
}
