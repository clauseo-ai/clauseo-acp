// The stop's own words (U12), in the thread's chunk with the stop's row (stop-rows.tsx): the window's line under a plan
// stop's title. Its words are words-limits-thread.ts's, which the first screen never loads.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { limThread } from "@clauseo/components/lib/words-limits-thread.ts";
import type { Timestamp } from "../../transport/protocol.ts";
import { whenOf } from "./clock.ts";
import { resetPassed } from "./stops.ts";

/** The window's line under a plan stop's title, "The 5-hour limit. It resets at 6:10 pm.", for the two windows it
 * names (Claude's `five_hour` and `seven_day`); null for another window, or no reset, or one gone by. */
export function windowLine(
  lang: Lang,
  window: string | undefined,
  resets: Timestamp | undefined,
  now: number,
): string | null {
  if (resets === undefined || resetPassed(resets, now)) return null;
  const { day, time } = whenOf(lang, resets, now);
  if (window === "five_hour")
    return day === null
      ? limThread(lang, "The 5-hour limit. It resets at {time}.", { time })
      : limThread(lang, "The 5-hour limit. It resets on {day} at {time}.", { day, time });
  if (window === "seven_day")
    return day === null
      ? limThread(lang, "The weekly limit. It resets at {time}.", { time })
      : limThread(lang, "The weekly limit. It resets on {day} at {time}.", { day, time });
  return null;
}
