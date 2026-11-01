// The thread's clock times: the lawyer's own zone, "4:31 pm" today, the weekday within six days either way, the day and
// month further off (U2's clockOf rule), Hindi through clock().
import { clock } from "@clauseo/components/lib/words.ts";
import { expect, test } from "vitest";
import { clockTime, isoOf, stamp, whenOf } from "./clock.ts";

// local times, so the checks hold in any zone the tests run in
const at = (day: number, h: number, m: number) => new Date(2026, 10, day, h, m).getTime();
const NOW = at(2, 12, 0);

test("a time reads as the clock shows it, in twelve hours with am or pm", () => {
  expect(clockTime("en", at(2, 16, 31))).toBe("4:31 pm");
  expect(clockTime("en", at(2, 0, 5))).toBe("12:05 am");
  expect(clockTime("en", at(2, 12, 0))).toBe("12:00 pm");
  expect(clockTime("en", at(2, 9, 7))).toBe("9:07 am");
});

test("Hindi puts the part of the day first, through words.ts's clock()", () => {
  expect(clockTime("hi", at(2, 16, 31))).toBe(clock("hi", "4:31 pm"));
  expect(clockTime("hi", at(2, 16, 31))).not.toContain("pm");
  expect(clockTime("hi", at(2, 9, 7))).toBe(clock("hi", "9:07 am"));
});

test("today has no day; within six days either way the weekday; further off, past or later, the day and month", () => {
  expect(whenOf("en", at(2, 23, 59), NOW)).toEqual({ day: null, time: "11:59 pm" });
  expect(whenOf("en", at(2, 0, 0), NOW).day).toBeNull();
  // 3 November 2026 is a Tuesday, 8 November a Sunday, 1 November a Sunday, 27 October a Tuesday
  expect(whenOf("en", at(3, 0, 0), NOW)).toEqual({ day: "Tuesday", time: "12:00 am" });
  expect(stamp("en", at(8, 8, 0), NOW)).toBe("Sunday 8:00 am");
  expect(stamp("en", at(9, 8, 0), NOW)).toBe("9 November 8:00 am");
  expect(stamp("en", at(1, 16, 31), NOW)).toBe("Sunday 4:31 pm");
  expect(stamp("en", new Date(2026, 9, 27, 8, 0).getTime(), NOW)).toBe("Tuesday 8:00 am");
  expect(stamp("en", new Date(2026, 9, 26, 8, 0).getTime(), NOW)).toBe("26 October 8:00 am");
  expect(stamp("en", new Date(2026, 9, 18, 8, 0).getTime(), NOW)).toBe("18 October 8:00 am");
  expect(stamp("en", at(2, 16, 31), NOW)).toBe("4:31 pm");
  expect(whenOf("hi", at(3, 9, 0), NOW).day).toBe("मंगलवार");
  const hiDate = new Intl.DateTimeFormat("hi-IN", { day: "numeric", month: "long" });
  expect(whenOf("hi", new Date(2026, 9, 18, 9, 0).getTime(), NOW).day).toBe(
    hiDate.format(new Date(2026, 9, 18)),
  );
  expect(whenOf("hi", at(9, 9, 0), NOW).day).toBe(hiDate.format(new Date(2026, 10, 9)));
});

test("six days count calendar days, not hours, both ways", () => {
  expect(whenOf("en", at(8, 23, 59), at(2, 0, 1)).day).toBe("Sunday");
  expect(whenOf("en", at(9, 0, 1), at(2, 23, 59)).day).toBe("9 November");
  expect(whenOf("en", new Date(2026, 9, 27, 0, 1).getTime(), at(2, 23, 59)).day).toBe("Tuesday");
  expect(whenOf("en", new Date(2026, 9, 26, 23, 59).getTime(), at(2, 0, 1)).day).toBe("26 October");
});

test("never a relative time", () => {
  expect(stamp("en", NOW - 60_000, NOW)).not.toMatch(/ago|min/);
});

test("the ISO form is UTC", () => {
  expect(isoOf(Date.UTC(2026, 10, 2, 11, 1))).toBe("2026-11-02T11:01:00.000Z");
});
