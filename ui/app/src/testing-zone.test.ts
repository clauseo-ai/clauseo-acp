// The suite's zone is UTC whatever the shell's TZ (vite.config.ts, test.env): run this with TZ=Asia/Kolkata and it still passes.
import { expect, test } from "vitest";

test("the app's tests run in UTC", () => {
  expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("UTC");
  expect(new Date(0).getTimezoneOffset()).toBe(0);
  expect(new Date(2026, 10, 3, 16, 32).toISOString()).toBe("2026-11-03T16:32:00.000Z");
});
