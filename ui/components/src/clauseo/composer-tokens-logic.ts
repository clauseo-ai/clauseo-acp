// The composer's tokens, apart from the page: a picked command is plain text in the field (`/review`), tracked here as
// a range, so it deletes as one unit, the caret never rests inside it, and the overlay can draw a box behind it.
// G1a's spike (measurements/g1a-indic-input/spike/src/mentions/logic.ts in the private repo) is the reference:
// - an edit moves the ranges after it, found by diffing the old value and the new (common prefix and suffix); a token
//   the edit cuts into is no token any more, and an insertion at a token's edge keeps it, so an input method's
//   composition beside it never loses it;
// - but a token counts (its box, delete as one unit, the caret kept out, a paper sent) only while it stands alone,
//   whitespace or the field's edge on both sides, as a pick leaves it: typed against (`see @b.pdfx`) it reads as
//   plain text until the space is back (`standing`);
// - undo and redo bring text back without a pick, so the value is read again then for the tokens' texts: the agent's
//   commands (`/review`) and the papers picked in this field (`@Notice.pdf`);
// - a command the agent no longer lists is plain text, not a token.

const WS = /\s/u;

/** A token: its text in the field (`/review`), from `start` to `end` (exclusive). */
export type Token = { start: number; end: number; text: string };

/** Whether a token may stand at `start` to `end`: at the field's start or after whitespace, and followed by
 * whitespace or the field's end, as the menu puts one in. */
const standsAlone = (value: string, start: number, end: number) =>
  (start === 0 || WS.test(value[start - 1] ?? "")) &&
  (end >= value.length || WS.test(value[end] ?? ""));

/** Moves the tokens across the edit that turned `prev` into `next`; a token the edit cut into is dropped. */
export function shiftTokens(prev: string, next: string, tokens: readonly Token[]): Token[] {
  if (prev === next) return [...tokens];
  const max = Math.min(prev.length, next.length);
  let p = 0;
  while (p < max && prev[p] === next[p]) p += 1;
  let s = 0;
  while (s < max - p && prev[prev.length - 1 - s] === next[next.length - 1 - s]) s += 1;
  const oldEnd = prev.length - s;
  const delta = next.length - prev.length;
  const out: Token[] = [];
  const holds = (t: Token) =>
    next.slice(t.start, t.end) === t.text && !out.some((o) => t.start < o.end && t.end > o.start);
  for (const t of tokens) {
    const moved = { ...t, start: t.start + delta, end: t.end + delta };
    if (t.end <= p) out.push(t);
    else if (t.start >= oldEnd) out.push(moved);
    // the diff can take a token's own characters into its common ends (deleting "/review the lease " before
    // "/compact" diffs as "/" kept and "review the lease /" gone): a token whose text still stands, moved or not,
    // was never edited
    else if (holds(moved)) out.push(moved);
    else if (holds(t)) out.push(t);
  }
  return out.filter((t) => next.slice(t.start, t.end) === t.text);
}

/** The tokens that count in `value`: those standing alone. One typed against is plain text while it is. */
export function standing(value: string, tokens: readonly Token[]): Token[] {
  return tokens.filter((t) => standsAlone(value, t.start, t.end));
}

/** The quoted runs a value holds: `@"` at a run's start, up to the next `"` with whitespace or the field's end after
 * it. A token's text found starting inside one is part of a longer quoted name, never a token of its own. */
function quotedRuns(value: string): { start: number; end: number }[] {
  const runs: { start: number; end: number }[] = [];
  for (let i = 0; i < value.length - 1; i += 1) {
    if (value[i] !== "@" || value[i + 1] !== '"' || (i > 0 && !WS.test(value[i - 1] ?? "")))
      continue;
    let j = i + 2;
    while (
      j < value.length &&
      !(value[j] === '"' && (j + 1 >= value.length || WS.test(value[j + 1] ?? "")))
    )
      j += 1;
    if (j >= value.length) break;
    runs.push({ start: i, end: j + 1 });
    i = j;
  }
  return runs;
}

/** The tokens `keep`, and every other of `texts` (`/review`, `@Notice.pdf`) the value holds standing alone: after
 * undo or redo, or as the menu's chunk arrives over words already there. A text that starts inside a quoted run (`@b.pdf` in
 * `@"x @b.pdf y"`) is none: an unquoted text holds no whitespace, so it can stand alone inside no other text but a
 * quoted one. */
export function scanTokens(
  value: string,
  texts: readonly string[],
  keep: readonly Token[],
): Token[] {
  const found: Token[] = [...keep];
  const free = (s: number, e: number) => !found.some((t) => s < t.end && e > t.start);
  const quoted = quotedRuns(value);
  const inQuotes = (s: number) => quoted.some((q) => s > q.start && s < q.end);
  for (const text of texts) {
    if (text === "") continue;
    for (let i = value.indexOf(text); i !== -1; i = value.indexOf(text, i + 1)) {
      const e = i + text.length;
      if (standsAlone(value, i, e) && free(i, e) && !inQuotes(i))
        found.push({ start: i, end: e, text });
    }
  }
  return found.sort((a, b) => a.start - b.start);
}

/** The tokens whose text is still one of `texts`: a command the agent still lists, a paper picked here. */
export function stillListed(tokens: readonly Token[], texts: readonly string[]): Token[] {
  const listed = new Set(texts);
  return tokens.filter((t) => listed.has(t.text));
}

/** A pick's token, added once the field holds it: any token it overlaps goes. */
export function withPick(value: string, tokens: readonly Token[], pick: Token): Token[] {
  if (value.slice(pick.start, pick.end) !== pick.text) return [...tokens];
  return [...tokens.filter((t) => t.end <= pick.start || t.start >= pick.end), pick].sort(
    (a, b) => a.start - b.start,
  );
}

export const tokenEndingAt = (tokens: readonly Token[], at: number) =>
  tokens.find((t) => t.end === at);
export const tokenStartingAt = (tokens: readonly Token[], at: number) =>
  tokens.find((t) => t.start === at);
/** The token the place stands strictly inside, if any. */
export const tokenAround = (tokens: readonly Token[], at: number) =>
  tokens.find((t) => at > t.start && at < t.end);

/** Where a selection goes so it rests in no token: a caret to the token's nearer edge (its end at a tie); a range's
 * end that stands in a token out to the token's edge. When a range was resting (`last`) and only one end moved, as
 * Shift with an arrow moves it, that end goes to the edge it was travelling to, so Shift+Left into a token takes it in
 * whole and Shift+Right lets it go again; otherwise (a new selection, a press and drag) outward. Null when the
 * selection rests in no token already. */
export function snapped(
  tokens: readonly Token[],
  start: number,
  end: number,
  last?: { start: number; end: number } | null,
): { start: number; end: number } | null {
  if (start === end) {
    const t = tokenAround(tokens, start);
    if (t === undefined) return null;
    const to = start - t.start < t.end - start ? t.start : t.end;
    return { start: to, end: to };
  }
  const a = tokenAround(tokens, start);
  const b = tokenAround(tokens, end);
  if (a === undefined && b === undefined) return null;
  const rest = last !== null && last !== undefined && last.start !== last.end ? last : null;
  // where the one moving end was, when only one moved
  const from = rest !== null && rest.end === end && rest.start !== start ? rest.start : null;
  const to = rest !== null && rest.start === start && rest.end !== end ? rest.end : null;
  return {
    start: a === undefined ? start : from !== null && start > from ? a.end : a.start,
    end: b === undefined ? end : to !== null && end < to ? b.start : b.end,
  };
}

/** What a word delete (Option or Ctrl with Backspace or Delete) from the caret `at` would take of a token: the
 * engine deletes the whitespace beside the caret and the word past it, which would leave a bare `/` or part of the
 * name. The range to delete instead, the whole token and the whitespace up to the caret; null when no token is in
 * reach. */
export function wordDelete(
  value: string,
  tokens: readonly Token[],
  at: number,
  forward: boolean,
): { start: number; end: number } | null {
  let i = at;
  if (forward) {
    while (i < value.length && WS.test(value[i] ?? "")) i += 1;
    const t = tokens.find((x) => x.start === i);
    return t === undefined ? null : { start: at, end: t.end };
  }
  while (i > 0 && WS.test(value[i - 1] ?? "")) i -= 1;
  const t = tokens.find((x) => x.end === i);
  return t === undefined ? null : { start: t.start, end: at };
}
