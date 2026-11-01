// The rail's sense of time, pure: which date group a matter's next date falls in, the order inside a group, and a
// moment as the rail says it ("4:31 pm", with its day when it isn't today). Groups follow the design's whenOf
// (design/03-system/src/pages/first-launch/flow.ts): Tomorrow, This week (to Sunday), Next week (to the Sunday after),
// Later, and No date yet for none or a date gone by. Today and every clock time are taken in the lawyer's zone, given
// with `now` as inputs, so nothing here reads the clock.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { MatterId, Timestamp } from "../transport/protocol.ts";

/** The date groups, in the rail's order. English keys: the rail puts them in the language. */
export const WHEN = ["Tomorrow", "This week", "Next week", "Later", "No date yet"] as const;
export type When = (typeof WHEN)[number];

/** A calendar day as days since 1970-01-01, so two days subtract. */
export type Day = number;

const DAY_MS = 86_400_000;

const formats = new Map<string, Intl.DateTimeFormat>();
/** One formatter per locale, zone and options, made once: making them is the slow part. */
function format(locale: string, zone: string, options: Intl.DateTimeFormatOptions) {
  const key = `${locale}|${zone}|${JSON.stringify(options)}`;
  let made = formats.get(key);
  if (made === undefined) {
    made = new Intl.DateTimeFormat(locale, { ...options, timeZone: zone });
    formats.set(key, made);
  }
  return made;
}

/** The lawyer's zone, as the computer gives it. */
export const localZone = (): string => Intl.DateTimeFormat().resolvedOptions().timeZone;

/** The calendar day a moment falls on in the zone. */
export function dayOf(at: Timestamp, zone: string): Day {
  const parts = format("en-US", zone, { year: "numeric", month: "numeric", day: "numeric" })
    .formatToParts(at)
    .reduce<Record<string, number>>((acc, p) => {
      if (p.type !== "literal") acc[p.type] = Number(p.value);
      return acc;
    }, {});
  return Date.UTC(parts["year"] ?? 1970, (parts["month"] ?? 1) - 1, parts["day"] ?? 1) / DAY_MS;
}

/** An ISO calendar date, "2026-10-30", as a Day; null when it isn't one. */
export function isoDay(iso: string): Day | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (m === null) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]) - 1, Number(m[3])];
  const ms = Date.UTC(y, mo, d);
  const back = new Date(ms);
  // 2026-02-30 is no date
  if (back.getUTCFullYear() !== y || back.getUTCMonth() !== mo || back.getUTCDate() !== d)
    return null;
  return ms / DAY_MS;
}

/** The group a next date puts its matter in, on `today`: whenOf's rule. */
export function groupOf(nextDate: string | undefined, today: Day): When {
  const day = nextDate === undefined ? null : isoDay(nextDate);
  if (day === null) return "No date yet";
  const days = day - today;
  // Sunday is 0: this week runs to Sunday
  const toSunday = (7 - new Date(today * DAY_MS).getUTCDay()) % 7;
  if (days === 1) return "Tomorrow";
  if (days >= 0 && days <= toSunday) return "This week";
  if (days > toSunday && days <= toSunday + 7) return "Next week";
  return days < 0 ? "No date yet" : "Later";
}

/** What a matter brings to its group's order. */
export type Dated = { matter: MatterId; next_date?: string; last_activity?: Timestamp };

/** The order inside a group: the earlier next date first, then the latest activity, then the newest (v7 ids sort by
 * creation). */
export function byWhen(a: Dated, b: Dated): number {
  const da = a.next_date === undefined ? null : isoDay(a.next_date);
  const db = b.next_date === undefined ? null : isoDay(b.next_date);
  if (da !== db) {
    if (da === null) return 1;
    if (db === null) return -1;
    return da - db;
  }
  const la = a.last_activity ?? -Infinity;
  const lb = b.last_activity ?? -Infinity;
  if (la !== lb) return lb - la;
  return a.matter < b.matter ? 1 : a.matter > b.matter ? -1 : 0;
}

/** The matters in their date groups, in the rail's order, each group in byWhen's order; no empty group. */
export function groupMatters<M extends Dated>(
  matters: readonly M[],
  today: Day,
): { when: When; matters: M[] }[] {
  const groups = new Map<When, M[]>();
  for (const m of matters) {
    const when = groupOf(m.next_date, today);
    const list = groups.get(when);
    if (list === undefined) groups.set(when, [m]);
    else list.push(m);
  }
  return WHEN.flatMap((when) => {
    const list = groups.get(when);
    return list === undefined ? [] : [{ when, matters: list.sort(byWhen) }];
  });
}

/** A moment as the rail says it: `time` the clock time in English, "4:31 pm" (the rail's words put it in the
 * language), and `day`, in the language, when it falls on another day than `now`: its weekday within six days either
 * way, else its date, "28 October"; "" on the same day. */
export function clockOf(
  at: Timestamp,
  now: Timestamp,
  zone: string,
  lang: Lang,
): { time: string; day: string } {
  const parts = format("en-US", zone, {
    hour: "numeric",
    minute: "2-digit",
    hour12: true,
  }).formatToParts(at);
  const part = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  const time = `${part("hour")}:${part("minute")} ${part("dayPeriod").toLowerCase()}`;
  const apart = dayOf(at, zone) - dayOf(now, zone);
  if (apart === 0) return { time, day: "" };
  const locale = lang === "hi" ? "hi-IN" : "en-GB";
  const day =
    Math.abs(apart) <= 6
      ? format(locale, zone, { weekday: "long" }).format(at)
      : format(locale, zone, { day: "numeric", month: "long" }).format(at);
  return { time, day };
}
