// The lawyer's wall clock, pure (U16b): a moment's day and minute of the day in a zone, and back from a day and a
// minute to the moment. For the morning sweep (local midnight, 05:00) and the snooze menu's times (6:00 pm, 9:00 am).
import type { Timestamp } from "../transport/protocol.ts";
import { format, type Day } from "./when.ts";

const DAY_MS = 86_400_000;

/** A moment on the zone's wall clock: its day, and the minutes since that day's midnight. */
export function wallOf(at: Timestamp, zone: string): { day: Day; minute: number } {
  const p: Record<string, number> = {};
  for (const part of format("en-US", zone, {
    year: "numeric",
    month: "numeric",
    day: "numeric",
    hour: "numeric",
    minute: "numeric",
    hourCycle: "h23",
  }).formatToParts(at))
    if (part.type !== "literal") p[part.type] = Number(part.value);
  const day = Date.UTC(p["year"] ?? 1970, (p["month"] ?? 1) - 1, p["day"] ?? 1) / DAY_MS;
  return { day, minute: ((p["hour"] ?? 0) % 24) * 60 + (p["minute"] ?? 0) };
}

/** How far the zone's wall clock runs ahead of UTC at `at`, in ms. */
const offsetAt = (at: Timestamp, zone: string) => {
  const w = wallOf(at, zone);
  return w.day * DAY_MS + w.minute * 60_000 - Math.floor(at / 60_000) * 60_000;
};

/** The moment the zone's wall clock reads `minute` on `day`: twice over, so a change of offset between the guess and
 * the answer (summer time) is taken into account. A time the clock skips (summer time starting at that hour, as at
 * midnight in Santiago on 6 September 2026) gives the first moment after the skip on that day, never the day before. */
export function atLocal(day: Day, minute: number, zone: string): Timestamp {
  const wall = day * DAY_MS + minute * 60_000;
  const first = wall - offsetAt(wall, zone);
  const second = wall - offsetAt(first, zone);
  const reads = wallOf(second, zone);
  return reads.day === day && reads.minute >= minute ? second : Math.max(first, second);
}
