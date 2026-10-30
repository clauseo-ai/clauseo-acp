import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { frame } from "./words-frame.ts";

test("a matter paused at the plan's limit asks to continue at a clock time, the part of the day first in Hindi", () => {
  const at = (lang: "en" | "hi") => ({ time: clock(lang, "9:40 pm") });
  expect(frame("en", "Limit reached. Continue at {time}?", at("en"))).toBe(
    "Limit reached. Continue at 9:40 pm?",
  );
  expect(frame("hi", "Limit reached. Continue at {time}?", at("hi"))).toBe(
    "सीमा पूरी। रात 9:40 बजे जारी\u00a0रखें?",
  );
  expect(frame("en", "Paused until {time}", at("en"))).toBe("Paused until 9:40 pm");
  expect(frame("hi", "Paused until {time}", at("hi"))).toBe("रात 9:40 बजे तक रुका है");
});

test("a working matter gives the clock time it expects to finish", () => {
  expect(frame("en", "Answering. Expected by {time}", { time: clock("en", "4:32 pm") })).toBe(
    "Answering. Expected by 4:32 pm",
  );
  expect(frame("hi", "Answering. Expected by {time}", { time: clock("hi", "4:32 pm") })).toBe(
    "उत्तर तैयार हो रहा है। शाम 4:32 बजे तक अपेक्षित",
  );
});
