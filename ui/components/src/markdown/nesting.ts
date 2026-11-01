// Limits on hostile input, applied before lexing, so one injected message cannot crash or freeze the
// answer. marked lexes block quotes recursively and mdast-util-to-hast, the sanitiser and the JSX mapping
// walk the tree recursively: `"> ".repeat(3000)` threw "Maximum call stack size exceeded". micromark's
// parse time grows with nesting depth times lines (nesting built through 2,000 columns of tabs: 18.8 s
// for 35 KB) and with the square of an emphasis paragraph's delimiter runs (5,000 `*`, `a`, 5,000 `*`:
// 1.4 s). So, line by line and run by run:
// - A line's leading container prefix (block quote and list markers, spaces and tabs) stops at the 33rd
//   marker or at column MAX_INDENT, whichever comes first: the rest of the line is text.
// - A run of `*` and `_` keeps its first MAX_RUN characters as delimiters, and a paragraph (text between
//   blank lines) its first MAX_SEQUENCES delimiter sequences; the rest become text.
// - After parsing, a block whose tree is still deeper than MAX_TREE_DEPTH shows its source as plain text
//   (answer.ts).
// Each cut replaces the character that would open a container or delimit emphasis by a stand-in, a
// Unicode noncharacter that micromark reads as plain text (an escape would add a token per character, and
// a backslash would show in code). restoreText puts the characters back in the rendered text, so code,
// URLs and text read exactly as written; stand-ins in the input are dropped first.
import type { Nodes as Hast } from "hast";
import type { Nodes } from "mdast";

/** Block quotes and list items one line may open with its leading markers. */
export const MAX_NESTING = 32;
/** The column a line's container prefix may reach: 32 levels of `1. ` items, or 64 of `- `. */
export const MAX_INDENT = 64;
/** The `*` and `_` characters one run keeps as delimiters (the longest legitimate run is a few). */
export const MAX_RUN = 64;
/**
 * The `*` and `_` delimiter sequences one paragraph keeps: micromark matches each closer against every
 * opener before it, so `"a* ".repeat(5000)` took 560 ms; at 400 sequences the worst is a few ms, and a
 * paragraph of advice has tens.
 */
export const MAX_SEQUENCES = 400;
/** Levels of mdast below a block's root: 32 nested lists are 64 (list and item), plus their inline content. */
export const MAX_TREE_DEPTH = 128;

/** The characters a cut replaces, and their stand-ins, the noncharacters U+FDD1 to U+FDD9. */
const CUT = "*_-+>.) \t";
const STAND_IN = "\uFDD1\uFDD2\uFDD3\uFDD4\uFDD5\uFDD6\uFDD7\uFDD8\uFDD9";
const STAND_INS = /[\uFDD1-\uFDD9]/g;
/** A stand-in in a URL, which micromark percent-encodes (U+FDD1 is EF B7 91). */
const ENCODED_STAND_INS = /%EF%B7%9([1-9])/gi;

/** `char` (one of CUT) as its stand-in. */
export function standIn(char: string): string {
  return STAND_IN[CUT.indexOf(char)] ?? char;
}

/** Whether `value` holds a stand-in, so its rendering needs restoreText. */
export function hasStandIns(value: string): boolean {
  STAND_INS.lastIndex = 0;
  return STAND_INS.test(value);
}

const THEMATIC_BREAK = /^[ \t>]*([-*_])(?:[ \t]*\1){2,}[ \t]*$/;

/**
 * The character of `text[start, end)` (one line) to replace by its stand-in to keep the line's container
 * prefix in bounds (a marker, its ordered delimiter, or a space or tab), or -1.
 */
function prefixCut(text: string, start: number, end: number): number {
  let count = 0;
  let column = 0;
  let at = start;
  while (at < end) {
    const char = text.charCodeAt(at);
    let next = at + 1;
    let width = 1;
    if (char === 9) width = 4 - (column % 4);
    else if (char === 45 || char === 43 || char === 42 /* - + * */) {
      const after = text.charCodeAt(next);
      if (!(next === end || after === 32 || after === 9)) return -1;
      count++;
    } else if (char >= 48 && char <= 57) {
      while (
        next < end &&
        next - at < 9 &&
        text.charCodeAt(next) >= 48 &&
        text.charCodeAt(next) <= 57
      )
        next++;
      const delimiter = text.charCodeAt(next);
      const after = text.charCodeAt(next + 1);
      if (
        !(delimiter === 46 || delimiter === 41) ||
        !(next + 1 === end || after === 32 || after === 9)
      )
        return -1;
      next++;
      width = next - at;
      count++;
    } else if (char === 62 /* > */) count++;
    else if (char !== 32) return -1;
    if (count > MAX_NESTING || column >= MAX_INDENT) {
      if (THEMATIC_BREAK.test(text.slice(start, end))) return -1;
      return char >= 48 && char <= 57 ? next - 1 : at;
    }
    column += width;
    at = next;
  }
  return -1;
}

/** Every line's container prefix in bounds (prefixCut). */
function capPrefixes(text: string): string {
  let out = "";
  let copied = 0;
  let start = 0;
  while (start < text.length) {
    let end = text.indexOf("\n", start);
    if (end < 0) end = text.length;
    // Shorter than the tabs that reach MAX_INDENT: nothing to cut.
    if (end - start > MAX_INDENT / 4) {
      const at = prefixCut(text, start, end);
      if (at >= 0) {
        out += text.slice(copied, at) + standIn(text[at]!);
        copied = at + 1;
      }
    }
    start = end + 1;
  }
  return copied === 0 ? text : out + text.slice(copied);
}

const RUN = /[*_]+/g;
const BLANK_LINE = /\n[ \t]*\n/;

/** Delimiter sequences in a run of `*` and `_`: where the character changes, a new one starts. */
function sequencesIn(run: string): number {
  let sequences = 1;
  for (let at = 1; at < run.length; at++) if (run[at] !== run[at - 1]) sequences++;
  return sequences;
}

/** Every run and paragraph of `*` and `_` delimiters in bounds (MAX_RUN, MAX_SEQUENCES). */
function capRuns(text: string): string {
  let out = "";
  let copied = 0;
  let sequences = 0;
  let previous = 0;
  RUN.lastIndex = 0;
  for (let match = RUN.exec(text); match; match = RUN.exec(text)) {
    const run = match[0];
    const at = match.index;
    if (BLANK_LINE.test(text.slice(previous, at))) sequences = 0;
    previous = at + run.length;
    let keep = Math.min(run.length, MAX_RUN);
    let kept = sequencesIn(run.slice(0, keep));
    while (sequences + kept > MAX_SEQUENCES && keep > 0) {
      keep--;
      kept = keep === 0 ? 0 : sequencesIn(run.slice(0, keep));
    }
    sequences += kept;
    if (keep === run.length) continue;
    const lineStart = text.lastIndexOf("\n", at) + 1;
    const lineEnd = text.indexOf("\n", at);
    if (THEMATIC_BREAK.test(text.slice(lineStart, lineEnd < 0 ? text.length : lineEnd))) continue;
    let hidden = "";
    for (let index = keep; index < run.length; index++) hidden += standIn(run[index]!);
    out += text.slice(copied, at + keep) + hidden;
    copied = previous;
  }
  return copied === 0 ? text : out + text.slice(copied);
}

/** `text` within every limit above: what the splitter and the parser see. */
export function capNesting(text: string): string {
  const clean = hasStandIns(text) ? text.replace(STAND_INS, "") : text;
  return capRuns(capPrefixes(clean));
}

/** `value` with the characters capNesting replaced put back. */
export function restore(value: string): string {
  return hasStandIns(value)
    ? value.replace(STAND_INS, (char) => CUT[STAND_IN.indexOf(char)] ?? "")
    : value;
}

/** A URL with the characters capNesting replaced put back (a space or tab stays percent-encoded). */
function restoreUrl(value: string): string {
  return restore(value).replace(ENCODED_STAND_INS, (_, digit: string) => {
    const char = CUT[Number(digit) - 1] ?? "";
    return char === " " ? "%20" : char === "\t" ? "%09" : char;
  });
}

/** Puts back what capNesting replaced in every text and string property of `root`, in place. */
export function restoreText(root: Hast): void {
  const stack: Hast[] = [root];
  for (let node = stack.pop(); node; node = stack.pop()) {
    if (node.type === "text") node.value = restore(node.value);
    else if (node.type === "element") {
      for (const [name, value] of Object.entries(node.properties))
        if (typeof value === "string") node.properties[name] = restoreUrl(value);
    }
    if ("children" in node) for (const child of node.children) stack.push(child);
  }
}

/** Whether `root` has a node more than `limit` levels below it (an iterative walk: the tree may be deep). */
export function deeperThan(root: Nodes, limit: number): boolean {
  const stack: [Nodes, number][] = [[root, 0]];
  for (let item = stack.pop(); item; item = stack.pop()) {
    const [node, depth] = item;
    if (depth > limit) return true;
    if ("children" in node) for (const child of node.children) stack.push([child, depth + 1]);
  }
  return false;
}
