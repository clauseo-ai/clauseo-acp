// The form read from an elicitation's schema (elicitation-schema.ts): Claude's AskUserQuestion as claude-agent-acp
// sends it, the other kinds of field, the values each gives, when Send can go, and what gives no form at all.
import { expect, test } from "vitest";
import type { RawPayload } from "../transport/protocol.ts";
import { ONE_QUESTION, TWO_QUESTIONS } from "./elicitation-fixtures.ts";
import {
  anyFilled,
  canSend,
  content,
  initialDraft,
  parseForm,
  problems,
  type Field,
  type Form,
} from "./elicitation-schema.ts";

const form = (schema: RawPayload): Form => {
  const f = parseForm(schema);
  if (f === null) throw new Error("no form");
  return f;
};
const one = (props: Record<string, RawPayload>, required: string[] = []): Field =>
  form({ type: "object", properties: props, required }).fields[0]!;

test("one AskUserQuestion: a single choice with its options' lines, the Other beneath it, nothing required", () => {
  const f = form(ONE_QUESTION.schema);
  expect(f.fields).toHaveLength(1);
  const q = f.fields[0]!;
  expect(q).toMatchObject({ kind: "choice", key: "question_0", title: "Court", required: false });
  if (q.kind !== "choice") throw new Error();
  expect(q.description).toBeNull();
  expect(q.choices.map((c) => [c.title, c.description])).toEqual([
    ["Delhi High Court", "The bank's registered office is in New Delhi."],
    ["Sessions Court, Saket", "Where the complaint under section 138 is pending."],
    ["Supreme Court", null],
  ]);
  expect(q.other).toMatchObject({
    key: "question_0_custom",
    kind: "text",
    multiline: true,
    description: "Type your own answer, or add a note to the option you chose above (optional).",
  });
});

test("two AskUserQuestions: a choice and a multi-select, each with its question's words and its Other", () => {
  const f = form(TWO_QUESTIONS.schema);
  expect(f.fields.map((x) => [x.key, x.kind, x.description])).toEqual([
    ["question_0", "choice", "Which relief should the petition ask for first?"],
    ["question_1", "multi", "Which papers should be annexed?"],
  ]);
  const [a, b] = f.fields;
  expect(a?.kind === "choice" && a.other?.key).toBe("question_0_custom");
  expect(b?.kind === "multi" && b.other?.key).toBe("question_1_custom");
});

test("the values: a choice's const, a multi's consts in schema order, an Other as typed, empties left out", () => {
  const f = form(TWO_QUESTIONS.schema);
  const empty = initialDraft(f);
  expect(content(f, empty)).toEqual({});
  expect(anyFilled(f, empty)).toBe(false);
  expect(canSend(f, empty)).toBe(false);
  const d = { ...empty, question_0: 1, question_1: [0, 2], question_1_custom: "  Affidavit " };
  expect(canSend(f, d)).toBe(true);
  expect(content(f, d)).toEqual({
    question_0: "Stay the proceedings",
    question_1: ["Notice to the bank", "Bank statement, March 2001"],
    question_1_custom: "  Affidavit ",
  });
  // an Other alone is an answer; one of spaces is nothing
  expect(content(f, { ...empty, question_0_custom: "Both, in that order" })).toEqual({
    question_0_custom: "Both, in that order",
  });
  expect(canSend(f, { ...empty, question_0_custom: "   " })).toBe(false);
});

test("enum with enumNames is a choice whose value keeps its type; a default picks its option", () => {
  const f = one({
    size: { type: "integer", enum: [1, 2, 3], enumNames: ["One", "Two"], default: 2 },
  });
  expect(f).toMatchObject({ kind: "choice", initial: 1 });
  if (f.kind !== "choice") throw new Error();
  expect(f.choices.map((c) => c.title)).toEqual(["One", "Two", "3"]);
  const whole = form({
    type: "object",
    properties: { size: { type: "integer", enum: [1, 2, 3] } },
  });
  expect(content(whole, { size: 2 })).toEqual({ size: 3 });
});

test("a multi honours minItems and maxItems once anything is ticked, and items.enum", () => {
  const f = form({
    type: "object",
    properties: {
      p: { type: "array", items: { enum: ["a", "b", "c"] }, minItems: 2, maxItems: 2 },
    },
  });
  expect(problems(f, { p: [] }).size).toBe(0);
  expect(problems(f, { p: [0] }).get("p")).toBe("invalid");
  expect(problems(f, { p: [0, 1, 2] }).get("p")).toBe("invalid");
  expect(content(f, { p: [2, 0] })).toEqual({ p: ["a", "c"] });
});

test("strings: format gives the input, lengths and formats are checked, a long one is a textarea", () => {
  expect(one({ e: { type: "string", format: "email" } })).toMatchObject({
    input: "email",
    multiline: false,
  });
  expect(one({ u: { type: "string", format: "uri" } })).toMatchObject({ input: "url" });
  expect(one({ d: { type: "string", format: "date" } })).toMatchObject({ input: "date" });
  expect(one({ n: { type: "string", maxLength: 500 } })).toMatchObject({
    input: "text",
    multiline: true,
  });
  expect(one({ n: { type: "string", maxLength: 200 } })).toMatchObject({ multiline: false });
  const f = form({
    type: "object",
    properties: {
      name: { type: "string", minLength: 3, maxLength: 5 },
      mail: { type: "string", format: "email" },
      site: { type: "string", format: "uri" },
      day: { type: "string", format: "date" },
    },
  });
  const p = (d: Record<string, string>) => [...problems(f, d).entries()];
  expect(p({ name: "ab" })).toEqual([["name", "invalid"]]);
  expect(p({ name: "abcdef" })).toEqual([["name", "invalid"]]);
  expect(p({ name: "abc" })).toEqual([]);
  expect(p({ mail: "rohan@" })).toEqual([["mail", "invalid"]]);
  expect(p({ mail: "rohan@clauseo.chat" })).toEqual([]);
  expect(p({ site: "not a url" })).toEqual([["site", "invalid"]]);
  expect(p({ site: "https://clauseo.chat" })).toEqual([]);
  expect(p({ day: "3 Nov" })).toEqual([["day", "invalid"]]);
  expect(p({ day: "2026-11-03" })).toEqual([]);
});

test("numbers: a Number in the values, left out when empty; integer and range checked", () => {
  const f = form({
    type: "object",
    properties: { n: { type: "integer", minimum: 1, maximum: 10 }, x: { type: "number" } },
  });
  expect(content(f, { n: "4", x: "" })).toEqual({ n: 4 });
  expect(content(f, { n: "", x: "2.5" })).toEqual({ x: 2.5 });
  expect(problems(f, { n: "4.5" }).get("n")).toBe("invalid");
  expect(problems(f, { n: "11" }).get("n")).toBe("invalid");
  expect(problems(f, { n: "0" }).get("n")).toBe("invalid");
});

test("a boolean is always sent, its default or false", () => {
  const f = form({
    type: "object",
    properties: {
      a: { type: "boolean" },
      b: { type: "boolean", default: true },
      t: { type: "string" },
    },
  });
  const d = initialDraft(f);
  expect(d).toEqual({ a: false, b: true, t: "" });
  expect(content(f, d)).toEqual({ a: false, b: true });
});

test("required: an empty required field stops Send, and its problem is required", () => {
  const f = form({
    type: "object",
    properties: { name: { type: "string", title: "Name" }, note: { type: "string" } },
    required: ["name"],
  });
  expect(f.fields[0]?.required).toBe(true);
  expect(problems(f, { name: "", note: "x" }).get("name")).toBe("required");
  expect(canSend(f, { name: "", note: "x" })).toBe(false);
  expect(canSend(f, { name: "Rohan", note: "" })).toBe(true);
});

test("unsupported: an optional property of no known kind is left out, a required one gives no form", () => {
  const props = {
    where: { type: "object", properties: {} },
    name: { type: "string" },
  };
  expect(form({ type: "object", properties: props }).fields.map((f) => f.key)).toEqual(["name"]);
  expect(parseForm({ type: "object", properties: props, required: ["where"] })).toBeNull();
  expect(parseForm({ type: "object", properties: { where: { type: "object" } } })).toBeNull();
  expect(parseForm({ type: "object" })).toBeNull();
  expect(parseForm({ type: "array", items: {} })).toBeNull();
  expect(parseForm("form")).toBeNull();
  expect(parseForm(null)).toBeNull();
});

test("a _custom with no question beside it stands as a field of its own", () => {
  const f = form({
    type: "object",
    properties: { note_custom: { type: "string", title: "Note" } },
  });
  expect(f.fields.map((x) => [x.key, x.kind])).toEqual([["note_custom", "text"]]);
});
