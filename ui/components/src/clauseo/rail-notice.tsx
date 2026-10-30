// What the room says about Clauseo itself, an update, the first crash, a lab's terms changing,
// as one quiet card. It stands at the rail's foot, in its own row above Scratch matter, so the matters scroll above it
// and nothing sits under it; where the rail folds away, at 1000 to 1199 px and on a phone, the room puts it at the
// head of the wall instead, before the papers, in flow. Words carry every state, in concrete, never in a colour.
import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";

export type RailNoticeProps = {
  /** What the card is about, for assistive technology, such as "Updates". */
  label: string;
  /** The title, at the chip's weight: "An update is ready". */
  title?: ReactNode;
  /** One line under it, muted: "Restarting stops the answer that's running." */
  line?: ReactNode;
  /** The line in primary, where it says what will happen: "Restarts after this answer, by 4:32 pm." */
  firm?: boolean;
  /** The question asked, at the chip's weight, with any link on its line: "Send this crash report?…" */
  ask?: ReactNode;
  /** The card's buttons, in one row that wraps. */
  actions?: ReactNode;
  /** Optional: more, such as a folded list of what changed, under the actions. */
  details?: ReactNode;
  /** The card's state, for the page's own styling and the tests. */
  state?: string;
  /** Polite live region, for a card that changes as the lawyer answers it. */
  live?: boolean;
  className?: string;
};

export function RailNotice({
  label,
  title,
  line,
  firm = false,
  ask,
  actions,
  details,
  state,
  live = false,
  className,
}: RailNoticeProps) {
  return (
    <section
      className={cn("rail-notice", className)}
      aria-label={label}
      data-state={state}
      aria-live={live ? "polite" : undefined}
    >
      {title !== undefined && <p className="rail-notice-title">{title}</p>}
      {line !== undefined && (
        <p className={cn("rail-notice-line", firm && "rail-notice-firm")}>{line}</p>
      )}
      {ask !== undefined && <p className="rail-notice-ask">{ask}</p>}
      {actions !== undefined && <div className="rail-notice-acts">{actions}</div>}
      {details}
    </section>
  );
}
