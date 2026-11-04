// The Snoozed shelf (U16b), at the foot of the rail's list above Scratch matter: folded, "Snoozed", how many, and when
// the next comes back; open, what the room draws (rail-shelf-rows.tsx, loaded later).
import type { ReactNode } from "react";
import { useDisplay } from "../lib/display.ts";
import { tw } from "../lib/words-triage.ts";
import { Fold } from "./rail-fold.tsx";

export function RailShelf({
  count,
  next,
  rows,
  testId,
}: {
  count: number;
  /** When the next comes back, as words: "6:00 pm", "tomorrow 9:00 am". */
  next: string;
  /** The open shelf's rows. */
  rows: () => ReactNode;
  testId?: string;
}) {
  const { lang } = useDisplay();
  return (
    <Fold
      name={tw(lang, "Snoozed")}
      count={count}
      folded={tw(lang, "Next back {when}", { when: next })}
      className="room-shelf"
      testId={testId}
    >
      {rows}
    </Fold>
  );
}
