// prompt-cost.ts: a question's text counted as the host counts it against MAX_PROMPT_TEXT_BYTES, held to the cases the
// Rust test reads too (crates/protocol/tests/support/escaped-len-cases.json), and the cap the bindings carry.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { escapedLen, MAX_PROMPT_TEXT_BYTES, tooLong } from "./prompt-cost.ts";

type Cases = { note: string; cases: { name: string; text: string; escaped_bytes: number }[] };

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

const shared = JSON.parse(
  read("../../../../../crates/protocol/tests/support/escaped-len-cases.json"),
) as Cases;

test("every shared case counts as the host counts it", () => {
  expect(shared.cases.length).toBeGreaterThan(0);
  for (const c of shared.cases)
    expect([c.name, escapedLen(c.text)]).toEqual([c.name, c.escaped_bytes]);
  // and the note names this test, which it holds to the same answers
  expect(shared.note).toContain("ui/app/src/room/thread/prompt-cost.test.ts");
});

test("a count that disagrees with a case is caught", () => {
  // the length in UTF-16 code units, or the bytes before escaping, would pass none of these
  const mixed = shared.cases.find((c) => c.name === "mixed");
  expect(mixed).toBeDefined();
  expect(mixed!.text.length).not.toBe(mixed!.escaped_bytes);
  expect(new TextEncoder().encode(mixed!.text).length).not.toBe(mixed!.escaped_bytes);
});

test("a lone surrogate counts 3, as the U+FFFD it is sent as", () => {
  expect(escapedLen("\ud83d")).toBe(3);
  expect(escapedLen("a\ude00b")).toBe(5);
  expect(escapedLen("\ud83d\ude00")).toBe(4);
});

test("MAX_PROMPT_TEXT_BYTES is the bindings' own", () => {
  const wire = /export const MAX_PROMPT_TEXT_BYTES = (\d+) as const;/.exec(
    read("../../transport/bindings.ts"),
  );
  expect(Number(wire?.[1])).toBe(MAX_PROMPT_TEXT_BYTES);
});

test("at the cap a question goes; one byte over, or an escape that crosses it, it doesn't", () => {
  expect(tooLong("a".repeat(MAX_PROMPT_TEXT_BYTES))).toBe(false);
  expect(tooLong("a".repeat(MAX_PROMPT_TEXT_BYTES + 1))).toBe(true);
  // a newline costs 2: one short of the cap in characters, over it in bytes
  expect(tooLong("a".repeat(MAX_PROMPT_TEXT_BYTES - 2) + "\n")).toBe(false);
  expect(tooLong("a".repeat(MAX_PROMPT_TEXT_BYTES - 1) + "\n")).toBe(true);
  // Hindi's 3 bytes a character: a third of the cap goes, one more doesn't
  expect(tooLong("क".repeat(MAX_PROMPT_TEXT_BYTES / 3))).toBe(false);
  expect(tooLong("क".repeat(MAX_PROMPT_TEXT_BYTES / 3 + 1))).toBe(true);
});
