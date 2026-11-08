// How a tool call's output is read (U17, Decided here 1), pure: one test per rule, and the caps.
import { expect, test } from "vitest";
import type { TextOrBlob } from "../../../transport/protocol.ts";
import {
  HUGE_RESULT,
  JSON_RESULT,
  LONG_RESULT,
  MCP_ERROR,
  MCP_MIXED,
  MCP_TEXT,
  NO_BLOCKS,
  NOT_JSON,
  NULL,
  OPAQUE_ERROR,
  QUOTED,
  QUOTED_OBJECT,
} from "./fixtures.ts";
import { cut, FOLDED_LINES, MAX_LINES, MAX_TEXT, readOutput } from "./parse.ts";

const read = (o: TextOrBlob) => readOutput(o.inline!);
const plain = (face: "words" | "code", text: string, error = false) => ({
  face,
  text,
  error,
  pictures: 0,
  others: 0,
});

test("codex-acp's opaque result is unwrapped: its MCP result shows its text", () => {
  expect(read(MCP_TEXT)).toEqual(plain("words", "3 judgments found."));
});

test("an opaque error shows its text as an error; one that isn't a string, as JSON", () => {
  expect(read(OPAQUE_ERROR)).toEqual(plain("words", "connector: out of credit", true));
  expect(readOutput('{"result":null,"error":{"code":-32000}}')).toEqual(
    plain("code", '{\n  "code": -32000\n}', true),
  );
  // the Claude-shaped corpus sample: an object of error alone
  expect(readOutput('{"error":"denied"}')).toEqual(plain("words", "denied", true));
});

test("MCP text blocks show in order, a blank line apart; pictures and other blocks are counted, not drawn", () => {
  expect(read(MCP_MIXED)).toEqual({
    face: "words",
    text: "The page has two figures.",
    error: false,
    pictures: 2,
    others: 1,
  });
  expect(
    readOutput(
      JSON.stringify({
        content: [
          { type: "text", text: "a" },
          { type: "text", text: "b" },
        ],
      }),
    ),
  ).toEqual(plain("words", "a\n\nb"));
});

test("MCP's isError shows its text as an error", () => {
  expect(read(MCP_ERROR)).toEqual(
    plain("words", "Indian Kanoon refused the search: the key was not accepted.", true),
  );
});

test("a quoted string is parsed once more: a plain string shows as itself, a quoted object is read on", () => {
  expect(read(QUOTED)).toEqual(plain("words", "Done"));
  expect(read(QUOTED_OBJECT)).toEqual(plain("words", "ok"));
  // a quoted string holding a number stays the string
  expect(readOutput('"42"')).toEqual(plain("words", "42"));
});

test("any other value pretty-prints with two-space indents in the code face", () => {
  const r = read(JSON_RESULT);
  expect(r?.face).toBe("code");
  expect(r?.text.split("\n").slice(0, 3)).toEqual([
    "{",
    '  "court": "Supreme Court of India",',
    '  "year": 2018,',
  ]);
  expect(readOutput('{"ok":true}')).toEqual(plain("code", '{\n  "ok": true\n}'));
  expect(readOutput("7")).toEqual(plain("code", "7"));
});

test("text that isn't JSON shows as it is, in the code face", () => {
  expect(read(NOT_JSON)).toEqual(plain("code", "exit 0: 14 files scanned"));
});

test("nothing came back: null, an empty string, an empty array or object, an MCP result with no blocks", () => {
  for (const raw of [
    NULL.inline!,
    '""',
    "[]",
    "{}",
    NO_BLOCKS.inline!,
    '{"result":null,"error":null}',
    "",
  ])
    expect(readOutput(raw), raw).toBeNull();
});

test("the cut: 12 lines folded, every line up to 2,000 or 64 KB, whichever comes first", () => {
  const long = cut(read(LONG_RESULT)!.text);
  expect(long.long).toBe(true);
  expect(long.capped).toBe(false);
  expect(long.folded.split("\n").length).toBe(FOLDED_LINES);
  expect(long.all.split("\n").length).toBe(22);
  const huge = cut(read(HUGE_RESULT)!.text);
  expect(huge.capped).toBe(true);
  expect(huge.all.split("\n").length).toBe(MAX_LINES);
  const wide = cut("y".repeat(MAX_TEXT + 10));
  expect(wide).toMatchObject({ long: false, capped: true });
  expect(wide.all.length).toBe(MAX_TEXT);
  expect(cut("one\ntwo")).toEqual({
    all: "one\ntwo",
    folded: "one\ntwo",
    long: false,
    capped: false,
  });
});

test("the parse reads at most 64 KB: past it the text is cut first, so it reads as text", () => {
  const big = JSON.stringify({ result: "z".repeat(MAX_TEXT), error: null });
  const r = readOutput(big);
  expect(r?.face).toBe("code");
  expect(r?.text.length).toBe(MAX_TEXT);
});
