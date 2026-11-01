// The thread's clock times: a host Timestamp (ms since the epoch, UTC) on the lawyer's own clock, in the computer's
// time zone. "4:31 pm" today, the weekday before it on another day ("Monday 4:31 pm"), and Hindi through words.ts's
// clock(), which puts the part of the day first. Never "min ago": nothing here runs a timer to keep such words true.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import type { Timestamp } from "../../transport/protocol.ts";

const LOCALE = { en: "en-GB", hi: "hi-IN" } as const satisfies Record<Lang, string>;

const weekdays = new Map<Lang, Intl.DateTimeFormat>();
function weekdayOf(lang: Lang, at: Date): string {
  let format = weekdays.get(lang);
  if (format === undefined) {
    format = new Intl.DateTimeFormat(LOCALE[lang], { weekday: "long" });
    weekdays.set(lang, format);
  }
  return format.format(at);
}

const sameDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() &&
  a.getMonth() === b.getMonth() &&
  a.getDate() === b.getDate();

/** A time as the clock shows it: "4:31 pm", or in Hindi "शाम 4:31". */
export function clockTime(lang: Lang, ts: Timestamp): string {
  const at = new Date(ts);
  const h = at.getHours();
  const minutes = String(at.getMinutes()).padStart(2, "0");
  return clock(lang, `${h % 12 === 0 ? 12 : h % 12}:${minutes} ${h < 12 ? "am" : "pm"}`);
}

/** A time and, when it falls on another day than `now`, that day's weekday; null today. */
export function whenOf(
  lang: Lang,
  ts: Timestamp,
  now: number,
): { day: string | null; time: string } {
  const at = new Date(ts);
  return {
    day: sameDay(at, new Date(now)) ? null : weekdayOf(lang, at),
    time: clockTime(lang, ts),
  };
}

/** One line's time: "4:31 pm" today, "Monday 4:31 pm" on another day. */
export function stamp(lang: Lang, ts: Timestamp, now: number): string {
  const { day, time } = whenOf(lang, ts, now);
  return day === null ? time : `${day} ${time}`;
}

/** The ISO form for a `<time dateTime>`, in UTC. */
export const isoOf = (ts: Timestamp): string => new Date(ts).toISOString();
