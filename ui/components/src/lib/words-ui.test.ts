import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { osOf, revealKey, ui } from "./words-ui.ts";

test("the computer's own file manager: Finder on the Mac, File Explorer on Windows", () => {
  expect(revealKey(osOf("MacIntel"))).toBe("Show in Finder");
  expect(revealKey(osOf("macOS"))).toBe("Show in Finder");
  expect(revealKey(osOf("Win32"))).toBe("Show in File Explorer");
  expect(revealKey(osOf("Windows"))).toBe("Show in File Explorer");
  expect(osOf("Linux x86_64")).toBe("linux");
  expect(revealKey("linux")).toBe("Show in folder");
  expect(ui("hi", revealKey("linux"))).toBe("फ़ोल्डर में दिखाएँ");
  expect(ui("hi", revealKey("windows"))).toBe("File Explorer में दिखाएँ");
});

test("slots fill in both languages, and Hindi takes the part of the day first", () => {
  expect(
    ui("en", "Restore the {time} version of the petition?", { time: clock("en", "3:58 pm") }),
  ).toBe("Restore the 3:58 pm version of the petition?");
  expect(
    ui("hi", "Restore the {time} version of the petition?", { time: clock("hi", "3:58 pm") }),
  ).toBe("याचिका का दोपहर 3:58 बजे वाला संस्करण पुनर्स्थापित करें?");
  expect(ui("hi", "Reading papers, {n} of {total}", { n: 120, total: 400 })).toBe(
    "दस्तावेज़ पढ़े जा रहे हैं, 400 में से 120",
  );
});

test.each([
  ["./words-ui.ts", 40],
  ["./words-frame.ts", 20],
])(
  "Hindi sentences in %s end with the danda, never a full stop, and avoid the banned words",
  async (file, least) => {
    const { readFileSync } = await import("node:fs");
    const src = readFileSync(new URL(file, import.meta.url), "utf8");
    const hindi = [...src.matchAll(/"([^"]*[\u0900-\u097F][^"]*)"/g)].map((m) => m[1] ?? "");
    expect(hindi.length).toBeGreaterThan(least);
    for (const s of hindi) {
      expect(s, s).not.toMatch(/[\u0900-\u097F]\.$/);
      expect(s, s).not.toMatch(/पेंसिल|स्याही|चरण|कदम|मैं|[\u0966-\u096F]/);
    }
    for (const [en, hi] of [...src.matchAll(/^\s+"?([^":]+?)"?: "([^"]+)",$/gm)].map(
      (m) => [m[1] ?? "", m[2] ?? ""] as const,
    )) {
      if (en.endsWith(".")) expect(hi, en).toMatch(/।$/);
    }
  },
);
