// when.ts: today in the lawyer's zone, a calendar date read, and the rail's clock.
import { expect, test } from "vitest";
import { clockOf, dayOf, isoDay } from "./when.ts";

const day = (iso: string) => {
  const d = isoDay(iso);
  if (d === null) throw new Error(iso);
  return d;
};

test("today is the lawyer's: late Sunday evening in UTC is already Monday in Kolkata", () => {
  // 20:00 UTC on Sunday 1 November is 01:30 on Monday 2 November in India
  const at = Date.UTC(2026, 10, 1, 20, 0);
  expect(dayOf(at, "UTC")).toBe(day("2026-11-01"));
  expect(dayOf(at, "Asia/Kolkata")).toBe(day("2026-11-02"));
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
