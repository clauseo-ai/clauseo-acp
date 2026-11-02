// The token rules apart from the page: an edit before or after a token moves it, one into it drops it, undo's text is
// read again for the agent's commands, a command no longer listed is plain text, and the caret never rests inside.
import { expect, test } from "vitest";
import {
  scanTokens,
  shiftTokens,
  snapped,
  stillListed,
  tokenAround,
  tokenEndingAt,
  tokenStartingAt,
  withPick,
  wordDelete,
  type Token,
} from "./composer-tokens-logic.ts";

const tok = (value: string, text: string, from = 0): Token => {
  const start = value.indexOf(text, from);
  return { start, end: start + text.length, text };
};

test("an edit before a token moves it, one after it leaves it, an insertion at either edge keeps it", () => {
  const v = "ask /review now";
  const t = tok(v, "/review");
  expect(shiftTokens(v, `please ${v}`, [t])).toEqual([{ ...t, start: 11, end: 18 }]);
  expect(shiftTokens(v, "ask /review now, quickly", [t])).toEqual([t]);
  expect(shiftTokens(v, "sk /review now", [t])).toEqual([{ ...t, start: 3, end: 10 }]);
  // typed right against its start, or right after its end: the token's characters are still there
  expect(shiftTokens(v, "ask x/review now", [t])).toEqual([{ ...t, start: 5, end: 12 }]);
  expect(shiftTokens(v, "ask /reviewx now", [t])).toEqual([t]);
  // Devanagari before it, composed in several code units
  expect(shiftTokens(v, `प्रश्न ${v}`, [t])[0]?.start).toBe(11);
});

test("an edit inside a token, or across it, drops it; the others stand, even where the diff takes in their slash", () => {
  const v = "/review the lease /compact";
  const a = tok(v, "/review");
  const b = tok(v, "/compact");
  expect(shiftTokens(v, "/reiew the lease /compact", [a, b])).toEqual([
    { ...b, start: 17, end: 25 },
  ]);
  expect(shiftTokens(v, "/review the lease /co", [a, b])).toEqual([a]);
  expect(shiftTokens(v, "/compact", [a, b])).toEqual([{ ...b, start: 0, end: 8 }]);
  expect(shiftTokens(v, v, [a, b])).toEqual([a, b]);
  // the first of two alike taken out: one token left, not two on the same characters
  const w = "/review /review";
  expect(shiftTokens(w, "/review", [tok(w, "/review"), tok(w, "/review", 1)])).toEqual([
    { start: 0, end: 7, text: "/review" },
  ]);
});

test("undo's text is read again: the agent's commands that stand alone are tokens, the kept ones stay", () => {
  const names = ["review", "compact", "rev"];
  const v = "/review the lease, then /compact. a/review x /reviewed /rev";
  const found = scanTokens(v, names, []);
  expect(found.map((t) => [t.text, t.start])).toEqual([
    ["/review", 0],
    ["/rev", v.lastIndexOf("/rev")],
  ]);
  // `/compact.` is followed by a full stop, not a space: no token, as a pick always leaves a space
  expect(scanTokens("/compact\n/review", names, []).map((t) => t.text)).toEqual([
    "/compact",
    "/review",
  ]);
  // a kept token is never doubled or overlapped
  const kept = tok(v, "/review");
  expect(scanTokens(v, names, [kept]).filter((t) => t.start === 0)).toHaveLength(1);
  // with the command gone from the list, the text is plain
  expect(scanTokens("/review", ["compact"], [])).toEqual([]);
});

test("a command the agent no longer lists is plain text; a pick is a token once the field holds it", () => {
  const v = "/review and /compact";
  const both = [tok(v, "/review"), tok(v, "/compact")];
  expect(stillListed(both, ["compact"])).toEqual([both[1]]);
  const pick = { start: 0, end: 7, text: "/review" };
  expect(withPick(v, [both[1]!], pick)).toEqual(both);
  expect(withPick("/rev and /compact", [], pick)).toEqual([]);
});

test("the caret goes to a token's nearer edge, its end at a tie; a selection grows to whole tokens", () => {
  const v = "x /review y";
  const t = [tok(v, "/review")];
  expect(tokenAround(t, 2)).toBeUndefined();
  expect(tokenAround(t, 3)).toBe(t[0]);
  expect(tokenEndingAt(t, 9)).toBe(t[0]);
  expect(tokenStartingAt(t, 2)).toBe(t[0]);
  expect(snapped(t, 3, 3)).toEqual({ start: 2, end: 2 });
  expect(snapped(t, 8, 8)).toEqual({ start: 9, end: 9 });
  // "/review" is 7 long: from 2, the middle at 5.5; 5 is nearer the start, 6 the end
  expect(snapped(t, 5, 5)).toEqual({ start: 2, end: 2 });
  expect(snapped(t, 6, 6)).toEqual({ start: 9, end: 9 });
  expect(snapped(t, 2, 2)).toBeNull();
  expect(snapped(t, 9, 9)).toBeNull();
  expect(snapped(t, 0, 4)).toEqual({ start: 0, end: 9 });
  expect(snapped(t, 4, 11)).toEqual({ start: 2, end: 11 });
  expect(snapped(t, 0, 11)).toBeNull();
});

test("a selection's end that moves into a token goes on the way it travelled, so Shift+Arrow can let a token go", () => {
  const v = "ask /review now";
  const t = [tok(v, "/review")];
  // Shift+Left from the end, a step into the token: the token is taken in whole
  expect(snapped(t, 10, 15, { start: 11, end: 15 })).toEqual({ start: 4, end: 15 });
  // Shift+Right from there: the token is let go, not pushed back out to its start
  expect(snapped(t, 5, 15, { start: 4, end: 15 })).toEqual({ start: 11, end: 15 });
  // the other way round, the end moving
  expect(snapped(t, 0, 6, { start: 0, end: 4 })).toEqual({ start: 0, end: 11 });
  expect(snapped(t, 0, 10, { start: 0, end: 11 })).toEqual({ start: 0, end: 4 });
  // a new selection, or one from a caret: outward
  expect(snapped(t, 6, 15, { start: 15, end: 15 })).toEqual({ start: 4, end: 15 });
  expect(snapped(t, 1, 6, null)).toEqual({ start: 1, end: 11 });
});

test("a word delete that would cut into a token takes the token whole and the spaces up to the caret", () => {
  const v = "ask /review  now";
  const t = [tok(v, "/review")];
  expect(wordDelete(v, t, 11, false)).toEqual({ start: 4, end: 11 });
  expect(wordDelete(v, t, 13, false)).toEqual({ start: 4, end: 13 });
  expect(wordDelete(v, t, 4, true)).toEqual({ start: 4, end: 11 });
  expect(wordDelete(v, t, 3, true)).toEqual({ start: 3, end: 11 });
  // a word that isn't the token's: the engine's own delete
  expect(wordDelete(v, t, 16, false)).toBeNull();
  expect(wordDelete(v, t, 2, false)).toBeNull();
  expect(wordDelete(v, t, 11, true)).toBeNull();
});
