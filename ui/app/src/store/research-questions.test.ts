// What Clauseo Case Law's credits buy, in the server's words (r7470): read as written when present, nothing when
// absent or empty; the library's price and credit lines and Settings' pack line with and without them.
import { expect, test } from "vitest";
import { caseLawFacts } from "../library/addons-source.ts";
import { creditLines, priceLines } from "../library/case-law-model.ts";
import { packLabel } from "../settings/sections/connectors-model.ts";
import type { Balance, ConnectorStatus, ConnectorsState, PackView } from "../transport/protocol.ts";
import { balanceQuestions, packQuestions } from "./research-questions.ts";

/** A pack or a balance with the server's words, as #1330's host passes them on. */
const saying = <T extends PackView | Balance>(value: T, text: string): T =>
  ({ ...value, research_questions_text: text }) as T;

const PACK: PackView = { pack: "p2000", credits: 2_000, paise: 23_600 };
const FIVE = "about 5 research questions";
const UNDER_ONE = "less than one research question";

test("the readers give the text as written, and null when it is absent or empty", () => {
  expect(packQuestions(saying(PACK, FIVE))).toBe(FIVE);
  expect(packQuestions(saying(PACK, UNDER_ONE))).toBe(UNDER_ONE);
  expect(packQuestions(PACK)).toBeNull();
  expect(packQuestions(saying(PACK, ""))).toBeNull();
  expect(packQuestions(saying(PACK, "  "))).toBeNull();
  expect(packQuestions(null)).toBeNull();
  expect(balanceQuestions(saying({ credits: 1_240 }, "about 3 research questions"))).toBe(
    "about 3 research questions",
  );
  expect(balanceQuestions({ credits: 1_240 })).toBeNull();
  expect(balanceQuestions(undefined)).toBeNull();
});

test("Settings' pack line: with the words after the price, else as before", () => {
  expect(packLabel("en", saying(PACK, FIVE))).toBe(
    "2,000 credits, ₹236, for about 5 research questions",
  );
  expect(packLabel("en", PACK)).toBe("2,000 credits, ₹236");
  expect(packLabel("hi", saying(PACK, FIVE))).toBe(
    "2,000 क्रेडिट, ₹236, about 5 research questions के लिए",
  );
});

test("the library's lines: the price with the pack's words, the balance's under credits left", () => {
  const pack = { paise: 23_600, credits: 2_000 };
  expect(priceLines("en", { ...pack, questions: FIVE })[0]).toBe(
    "From ₹236, for about 5 research questions.",
  );
  expect(priceLines("en", { ...pack, questions: null })[0]).toBe("From ₹236.");
  expect(creditLines("en", 1_240, false, "about 3 research questions")).toEqual([
    "1,240 credits left",
    "Enough for about 3 research questions.",
  ]);
  expect(creditLines("en", 1_240, false, null)).toEqual(["1,240 credits left"]);
  // out of credit and at 0: the rule's words alone
  expect(creditLines("en", 0, false, UNDER_ONE)).toEqual(["No credits. Add credits to search."]);
});

const stateOf = (status: ConnectorStatus, packs: PackView[]): ConnectorsState => ({
  available: true,
  locked: false,
  connectors: [{ id: "case-law", name: "Clauseo Case Law", price_paise: 23_600, packs, status }],
});

test("the library's facts carry the smallest pack's words and the balance's, never while out of credit", () => {
  const packs = [
    saying({ ...PACK, pack: "big", credits: 10_000, paise: 1_18_000 }, "about 25"),
    saying(PACK, FIVE),
  ];
  const paid: ConnectorStatus = {
    signed_in: { low: false, balance: saying({ credits: 1_240 }, "about 3 research questions") },
  };
  const facts = caseLawFacts(stateOf(paid, packs), true);
  expect(facts.pack).toEqual({ paise: 23_600, credits: 2_000, questions: FIVE });
  expect(facts.questions).toBe("about 3 research questions");
  const out = caseLawFacts(stateOf({ out_of_credit: {} }, packs), true);
  expect(out.questions).toBeNull();
  // absent: no words anywhere
  const plain = caseLawFacts(
    stateOf({ signed_in: { low: false, balance: { credits: 1_240 } } }, [PACK]),
    true,
  );
  expect(plain.pack?.questions).toBeNull();
  expect(plain.questions).toBeNull();
});
