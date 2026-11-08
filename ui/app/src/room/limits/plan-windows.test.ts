// A window's label by its model and length: a per-model 5-hour window (a ChatGPT plan's Spark bucket) reads
// "{model} 5-hour limit", a weekly one or one with no length "{model} weekly limit", one of another length the host's
// label; a window with no model keeps the host's label, translated when the card knows it.
import { loadHindi } from "@clauseo/components/lib/words-core.ts";
import { beforeAll, expect, test } from "vitest";
import { windowLabel } from "./plan-windows.ts";

beforeAll(() => loadHindi());

const spark = (duration_mins: number | undefined, label = "Spark usage limit") => ({
  label,
  model: "Spark",
  ...(duration_mins === undefined ? {} : { duration_mins }),
});

test("a per-model window is named by its length, in English and Hindi", () => {
  expect(windowLabel("en", spark(300))).toBe("Spark 5-hour limit");
  expect(windowLabel("en", spark(10080))).toBe("Spark weekly limit");
  expect(windowLabel("en", spark(undefined))).toBe("Spark weekly limit");
  expect(windowLabel("hi", spark(300))).toBe("Spark की 5 घंटे की सीमा");
  expect(windowLabel("hi", spark(10080))).toBe("Spark की साप्ताहिक सीमा");
});

test("a per-model window of another length keeps the host's label, and so does one with no model", () => {
  expect(windowLabel("en", spark(43200, "Spark monthly limit"))).toBe("Spark monthly limit");
  expect(windowLabel("hi", spark(4320, "Spark 3-day limit"))).toBe("Spark 3-day limit");
  expect(windowLabel("en", { label: "5-hour limit", duration_mins: 300 })).toBe("5-hour limit");
  expect(windowLabel("hi", { label: "5-hour limit", model: "", duration_mins: 300 })).toBe(
    "5 घंटे की सीमा",
  );
  expect(windowLabel("en", { label: "Plus monthly cap" })).toBe("Plus monthly cap");
});
