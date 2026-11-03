// The thread's clock times: a host Timestamp (ms since the epoch, UTC) on the lawyer's own clock, in the computer's
// time zone. "4:31 pm" today, the weekday before it within six days either way ("Monday 4:31 pm"), the day and month
// further off, past or later ("18 October 4:31 pm"): U2's rule, room/when.ts clockOf, which one helper should take on
// when both land. Hindi through words.ts's clock(), which puts the part of the day first. Never "min ago": nothing here runs a timer to keep such words true.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import type { Timestamp } from "../../transport/protocol.ts";

const LOCALE = { en: "en-GB", hi: "hi-IN" } as const satisfies Record<Lang, string>;

type DayForm = "weekday" | "date";
const OPTIONS = {
  weekday: { weekday: "long" },
  date: { day: "numeric", month: "long" },
} as const satisfies Record<DayForm, Intl.DateTimeFormatOptions>;

const formats = new Map<string, Intl.DateTimeFormat>();
function dayOf(lang: Lang, form: DayForm, at: Date): string {
  const key = `${lang} ${form}`;
  let format = formats.get(key);
  if (format === undefined) {
    format = new Intl.DateTimeFormat(LOCALE[lang], OPTIONS[form]);
    formats.set(key, format);
  }
  return format.format(at);
}

/** Calendar days from `now`'s day to `at`'s, on the local clock: 0 today, 1 tomorrow, -1 yesterday. */
function daysBetween(now: Date, at: Date): number {
  const start = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  const end = Date.UTC(at.getFullYear(), at.getMonth(), at.getDate());
  return Math.round((end - start) / 86_400_000);
}

/** A time as the clock shows it: "4:31 pm", or in Hindi "शाम 4:31". */
export function clockTime(lang: Lang, ts: Timestamp): string {
  const at = new Date(ts);
  const h = at.getHours();
  const minutes = String(at.getMinutes()).padStart(2, "0");
  return clock(lang, `${h % 12 === 0 ? 12 : h % 12}:${minutes} ${h < 12 ? "am" : "pm"}`);
}

const widest = new Map<Lang, readonly string[]>();
/** A clock time of each form the language has, with a two-digit hour ("10:00 pm", "शाम 10:00"): one of them is as wide
 * as any time today, its digits all one width (tabular). For the meta line, whose time's place is kept that wide before
 * the turn's time comes and after. */
export function clockWidest(lang: Lang): readonly string[] {
  let out = widest.get(lang);
  if (out === undefined) {
    const forms = new Set<string>();
    for (let h = 0; h < 24; h += 1)
      forms.add(
        clockTime(lang, new Date(2000, 0, 1, h, 0).getTime()).replace(/\d{1,2}:\d\d/, "10:00"),
      );
    out = [...forms];
    widest.set(lang, out);
  }
  return out;
}

/** A time and its day: null today, the weekday within six days either way, the day and month further off. */
export function whenOf(
  lang: Lang,
  ts: Timestamp,
  now: number,
): { day: string | null; time: string } {
  const at = new Date(ts);
  const days = daysBetween(new Date(now), at);
  return {
    day: days === 0 ? null : dayOf(lang, Math.abs(days) <= 6 ? "weekday" : "date", at),
    time: clockTime(lang, ts),
  };
}

/** One line's time: "4:31 pm" today, "Monday 4:31 pm" within six days, "18 October 4:31 pm" further off. */
export function stamp(lang: Lang, ts: Timestamp, now: number): string {
  const { day, time } = whenOf(lang, ts, now);
  return day === null ? time : `${day} ${time}`;
}

/** The ISO form for a `<time dateTime>`, in UTC. */
export const isoOf = (ts: Timestamp): string => new Date(ts).toISOString();
