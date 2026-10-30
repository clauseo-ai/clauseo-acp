import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { frame } from "./words-frame.ts";

test("the plan meter's three states, in both languages, with the part of the day first in Hindi", () => {
  const at = (lang: "en" | "hi") => ({ time: clock(lang, "9:40 pm") });
  expect(frame("en", "Plenty left until the {time} reset", at("en"))).toBe(
    "Plenty left until the 9:40 pm reset",
  );
  expect(frame("hi", "Plenty left until the {time} reset", at("hi"))).toBe(
    "रात 9:40 बजे के रीसेट तक काफ़ी बाकी है",
  );
  expect(frame("en", "Close to the limit. Resets at {time}.", at("en"))).toBe(
    "Close to the limit. Resets at 9:40 pm.",
  );
  expect(frame("hi", "Close to the limit. Resets at {time}.", at("hi"))).toBe(
    "सीमा के करीब। रात 9:40 बजे रीसेट होगा।",
  );
  expect(frame("en", "Limit reached until {time}", at("en"))).toBe("Limit reached until 9:40 pm");
  expect(frame("hi", "Limit reached until {time}", at("hi"))).toBe("रात 9:40 बजे तक सीमा पूरी");
});

test("a working matter gives the clock time it expects to finish", () => {
  expect(frame("en", "Answering. Expected by {time}", { time: clock("en", "4:32 pm") })).toBe(
    "Answering. Expected by 4:32 pm",
  );
  expect(frame("hi", "Answering. Expected by {time}", { time: clock("hi", "4:32 pm") })).toBe(
    "उत्तर तैयार हो रहा है। शाम 4:32 बजे तक अपेक्षित",
  );
});
