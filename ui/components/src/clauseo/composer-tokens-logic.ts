// The composer's tokens, apart from the page: a picked command is plain text in the field (`/review`), tracked here as
// a range, so it deletes as one unit, the caret never rests inside it, and the overlay can draw a box behind it.
// G1a's spike (measurements/g1a-indic-input/spike/src/mentions/logic.ts in the private repo) is the reference:
// - an edit moves the ranges after it, found by diffing the old value and the new (common prefix and suffix); a token
//   the edit cuts into is no token any more, and an insertion at a token's edge keeps it;
// - undo and redo bring text back without a pick, so the value is read again then for the agent's commands;
// - a command the agent no longer lists is plain text, not a token.

const WS = /\s/u;

/** A token: its text in the field (`/review`), from `start` to `end` (exclusive). */
export type Token = { start: number; end: number; text: string };

/** Moves the tokens across the edit that turned `prev` into `next`. */
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

/** Whether a token may stand at `start` to `end`: at the field's start or after whitespace, and followed by
 * whitespace or the field's end, as the menu puts one in. */
const standsAlone = (value: string, start: number, end: number) =>
  (start === 0 || WS.test(value[start - 1] ?? "")) &&
  (end >= value.length || WS.test(value[end] ?? ""));

/** The tokens `keep`, and every other command of `names` the value holds standing alone: after undo or redo, or as
 * the menu's chunk arrives over words already there. */
export function scanTokens(
  value: string,
  names: readonly string[],
  keep: readonly Token[],
): Token[] {
  const found: Token[] = [...keep];
  const free = (s: number, e: number) => !found.some((t) => s < t.end && e > t.start);
  for (const name of names) {
    const text = `/${name}`;
    for (let i = value.indexOf(text); i !== -1; i = value.indexOf(text, i + 1)) {
      const e = i + text.length;
      if (standsAlone(value, i, e) && free(i, e)) found.push({ start: i, end: e, text });
    }
  }
  return found.sort((a, b) => a.start - b.start);
}

/** The tokens whose command the agent still lists. */
export function stillListed(tokens: readonly Token[], names: readonly string[]): Token[] {
  const listed = new Set(names.map((n) => `/${n}`));
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

/** Where a selection goes so it rests in no token: a caret to the token's nearer edge (its end at a tie), a range out
 * to whole tokens at both ends; null when it rests in none already. */
export function snapped(
  tokens: readonly Token[],
  start: number,
  end: number,
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
  return { start: a?.start ?? start, end: b?.end ?? end };
}
