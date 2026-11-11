// D-072: a folder's counts under New matter's folder line, pure: KB under 1 MB, MB to one decimal under 10 MB, the
// empty folder's own line, and the same in Hindi with its grouping. The lines in the room are scan.test.tsx's.
import { loadHindi } from "@clauseo/components/lib/words-core.ts";
import { beforeAll, expect, test } from "vitest";
import type { FolderScanned } from "../../transport/protocol.ts";
import { countsLine, sizeOf } from "./scan-words.ts";

beforeAll(async () => {
  await loadHindi();
});

const scanned = (files: number, bytes: number, done = true): FolderScanned => ({
  folder: "ref-a",
  files,
  bytes,
  place: "local",
  cloud_only: 0,
  done,
  cancelled: false,
});

test("200 files of 6 KB say 6 KB, not 1 MB", () => {
  expect(countsLine("en", scanned(200, 6_000))).toBe("200 files, 6 KB");
  expect(countsLine("en", scanned(1, 6_000))).toBe("1 file, 6 KB");
});

test("an empty folder says it's empty once the scan is done, and nothing while it runs", () => {
  expect(countsLine("en", scanned(0, 0))).toBe("The folder is empty.");
  expect(countsLine("en", scanned(0, 0, false))).toBeNull();
});

test("1.5 MB keeps its decimal; from 10 MB the size is whole; Indian grouping above a thousand", () => {
  expect(countsLine("en", scanned(24, 1_500_000))).toBe("24 files, 1.5 MB");
  expect(countsLine("en", scanned(30, 52_400_000))).toBe("30 files, 52 MB");
  expect(countsLine("en", scanned(1_200, 1_234_500_000))).toBe("1,200 files, 1,235 MB");
});

test("a size under 1 KB is at least 1 KB, and one that would round to 1,000 KB says 1 MB", () => {
  expect(sizeOf("en", 300)).toEqual({ unit: "KB", n: "1" });
  expect(sizeOf("en", 0)).toEqual({ unit: "KB", n: "0" });
  expect(sizeOf("en", 999_499)).toEqual({ unit: "KB", n: "999" });
  expect(sizeOf("en", 999_500)).toEqual({ unit: "MB", n: "1" });
});

test("in Hindi: its words, its grouping, Latin digits", () => {
  expect(countsLine("hi", scanned(200, 6_000))).toBe("200 फ़ाइलें, 6 KB");
  expect(countsLine("hi", scanned(1, 1_500_000))).toBe("1 फ़ाइल, 1.5 MB");
  expect(countsLine("hi", scanned(12_345, 250_000))).toBe("12,345 फ़ाइलें, 250 KB");
  expect(countsLine("hi", scanned(0, 0))).toBe("फ़ोल्डर खाली है।");
});
