// The snooze menu's times, pure (U16b), on the lawyer's wall clock in her zone: 6:00 pm today until 5:00 pm, then
// 9:00 pm in its place (none from 9:00 pm); 9:00 am tomorrow; 9:00 am on Monday, unless tomorrow is Monday; and 9:00
// am on the day before the matter's next date, where that date is more than a day away. None past the host's 366 days.
import type { Timestamp } from "../transport/protocol.ts";
import { atLocal, wallOf } from "./local-time.ts";
import { isoDay } from "./when.ts";

export type SnoozeTime = {
  key: "evening" | "night" | "tomorrow" | "monday" | "before";
  until: Timestamp;
};

const DAY_MS = 86_400_000;
/** The host refuses a snooze further ahead (MAX_SNOOZE_AHEAD_MS). */
export const MOST_AHEAD = 366 * DAY_MS;
const H = 60;

export function snoozeTimes(now: Timestamp, zone: string, nextDate?: string): SnoozeTime[] {
  const { day, minute } = wallOf(now, zone);
  const times: SnoozeTime[] = [];
  if (minute < 17 * H) times.push({ key: "evening", until: atLocal(day, 18 * H, zone) });
  else if (minute < 21 * H) times.push({ key: "night", until: atLocal(day, 21 * H, zone) });
  times.push({ key: "tomorrow", until: atLocal(day + 1, 9 * H, zone) });
  // 1 January 1970 was a Thursday: day 0's weekday is 4, Sunday 0
  const toMonday = (8 - ((day + 4) % 7)) % 7 || 7;
  if (toMonday > 1) times.push({ key: "monday", until: atLocal(day + toMonday, 9 * H, zone) });
  const next = nextDate === undefined ? null : isoDay(nextDate);
  if (next !== null && next - day > 1) {
    const until = atLocal(next - 1, 9 * H, zone);
    if (until - now <= MOST_AHEAD) times.push({ key: "before", until });
  }
  return times;
}

/** The picker's value ("2026-11-05T17:30", the zone's wall clock) as a moment; null if it isn't one. */
export function pickedAt(value: string, zone: string): Timestamp | null {
  const m = /^(\d{4}-\d{2}-\d{2})T(\d{2}):(\d{2})$/.exec(value);
  const day = m === null ? null : isoDay(m[1] ?? "");
  if (m === null || day === null) return null;
  return atLocal(day, Number(m[2]) * H + Number(m[3]), zone);
}

/** A moment as the picker's value, on the zone's wall clock. */
export function pickerValue(at: Timestamp, zone: string): string {
  const { day, minute } = wallOf(at, zone);
  const date = new Date(day * DAY_MS).toISOString().slice(0, 10);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date}T${pad(Math.floor(minute / H))}:${pad(minute % H)}`;
}
