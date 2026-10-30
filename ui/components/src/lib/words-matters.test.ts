import { expect, test } from "vitest";
import { clock } from "./words.ts";
import { matterWords, mt } from "./words-matters.ts";

test("no string of living in matters says I, names a model, or uses a dash or a banned word", () => {
  for (const s of Object.entries(matterWords).flat()) {
    expect(s).not.toMatch(
      /\bI\b|\bI'm\b|[\u2013\u2014]|\b(pencil|ink|step|git|token|session|terminal|CLI|adapter|thread|safe)\b|agent process|Opus|Sonnet|Haiku|GPT-\d/i,
    );
    expect(s).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
  }
});

test("Hindi sentences end in the danda, never a full stop", () => {
  for (const [en, hi] of Object.entries(matterWords)) {
    expect(hi).not.toMatch(/[\u0900-\u097F]\.$/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});

test("slots fill in either language, with the part of the day first in Hindi", () => {
  expect(mt("en", "{n} queued, sent in order.", { n: 3 })).toBe("3 queued, sent in order.");
  expect(
    mt("hi", "Waiting for '{title}' to finish, by {time}", {
      title: "Draft plaint",
      time: clock("hi", "4:41 pm"),
    }),
  ).toBe("'Draft plaint' के पूरा होने की प्रतीक्षा, शाम 4:41 बजे तक");
  // a slot the caller didn't give stays as it is, so a missing value shows rather than vanishing
  expect(mt("en", "Client, {client}")).toBe("Client, {client}");
});
