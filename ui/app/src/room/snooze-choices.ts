// The snooze menu's rows in the language (U16b): snooze-times.ts's times as the lawyer's clock says them, with
// Tomorrow's and Monday's words. Loaded with the menu (snooze-late.tsx).
import type { SnoozeChoice } from "@clauseo/components/clauseo/snooze-menu.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import { twl } from "@clauseo/components/lib/words-triage-late.ts";
import type { Timestamp } from "../transport/protocol.ts";
import { snoozeTimes } from "./snooze-times.ts";
import { clockOf } from "./when.ts";

/** The menu's rows for `now` in `zone`, in the language: its clock times, Tomorrow's and Monday's. */
export function snoozeChoices(
  now: Timestamp,
  zone: string,
  lang: Lang,
  onPick: (until: Timestamp) => void,
): SnoozeChoice[] {
  return snoozeTimes(now, zone).map(({ key, until }): SnoozeChoice => {
    const time = clock(lang, clockOf(until, now, zone, lang).time);
    const pick = () => onPick(until);
    if (key === "tomorrow")
      return { key, label: twl(lang, "Tomorrow {time}", { time }), onPick: pick };
    if (key === "monday") return { key, label: twl(lang, "Monday {time}", { time }), onPick: pick };
    return { key, label: time, onPick: pick };
  });
}
