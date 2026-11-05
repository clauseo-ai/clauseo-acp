// recent.ts, pure: Recent's day headings in the lawyer's zone (at 23:59 and 00:01, in India and in UTC), and its list:
// every conversation not archived, the scratch matter's too, newest activity first.
import { expect, test } from "vitest";
import type { ThreadSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed } from "./fake-next.ts";
import { dayHeading, madeAt, recent } from "./recent.ts";

const INDIA = "Asia/Kolkata";
/** A moment in India: 5:30 ahead of UTC. */
const india = (y: number, mo: number, d: number, h: number, mi: number) =>
  Date.UTC(y, mo - 1, d, h, mi) - 330 * 60_000;

test("at 00:01, a minute after midnight is today and two minutes before it yesterday", () => {
  const now = india(2026, 11, 3, 0, 1);
  expect(dayHeading(india(2026, 11, 3, 0, 0), now, INDIA, "en")).toBe("Today");
  expect(dayHeading(india(2026, 11, 2, 23, 59), now, INDIA, "en")).toBe("Yesterday");
  expect(dayHeading(india(2026, 11, 1, 23, 59), now, INDIA, "en")).toBe("Sunday 1 November");
});

test("at 23:59, the day's first minute is still today; the day before is yesterday", () => {
  const now = india(2026, 11, 3, 23, 59);
  expect(dayHeading(india(2026, 11, 3, 0, 1), now, INDIA, "en")).toBe("Today");
  expect(dayHeading(india(2026, 11, 2, 23, 59), now, INDIA, "en")).toBe("Yesterday");
  expect(dayHeading(india(2026, 10, 27, 12, 0), now, INDIA, "en")).toBe("Tuesday 27 October");
});

test("the zone decides the day: the same moments read apart in India and in UTC", () => {
  // 00:01 in India on 3 November is 18:31 UTC on 2 November
  const now = india(2026, 11, 3, 0, 1);
  const before = india(2026, 11, 2, 23, 0);
  expect(dayHeading(before, now, INDIA, "en")).toBe("Yesterday");
  expect(dayHeading(before, now, "UTC", "en")).toBe("Today");
  expect(dayHeading(before, now, "America/New_York", "en")).toBe("Today");
});

test("every conversation not archived, the scratch matter's too, newest first under its day", () => {
  const now = india(2026, 11, 3, 18, 0);
  const a = detailed({ matter: uuid(1, 2), name: "Acme", kind: "folder" });
  const s = detailed({ matter: uuid(9, 2), name: "Scratch", kind: "scratch" });
  const t = (n: number, at: number, over: Partial<ThreadSummary> = {}): ThreadSummary => ({
    thread: uuid(n, 3),
    matter: n < 5 ? a.matter : s.matter,
    agent: "claude-code",
    status: "idle",
    last_activity: at,
    ...over,
  });
  const lists = new Map([
    [
      a.matter,
      [t(1, now - 60_000), t(2, india(2026, 11, 2, 9, 0)), t(3, now, { status: "archived" })],
    ],
    [s.matter, [t(5, now - 30_000), t(6, india(2026, 10, 30, 9, 0))]],
  ]);
  const groups = recent(lists, [a, s], now, INDIA, "en");
  expect(groups.map((g) => [g.day, g.items.map((i) => i.thread.thread)])).toEqual([
    ["Today", [uuid(5, 3), uuid(1, 3)]],
    ["Yesterday", [uuid(2, 3)]],
    ["Friday 30 October", [uuid(6, 3)]],
  ]);
});

test("a thread with no activity yet takes the moment its v7 id was made", () => {
  expect(madeAt("019a4d2c-1b00-7000-8000-000000000000")).toBe(0x019a4d2c1b00);
  expect(madeAt("not-an-id")).toBe(0);
});
