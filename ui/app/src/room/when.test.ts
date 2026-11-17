// when.ts: today in the lawyer's zone, a calendar date read, and the rail's clock.
import { expect, test } from "vitest";
import { clockOf, dayOf, isoDay, stampOf } from "./when.ts";

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

// A tracked change's w:date is the author's clock time labelled Z (Word and stetdoc write it so), not an instant.
test("stampOf keeps a tracked change's clock time: 12:31 written in Kolkata reads 12:31 pm there, today", () => {
  const now = Date.UTC(2026, 10, 17, 8, 0); // Tuesday 17 November, 13:30 in Kolkata
  const at = Date.UTC(2026, 10, 17, 12, 31, 22); // w:date 2026-11-17T12:31:22Z
  expect(stampOf(at, now, "Asia/Kolkata", "en")).toEqual({ time: "12:31 pm", day: "" });
});

test("stampOf: a change late last evening reads as yesterday's weekday once the local date has moved on", () => {
  const now = Date.UTC(2026, 10, 17, 4, 0); // Tuesday 17 November, 09:30 in Kolkata
  const at = Date.UTC(2026, 10, 16, 23, 30); // w:date 2026-11-16T23:30:00Z, Monday evening
  expect(stampOf(at, now, "Asia/Kolkata", "en")).toEqual({ time: "11:30 pm", day: "Monday" });
  expect(stampOf(at, now, "Asia/Kolkata", "hi").day).toBe("सोमवार");
});

test("stampOf in New York across midnight compares the change's date with the local date", () => {
  // 22:00 on Monday 16 November in New York; a change written at 02:00 on Tuesday 17 (in London, say) is Tuesday's
  const monday = Date.UTC(2026, 10, 17, 3, 0);
  expect(stampOf(Date.UTC(2026, 10, 17, 2, 0), monday, "America/New_York", "en")).toEqual({
    time: "2:00 am",
    day: "Tuesday",
  });
  // just after midnight, Tuesday 17 in New York; a change written at 23:30 on Monday 16 is Monday's
  const tuesday = Date.UTC(2026, 10, 17, 5, 0);
  expect(stampOf(Date.UTC(2026, 10, 16, 23, 30), tuesday, "America/New_York", "en")).toEqual({
    time: "11:30 pm",
    day: "Monday",
  });
});

test("stampOf across daylight saving: the clock time stays as written, even one New York skips", () => {
  // Sunday 8 March 2026, 11:00 in New York (EDT, after 02:00 jumped to 03:00)
  const now = Date.UTC(2026, 2, 8, 15, 0);
  expect(stampOf(Date.UTC(2026, 2, 8, 2, 30), now, "America/New_York", "en")).toEqual({
    time: "2:30 am",
    day: "",
  });
  // 10:00 written in London in summer time (w:date …T10:00:00Z) reads 10:00 am on a Mac in India, as Word shows it
  const july = Date.UTC(2026, 6, 1, 6, 0); // Wednesday 1 July, 11:30 in Kolkata
  expect(stampOf(Date.UTC(2026, 6, 1, 10, 0), july, "Asia/Kolkata", "en")).toEqual({
    time: "10:00 am",
    day: "",
  });
});
