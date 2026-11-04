// The line in a row's place on the rail after a snooze or Done (U16b), with Undo; loaded later, as it shows only once
// the host has answered.
import { useEffect, useRef } from "react";
import { useDisplay } from "../lib/display.ts";
import { t } from "../lib/words.ts";
import { Say } from "./say.tsx";

/** The line in a row's place after a snooze or Done: what happened, and Undo. It takes the focus where the row's
 * own button had it (gone with the row), so the keyboard stays on the rail. */
export function RailLine({ text, onUndo }: { text: string; onUndo: () => void }) {
  const { lang } = useDisplay();
  const undo = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const now = document.activeElement;
    if (now === null || now === document.body || undo.current?.closest("nav")?.contains(now))
      undo.current?.focus();
  }, []);
  return (
    <p className="room-line" role="status" data-testid="rail-line">
      <span className="deva-nudge">
        <Say>{text}</Say>
      </span>{" "}
      <button ref={undo} type="button" className="room-line-undo" onClick={onUndo}>
        {t(lang, "Undo")}
      </button>
    </p>
  );
}
