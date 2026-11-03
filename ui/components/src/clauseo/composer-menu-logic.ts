// The `/` menu's rules, apart from the DOM where they can be: where a trigger stands, which commands a query matches,
// and which row stays highlighted when the list changes. G1a's spike
// (measurements/g1a-indic-input/spike/src/mentions/logic.ts in the private repo) is the reference. Where the menu
// stands is composer-menu-place.ts's.
import type { SlashCommand } from "./composer.tsx";

const WS = /\s/u;

/** A `/` trigger: where its slash stands and the query typed after it, up to the caret. */
export type Slash = { at: number; query: string };

/** A trigger of either menu: its character (`/` for the agent's commands, `@` for the matter's papers), where it
 * stands, and the query typed after it, up to the caret. */
export type Trigger = Slash & { char: string };

/** The trigger the caret stands in, if any, of the characters `chars`. The caret's run is the text since the last
 * whitespace or the field's start; a trigger counts only as the run's first character, so `a/` and `rohan@x` are no
 * trigger and `x /re` and `x @le` are. */
export function triggerAt(value: string, caret: number, chars: string): Trigger | null {
  let start = caret;
  while (start > 0 && !WS.test(value[start - 1] ?? "")) start -= 1;
  const char = value[start];
  if (char === undefined || chars === "" || !chars.includes(char) || start >= caret) return null;
  return { char, at: start, query: value.slice(start + 1, caret) };
}

/** The `/` trigger the caret stands in, if any. */
export function slashAt(value: string, caret: number): Slash | null {
  const t = triggerAt(value, caret, "/");
  return t === null ? null : { at: t.at, query: t.query };
}

const fold = (s: string) => s.normalize("NFC").toLowerCase();

/** The commands whose name starts with the query, case and normalisation aside, in the agent's order. */
export function matching(commands: readonly SlashCommand[], query: string): SlashCommand[] {
  const q = fold(query);
  return commands.filter((c) => fold(c.name).startsWith(q));
}

/** The row to highlight in a new list: the one named as the highlighted row was, or the first. */
export function keepHighlight(items: readonly SlashCommand[], name: string | null): number {
  if (name === null) return 0;
  const i = items.findIndex((c) => c.name === name);
  return i === -1 ? 0 : i;
}

/** Where the run that starts at `from` ends: at the next whitespace, or the field's end. A pick replaces the whole
 * run, so `/re|view` picked as review is `/review `, not `/review view`. */
export function runEnd(value: string, from: number): number {
  let end = from;
  while (end < value.length && !WS.test(value[end] ?? "")) end += 1;
  return end;
}

/** The rows' element ids, from the list's id and each command's name, so a list replaced under the highlight says the
 * new row (aria-activedescendant changes with it). A name the agent lists twice gets a count after it. */
export function optionIds(list: string, items: readonly SlashCommand[]): string[] {
  return keyIds(
    list,
    items.map((c) => c.name),
  );
}

/** The rows' element ids from the list's id and each row's key (a command's name, a paper's id); a key listed twice
 * gets a count after it. */
export function keyIds(list: string, keys: readonly string[]): string[] {
  const seen = new Map<string, number>();
  return keys.map((k) => {
    const n = seen.get(k) ?? 0;
    seen.set(k, n + 1);
    const id = `${list}-${encodeURIComponent(k)}`;
    return n === 0 ? id : `${id}~${n}`;
  });
}

/** The text a pick puts in place of the trigger: the slash and the name, and a space unless one follows already. */
export function picked(command: SlashCommand, following: string | undefined): string {
  const text = `/${command.name}`;
  return following === undefined || !WS.test(following) ? `${text} ` : text;
}

/** What a copy of the field takes from it, so its words shape and wrap where the field's do: the caret's measure (composer-menu-place.ts),
 * and the tokens' overlay (composer-tokens.ts). Not its transform: each copy places itself its own way. */
export const FACE = [
  "boxSizing",
  "borderTopWidth",
  "borderRightWidth",
  "borderBottomWidth",
  "borderLeftWidth",
  "borderStyle",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "fontFamily",
  "fontSize",
  "fontStyle",
  "fontVariant",
  "fontWeight",
  "fontStretch",
  "fontKerning",
  "fontFeatureSettings",
  "fontVariationSettings",
  "lineHeight",
  "letterSpacing",
  "wordSpacing",
  "textTransform",
  "textIndent",
  "textAlign",
  "textRendering",
  "tabSize",
  "direction",
  "wordBreak",
  "overflowWrap",
  "hyphens",
] as const;
