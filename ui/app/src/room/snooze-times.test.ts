// local-time.ts, snooze-times.ts and sweep.ts, pure: the sweep's day start and its 05:00 in a zone other than UTC, the
// wall clock across a change to summer time, and the snooze menu's times at 4:59 pm, 5:00 pm and 9:00 pm, Monday's,
// and Before the next date's.
import { expect, test } from "vitest";
import { atLocal, wallOf } from "./local-time.ts";
import { pickedAt, pickerValue, snoozeTimes } from "./snooze-times.ts";
import { sweepStart } from "./sweep.ts";
import { dayOf } from "./when.ts";

const KOLKATA = "Asia/Kolkata";
/** Tuesday 3 November 2026, midnight in India: 18:30 UTC the day before. */
const INDIA_MIDNIGHT = Date.UTC(2026, 10, 2, 18, 30);
const MIN = 60_000;
const HOUR = 60 * MIN;

test("the sweep in India: nothing before 05:00 there, then that day's midnight there", () => {
  expect(sweepStart(INDIA_MIDNIGHT + 5 * HOUR - MIN, KOLKATA)).toBeNull();
  expect(sweepStart(INDIA_MIDNIGHT + 5 * HOUR, KOLKATA)).toBe(INDIA_MIDNIGHT);
  expect(sweepStart(INDIA_MIDNIGHT + 23 * HOUR + 59 * MIN, KOLKATA)).toBe(INDIA_MIDNIGHT);
  // the same moments in UTC: 23:29 the day before, then 23:30, both past 05:00
  expect(sweepStart(INDIA_MIDNIGHT + 5 * HOUR - MIN, "UTC")).toBe(Date.UTC(2026, 10, 2));
});

test("the wall clock across London's change to summer time, 29 March 2026", () => {
  const day = dayOf(Date.UTC(2026, 2, 29, 12), "Europe/London");
  expect(atLocal(day, 0, "Europe/London")).toBe(Date.UTC(2026, 2, 29, 0, 0));
  // 9:00 am is BST, 8:00 UTC
  expect(atLocal(day, 9 * 60, "Europe/London")).toBe(Date.UTC(2026, 2, 29, 8, 0));
  expect(wallOf(Date.UTC(2026, 2, 29, 8, 0), "Europe/London")).toEqual({ day, minute: 540 });
});

const keys = (now: number, next?: string) =>
  snoozeTimes(now, KOLKATA, next).map((t) => [t.key, t.until]);
/** Tuesday 3 November 2026, h:m in India. */
const at = (h: number, m = 0) => INDIA_MIDNIGHT + h * HOUR + m * MIN;
const DAY = 24 * HOUR;

test("at 4:59 pm 6:00 pm today; from 5:00 pm 9:00 pm in its place; from 9:00 pm neither", () => {
  expect(keys(at(16, 59))).toEqual([
    ["evening", at(18)],
    ["tomorrow", at(9) + DAY],
    ["monday", at(9) + 6 * DAY],
  ]);
  expect(keys(at(17))[0]).toEqual(["night", at(21)]);
  expect(keys(at(21))[0]).toEqual(["tomorrow", at(9) + DAY]);
});

test("Monday is left out when tomorrow is Monday", () => {
  // Sunday 8 November
  expect(keys(at(10) + 5 * DAY).map(([k]) => k)).toEqual(["evening", "tomorrow"]);
});

test("Before the next date: 9:00 am the day before, only with the date more than a day away", () => {
  expect(keys(at(10), "2026-11-11").at(-1)).toEqual(["before", at(9) + 7 * DAY]);
  expect(keys(at(10), "2026-11-05").at(-1)).toEqual(["before", at(9) + DAY]);
  expect(keys(at(10), "2026-11-04").map(([k]) => k)).not.toContain("before");
  expect(keys(at(10), "2026-11-01").map(([k]) => k)).not.toContain("before");
  // past the host's 366 days
  expect(keys(at(10), "2027-12-01").map(([k]) => k)).not.toContain("before");
});

test("the picker's value is the zone's wall clock both ways", () => {
  expect(pickerValue(at(17, 30), KOLKATA)).toBe("2026-11-03T17:30");
  expect(pickedAt("2026-11-03T17:30", KOLKATA)).toBe(at(17, 30));
  expect(pickedAt("2026-02-30T10:00", KOLKATA)).toBeNull();
  expect(pickedAt("", KOLKATA)).toBeNull();
});
