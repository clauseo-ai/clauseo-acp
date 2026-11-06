// The settings window's words: each string the design gives, in English and Hindi exactly as the design has them
// (design/03-system/src/lib/words-settings.ts, words-frame.ts, words-ui.ts), slots filled in both languages, and
// the voice: the danda after a Hindi sentence, no banned word, and no later section's strings yet.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { frame } from "./words-frame.ts";
import { ui } from "./words-ui.ts";
import { LANGUAGE_NAMES, PREVIEW_PAPERS, st } from "./words-settings.ts";
import { settingsWords } from "./words-settings.hi.ts";

test("the sections and their lines say the design's words in both languages", () => {
  expect(
    (["Sections", "Language", "Appearance", "About", "Day", "Night"] as const).map((k) =>
      st("hi", k),
    ),
  ).toEqual(["अनुभाग", "भाषा", "दिखावट", "परिचय", "दिन", "रात"]);
  expect(st("hi", "Changes at once, and everything stays where it was.")).toBe(
    "तुरंत बदलती है, और सब कुछ वहीं रहता है जहाँ था।",
  );
  expect(st("hi", "How Clauseo looks on this computer.")).toBe("इस कंप्यूटर पर Clauseo कैसा दिखे।");
  // hi: later, so the Hindi window shows the English
  expect(st("hi", "Managed by firm")).toBe("Managed by firm");
  expect(LANGUAGE_NAMES).toEqual({ en: "English", hi: "हिन्दी" });
});

test("appearance's preview says the design's line, and its papers keep their filed titles", () => {
  expect(st("hi", "How it looks")).toBe("यह कैसा दिखता है");
  expect(
    st("hi", "A strong case, because the complaint names Mr. Sharma only as a director."),
  ).toBe("मामला मज़बूत है, क्योंकि शिकायत में श्री शर्मा को केवल निदेशक के रूप में नामित किया गया है।");
  expect(PREVIEW_PAPERS.map((p) => p.title)).toEqual([
    "Form DIR-12",
    "Board minutes",
    "Legal notice",
  ]);
});

test("privacy's design lines keep the design's Hindi; it says nothing of Indian Kanoon or anonymous counts", () => {
  expect(st("hi", "Privacy")).toBe("निजता");
  expect(st("hi", "To {name}, on your plan", { name: "Claude" })).toBe("Claude को, आपके प्लान से");
  for (const left of ["To Indian Kanoon", "To Clauseo", "Anonymous counts", "Send crash reports"])
    expect(Object.keys(settingsWords)).not.toContain(left);
});

test("the side's notes fill their slots in both languages", () => {
  expect(st("en", "{theme}, text size {n}", { theme: st("en", "Night"), n: 18 })).toBe(
    "Night, text size 18",
  );
  expect(st("hi", "{theme}, text size {n}", { theme: st("hi", "Night"), n: 18 })).toBe(
    "रात, अक्षरों का आकार 18",
  );
  expect(st("en", "Version {version}", { version: "3.0.4" })).toBe("Version 3.0.4");
  expect(st("hi", "Version {version}", { version: "3.0.4" })).toBe("संस्करण 3.0.4");
});

test("appearance's controls take the frame's and the controls' words", () => {
  expect(frame("hi", "Theme")).toBe("थीम");
  expect(frame("hi", "Follow the computer")).toBe("कंप्यूटर के अनुसार");
  expect(frame("hi", "Text size")).toBe("अक्षरों का आकार");
  expect(frame("hi", "Every word on screen grows or shrinks with it.")).toBe(
    "स्क्रीन के सभी शब्द इसके साथ बड़े या छोटे होते हैं।",
  );
  expect(frame("hi", "Reduced motion")).toBe("कम हलचल");
  expect(frame("hi", "Every change lands at once, and nothing moves.")).toBe(
    "हर बदलाव तुरंत दिखता है और कुछ भी हिलता नहीं।",
  );
  expect(
    (
      [
        "Lite mode",
        "Takes away every shadow and all motion.",
        "On",
        "Always, on this computer",
        "Off",
        "Never, even over a remote desktop",
      ] as const
    ).map((k) => ui("hi", k)),
  ).toEqual([
    "सादा मोड",
    "सभी छायाएँ और सारी हलचल हटा देता है।",
    "चालू",
    "इस कंप्यूटर पर हमेशा",
    "बंद",
    "कभी नहीं, रिमोट डेस्कटॉप पर भी नहीं",
  ]);
});

test("each Hindi sentence ends with the danda, and no banned word or Devanagari digit appears", () => {
  for (const [en, hi] of Object.entries(settingsWords)) {
    if (hi === null) continue; // hi: later, no Hindi yet
    expect(hi, en).not.toMatch(/पेंसिल|स्याही|चरण|कदम|मैं|[\u0966-\u096F]/);
    if (hi === null) continue; // hi: later, no Hindi yet
    if (en.endsWith(".")) expect(hi, en).toMatch(/।$/);
    else expect(hi, en).not.toMatch(/[।.]$/);
    // a slot in English is a slot in Hindi
    expect([...hi.matchAll(/\{\w+\}/g)].map((m) => m[0]).sort(), en).toEqual(
      [...en.matchAll(/\{\w+\}/g)].map((m) => m[0]).sort(),
    );
  }
});

test("only the sections built so far bring their strings: no later section's name or line is here", () => {
  const src = ["./words-settings.ts", "./words-settings.hi.ts"]
    .map((file) => readFileSync(new URL(file, import.meta.url), "utf8"))
    .join("\n");
  for (const later of ["Indian Kanoon", "Updates", "Remembered answers"])
    expect(Object.keys(settingsWords), later).not.toContain(later);
  // UC built the Safe workspace (R17's part; its remembered answers are UC2's)
  expect(Object.keys(settingsWords)).toContain("Safe workspace");
  // the design's facts (its version and date) stay in the design
  expect(src).not.toMatch(/3\.0\.4|October/);
});
