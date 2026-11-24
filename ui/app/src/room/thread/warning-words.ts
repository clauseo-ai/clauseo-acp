// The composer's warning in words (U12), from the key limits/plan-model.ts's warningKey makes: the share used of the
// window, then its reset and what happens if it runs out mid-answer. In the thread's chunk with the note that says it
// (limit-warning-note.tsx), so the first load carries neither.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { limThread } from "@clauseo/components/lib/words-limits-thread.ts";
import type { Warning } from "../limits/plan-model.ts";
import type { Timestamp } from "../../transport/protocol.ts";
import { whenOf } from "./clock.ts";

/** The warning's two sentences from its key, `name` the plan's (Claude, ChatGPT): the share used, then the reset
 * with what a pause means (`at` null without a reset: the pause then said with no time); null for no warning. */
export function warningWords(
  lang: Lang,
  key: string,
  name: string,
  now: number,
): { lead: string; until: { text: string; at: Timestamp | null } } | null {
  if (key === "") return null;
  const { window, used, resets } = JSON.parse(key) as Warning;
  const lead =
    window === "five_hour"
      ? limThread(lang, "You've used {n}% of your 5-hour {name} limit.", { n: used, name })
      : window === "weekly"
        ? limThread(lang, "You've used {n}% of your weekly {name} limit.", { n: used, name })
        : lim(lang, "Close to the limit.");
  if (resets === null)
    return {
      lead,
      until: {
        text: limThread(lang, "If it runs out mid-answer, the answer pauses until it resets."),
        at: null,
      },
    };
  const { day, time } = whenOf(lang, resets, now);
  const text =
    day === null
      ? limThread(
          lang,
          "It resets at {time}. If it runs out mid-answer, the answer pauses until then.",
          { time },
        )
      : limThread(
          lang,
          "It resets on {day} at {time}. If it runs out mid-answer, the answer pauses until then.",
          { day, time },
        );
  return { lead, until: { text, at: resets } };
}
