// The Snoozed shelf, open (U16b), loaded later: each snoozed conversation (its title, its matter beneath) or matter
// snoozed whole (its name, "Whole matter"), the alarm clock and "Until {when}", and Wake now (the up arrow: back up
// into the list, apart from Mark as not done's undo arrow) at its end; then why a
// row may come back sooner, behind the info icon.
import { Button } from "../ui/button.tsx";
import { IconArrowUp, IconSnooze } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { langOfText } from "../lib/script.ts";
import { twl } from "../lib/words-triage-late.ts";
import { RailInfo } from "./rail-info.tsx";
import { Say } from "./say.tsx";

export type ShelfRow = {
  key: string;
  /** The conversation's title, or the matter's name when it's snoozed whole. */
  title: string;
  /** The conversation's matter; null for a matter snoozed whole. */
  matter: string | null;
  /** When it comes back, as words: "tomorrow 9:00 am". */
  until: string;
  onOpen: () => void;
  onWake: () => void;
};

export function ShelfRows({ rows }: { rows: readonly ShelfRow[] }) {
  const { lang } = useDisplay();
  const wake = twl(lang, "Wake now");
  return (
    <>
      <ul className="room-shelf-rows">
        {rows.map((r) => (
          <li key={r.key} className="room-conv-item room-one-act" data-testid="shelf-row">
            <button type="button" className="room-shelf-row" onClick={r.onOpen}>
              <span className="room-matter-name" lang={langOfText(r.title)}>
                {r.title}
              </span>
              <span className="room-matter-note">
                {r.matter === null ? (
                  <Say>{twl(lang, "Whole matter")}</Say>
                ) : (
                  <span lang={langOfText(r.matter)}>{r.matter}</span>
                )}
              </span>
              <span className="room-shelf-until">
                <IconSnooze size={14} className="flex-none" />
                <Say>{twl(lang, "Until {when}", { when: r.until })}</Say>
              </span>
            </button>
            <span className="room-conv-acts">
              <Button
                kind="quiet"
                size="small"
                shape="icon"
                aria-label={`${wake}, ${r.title}`}
                title={wake}
                data-testid="wake-now"
                onClick={r.onWake}
              >
                <IconArrowUp size={16} />
              </Button>
            </span>
          </li>
        ))}
      </ul>
      <RailInfo words={twl(lang, "Back sooner if Clauseo needs you or an answer finishes.")} />
    </>
  );
}
