// The composer's warning in words (U12), from the key limits/plan-model.ts's warningKey makes: the window, then the
// reset. In the thread's chunk with the note that says it (limit-warning-note.tsx), so the first load carries neither.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { limThread } from "@clauseo/components/lib/words-limits-thread.ts";
import type { Timestamp } from "../../transport/protocol.ts";
import { whenOf } from "./clock.ts";

/** The warning's two sentences from its key: the window, then the reset (null without one); null for no warning. */
export function warningWords(
  lang: Lang,
  key: string,
  now: number,
): { lead: string; until: { text: string; at: Timestamp } | null } | null {
  if (key === "") return null;
  const [label, resets] = JSON.parse(key) as [string, Timestamp | null];
  const lead =
    label === "5-hour limit"
      ? limThread(lang, "Close to your 5-hour limit.")
      : lim(lang, "Close to the limit.");
  if (resets === null) return { lead, until: null };
  const { day, time } = whenOf(lang, resets, now);
  const text =
    day === null
      ? limThread(lang, "Long tasks may pause until {time}.", { time })
      : limThread(lang, "Long tasks may pause until {day} {time}.", { day, time });
  return { lead, until: { text, at: resets } };
}
