// when.ts: date groups across a Sunday, today in the lawyer's zone, the order inside a group, and the rail's clock.
import { expect, test } from "vitest";
import { byWhen, clockOf, dayOf, groupMatters, groupOf, isoDay } from "./when.ts";

const day = (iso: string) => {
  const d = isoDay(iso);
  if (d === null) throw new Error(iso);
  return d;
};

test("groups from a Thursday: tomorrow, this week to Sunday, next week to the Sunday after, later, and none", () => {
  const thu = day("2026-10-29");
  expect(groupOf("2026-10-30", thu)).toBe("Tomorrow");
  expect(groupOf("2026-10-29", thu)).toBe("This week");
  expect(groupOf("2026-10-31", thu)).toBe("This week");
  expect(groupOf("2026-11-01", thu)).toBe("This week");
  // Monday after the Sunday
  expect(groupOf("2026-11-02", thu)).toBe("Next week");
  expect(groupOf("2026-11-08", thu)).toBe("Next week");
  expect(groupOf("2026-11-09", thu)).toBe("Later");
  expect(groupOf("2026-10-28", thu)).toBe("No date yet");
  expect(groupOf(undefined, thu)).toBe("No date yet");
  expect(groupOf("30 October 2026", thu)).toBe("No date yet");
  expect(groupOf("2026-02-30", thu)).toBe("No date yet");
});

test("groups across a Sunday: on the Sunday its week ends today, and Monday is tomorrow and next week's", () => {
  const sun = day("2026-11-01");
  expect(groupOf("2026-11-01", sun)).toBe("This week");
  expect(groupOf("2026-11-02", sun)).toBe("Tomorrow");
  expect(groupOf("2026-11-03", sun)).toBe("Next week");
  expect(groupOf("2026-11-08", sun)).toBe("Next week");
  expect(groupOf("2026-11-09", sun)).toBe("Later");
  const sat = day("2026-10-31");
  expect(groupOf("2026-11-01", sat)).toBe("Tomorrow");
  expect(groupOf("2026-11-02", sat)).toBe("Next week");
  expect(groupOf("2026-11-08", sat)).toBe("Next week");
  expect(groupOf("2026-11-09", sat)).toBe("Later");
});

test("today is the lawyer's: late Sunday evening in UTC is already Monday in Kolkata", () => {
  // 20:00 UTC on Sunday 1 November is 01:30 on Monday 2 November in India
  const at = Date.UTC(2026, 10, 1, 20, 0);
  expect(dayOf(at, "UTC")).toBe(day("2026-11-01"));
  expect(dayOf(at, "Asia/Kolkata")).toBe(day("2026-11-02"));
  expect(groupOf("2026-11-02", dayOf(at, "UTC"))).toBe("Tomorrow");
  expect(groupOf("2026-11-02", dayOf(at, "Asia/Kolkata"))).toBe("This week");
});

test("groupMatters keeps the rail's order, draws no empty group, and orders a group by date, activity, then id", () => {
  const thu = day("2026-10-29");
  const groups = groupMatters(
    [
      { matter: "a1", next_date: "2026-11-20" },
      { matter: "a2" },
      { matter: "a3", next_date: "2026-10-31", last_activity: 5 },
      { matter: "a4", next_date: "2026-10-30" },
      { matter: "a5", next_date: "2026-10-31", last_activity: 9 },
      { matter: "a6" },
      { matter: "a7", next_date: "2026-10-29" },
    ],
    thu,
  );
  expect(groups.map((g) => [g.when, g.matters.map((m) => m.matter)])).toEqual([
    ["Tomorrow", ["a4"]],
    ["This week", ["a7", "a5", "a3"]],
    ["Later", ["a1"]],
    ["No date yet", ["a6", "a2"]],
  ]);
  expect(byWhen({ matter: "x", next_date: "2026-10-30" }, { matter: "y" })).toBeLessThan(0);
});

test("clockOf says the clock time in the zone, and another day by its weekday or, further off, its date", () => {
  const now = Date.UTC(2026, 10, 2, 6, 0); // Monday 2 November, 11:30 in Kolkata
  const at = Date.UTC(2026, 10, 2, 11, 1);
  expect(clockOf(at, now, "Asia/Kolkata", "en")).toEqual({ time: "4:31 pm", day: "" });
  expect(clockOf(at, now, "UTC", "en")).toEqual({ time: "11:01 am", day: "" });
  const sunday = Date.UTC(2026, 10, 1, 3, 30);
  expect(clockOf(sunday, now, "Asia/Kolkata", "en")).toEqual({ time: "9:00 am", day: "Sunday" });
  expect(clockOf(sunday, now, "Asia/Kolkata", "hi").day).toBe("रविवार");
  const tuesday = Date.UTC(2026, 10, 3, 9, 0);
  expect(clockOf(tuesday, now, "Asia/Kolkata", "en")).toEqual({ time: "2:30 pm", day: "Tuesday" });
  expect(clockOf(Date.UTC(2026, 9, 20, 9, 0), now, "Asia/Kolkata", "en").day).toBe("20 October");
});
