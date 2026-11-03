// The thread's clock times: the lawyer's own zone, "4:31 pm" today, the weekday within six days either way, the day and
// month further off (U2's clockOf rule), Hindi through clock().
import { clock } from "@clauseo/components/lib/words.ts";
import { expect, test, vi } from "vitest";
import { clockTime, clockWidest, isoOf, stamp, whenOf } from "./clock.ts";

// local times, so the checks hold in any zone the tests run in (the suite pins its zone to UTC: vite.config.ts)
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

test("the weekday shows exactly when the instant is on another local day, read in Asia/Kolkata", async () => {
  // the zone is set inside this test (Node honours a runtime change) and restored after
  const zone = process.env["TZ"];
  process.env["TZ"] = "Asia/Kolkata";
  try {
    // a fresh copy: clock.ts caches its Intl formats, and a format keeps the zone it was made in
    vi.resetModules();
    const inKolkata = await import("./clock.ts");
    const now = Date.UTC(2026, 10, 2, 6, 30); // Monday 2 November, 12:00 in Kolkata
    // 16:31 on the same day: the time alone
    expect(inKolkata.stamp("en", Date.UTC(2026, 10, 2, 11, 1), now)).toBe("4:31 pm");
    // 00:00 on Tuesday 3 November in Kolkata (18:30 UTC on the 2nd): a different local day
    expect(inKolkata.stamp("en", Date.UTC(2026, 10, 2, 18, 30), now)).toBe("Tuesday 12:00 am");
    // 23:59 on Sunday 1 November in Kolkata (18:29 UTC on the 1st): the day before
    expect(inKolkata.stamp("en", Date.UTC(2026, 10, 1, 18, 29), now)).toBe("Sunday 11:59 pm");
  } finally {
    if (zone === undefined) delete process.env["TZ"];
    else process.env["TZ"] = zone;
    vi.resetModules();
  }
});

test("the widest clock time of each form the language has, a two-digit hour, for the meta line's kept time", () => {
  expect(clockWidest("en")).toEqual(["10:00 am", "10:00 pm"]);
  const hi = clockWidest("hi");
  expect(hi.length).toBeGreaterThan(1);
  for (const form of hi) expect(form).toMatch(/10:00/);
  // every time today has one of these forms
  for (let h = 0; h < 24; h += 1) {
    const t = clockTime("hi", new Date(2000, 0, 1, h, 7).getTime());
    expect(hi).toContain(t.replace(/\d{1,2}:\d\d/, "10:00"));
  }
});
