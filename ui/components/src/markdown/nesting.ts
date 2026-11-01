// Limits on hostile input, applied before lexing, so one injected message cannot crash or freeze the
// answer. marked lexes block quotes recursively and mdast-util-to-hast, the sanitiser and the JSX mapping
// walk the tree recursively: `"> ".repeat(3000)` threw "Maximum call stack size exceeded". micromark's
// parse time grows with nesting depth times lines (nesting built through 2,000 columns of tabs: 18.8 s
// for 35 KB) and with the square of an emphasis paragraph's delimiter runs (5,000 `*`, `a`, 5,000 `*`:
// 1.4 s), and with the square of the link and image brackets open at once (5,000 `[`, `a`, 5,000 `](u)`:
// 4.2 s). So, line by line and run by run:
// - A line's leading container prefix (block quote and list markers, spaces and tabs) stops at the 33rd
//   marker or at column MAX_INDENT, whichever comes first: the rest of the line is text.
// - A run of `*`, `_` or `~` keeps its first MAX_RUN characters as delimiters, and a paragraph (text
//   between blank lines) its first MAX_SEQUENCES delimiter sequences; the rest become text. A `~~~` fence
//   line is left alone.
// - A paragraph keeps at most MAX_OPEN_BRACKETS `[` (link or image openers) open at once, and
//   MAX_UNMATCHED_BRACKETS `]` with none open; past those a bracket is text.
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
/** The `*`, `_` and `~` characters one run keeps as delimiters (the longest legitimate run is a few). */
export const MAX_RUN = 64;
/**
 * The `*`, `_` and `~` delimiter sequences one paragraph keeps: micromark matches each closer against
 * every opener before it, so `"a* ".repeat(5000)` took 560 ms (`"a~ "`, 400 to 560 ms); at 400 sequences
 * the worst is a few ms, and a paragraph of advice has tens.
 */
export const MAX_SEQUENCES = 400;
/**
 * The `[` one paragraph may have open at once: micromark looks back over every open label start at each
 * `]`, so `"![".repeat(1000) + "a" + "](u)".repeat(1000)` took 1.9 s. Legitimate text nests a few.
 */
export const MAX_OPEN_BRACKETS = 32;
/**
 * The `]` one paragraph may have with no `[` open: micromark looks back over the whole paragraph for a label
 * start at each, so `"a" + "]".repeat(10000)` took 1.2 s. Legitimate text has a few.
 */
export const MAX_UNMATCHED_BRACKETS = 64;
/**
 * How far a `[` may stay open within a paragraph. GFM's autolink-literal check walks back to an unbalanced
 * `[` at every word, so one `[` left open before 10 KB of prose took 145 ms. Link text is never this long;
 * the longest footnote reference, `[^`, a 999-character label and `]`, closes 1,001 characters on.
 */
export const MAX_BRACKET_SPAN = 1001;
/** The brackets in code spans a paragraph may leave uncounted (`Use \`[\`` in prose, a short fenced block). */
export const MAX_CODE_BRACKETS = 64;
/** Levels of mdast below a block's root: 32 nested lists are 64 (list and item), plus their inline content. */
export const MAX_TREE_DEPTH = 128;

/** The characters a cut replaces, and their stand-ins, the noncharacters U+FDD1 to U+FDDB. */
const CUT = "*_-+>.) \t~[]";
const STAND_IN = "\uFDD1\uFDD2\uFDD3\uFDD4\uFDD5\uFDD6\uFDD7\uFDD8\uFDD9\uFDDA\uFDDB\uFDDC";
const STAND_INS = /[\uFDD1-\uFDDC]/g;
/** A stand-in in a URL, which micromark percent-encodes (U+FDD1 is EF B7 91, U+FDDC is EF B7 9C). */
const ENCODED_STAND_INS = /%EF%B7%9([1-9A-C])/gi;

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

const RUN = /[*_~]+/g;
/** A fenced code block's `~~~` opening or closing line, whose run is no delimiter. */
const TILDE_FENCE = /^ {0,3}~{3,}/;
const BLANK_LINE = /\n[ \t]*\n/;

/** Delimiter sequences in a run of `*`, `_` and `~`: where the character changes, a new one starts. */
function sequencesIn(run: string): number {
  let sequences = 1;
  for (let at = 1; at < run.length; at++) if (run[at] !== run[at - 1]) sequences++;
  return sequences;
}

/** Every run and paragraph of `*`, `_` and `~` delimiters in bounds (MAX_RUN, MAX_SEQUENCES). */
function capRuns(text: string): string {
  let out = "";
  let copied = 0;
  let sequences = 0;
  let previous = 0;
  // The line of the current run, found once per line (a 15 KB line of runs searched once per run was
  // quadratic), and whether it is a thematic break.
  let lineStart = 0;
  let lineEnd = -1;
  let lineIsBreak: boolean | undefined;
  RUN.lastIndex = 0;
  for (let match = RUN.exec(text); match; match = RUN.exec(text)) {
    const run = match[0];
    const at = match.index;
    if (BLANK_LINE.test(text.slice(previous, at))) sequences = 0;
    previous = at + run.length;
    if (at > lineEnd) {
      lineStart = text.lastIndexOf("\n", at) + 1;
      lineEnd = text.indexOf("\n", at);
      if (lineEnd < 0) lineEnd = text.length;
      lineIsBreak = undefined;
    }
    if (run[0] === "~" && at - lineStart <= 3 && TILDE_FENCE.test(text.slice(lineStart, previous)))
      continue;
    let keep = Math.min(run.length, MAX_RUN);
    let kept = sequencesIn(run.slice(0, keep));
    while (sequences + kept > MAX_SEQUENCES && keep > 0) {
      keep--;
      kept = keep === 0 ? 0 : sequencesIn(run.slice(0, keep));
    }
    sequences += kept;
    if (keep === run.length) continue;
    lineIsBreak ??= THEMATIC_BREAK.test(text.slice(lineStart, lineEnd));
    if (lineIsBreak) continue;
    let hidden = "";
    for (let index = keep; index < run.length; index++) hidden += standIn(run[index]!);
    out += text.slice(copied, at + keep) + hidden;
    copied = previous;
  }
  return copied === 0 ? text : out + text.slice(copied);
}

/** An escape, a bracket, a backtick run, or a line break with the next line's quote markers and indentation. */
const BRACKETS = /\\[\s\S]|[[\]]|`+|\n[ \t>]*/g;
const FENCE = /^(?:`{3,}|~{3,})/;

/**
 * Whether a fence starts at `next`, after the line break, quote markers and indentation `prefix`: indented
 * at most 3 columns past the last quote marker and its optional space (more is an indented line, which
 * does not end a paragraph).
 */
function fenceAt(text: string, prefix: string, next: number): boolean {
  const marker = prefix.lastIndexOf(">");
  let indent = 0;
  for (let at = marker >= 0 ? marker + 1 : 1; at < prefix.length; at++)
    indent += prefix[at] === "\t" ? 4 : 1;
  return indent <= (marker >= 0 ? 4 : 3) && FENCE.test(text.slice(next, next + 3));
}

/** The `[` and `]` in `text[start, end)`. */
function bracketsIn(text: string, start: number, end: number): number {
  let count = 0;
  for (let at = start; at < end; at++) {
    const char = text.charCodeAt(at);
    if (char === 91 || char === 93) count++;
  }
  return count;
}
const PARAGRAPH_END = /\n[ \t>]*(?:\n|$)/g;

/**
 * Where the code span opened by the backtick run at `at` ends (after its closing run of the same length,
 * before the paragraph ends), or -1. `missing` remembers, per run length, up to where no closing run
 * exists, so a paragraph of unclosed backticks is searched once per length.
 */
function codeSpanEnd(
  text: string,
  at: number,
  length: number,
  missing: Map<number, number>,
): number {
  const from = at + length;
  if ((missing.get(length) ?? -1) >= from) return -1;
  PARAGRAPH_END.lastIndex = from;
  const end = PARAGRAPH_END.exec(text)?.index ?? text.length;
  let search = from;
  for (;;) {
    const found = text.indexOf("`", search);
    if (found < 0 || found >= end) break;
    let stop = found;
    while (text.charCodeAt(stop) === 96 /* ` */) stop++;
    if (stop - found === length) return stop;
    search = stop;
  }
  missing.set(length, end);
  return -1;
}

/**
 * Every paragraph's brackets in bounds: a `[` past MAX_OPEN_BRACKETS open, a `[` still open
 * MAX_BRACKET_SPAN characters on (not counting the quote markers and indentation that start each line), and
 * a `]` with none open past MAX_UNMATCHED_BRACKETS of them, become text. Brackets in a code span do not
 * count, and a blank line or a code fence line starts the count again. The count is still approximate (a
 * fenced block's lines count, as text, unless its fence closes before a blank line), which only matters
 * past the limits.
 */
function capBrackets(text: string): string {
  if (!text.includes("[") && !text.includes("]")) return text;
  const cuts: number[] = [];
  /** Each open `[`: where it is, and the line-start characters before it. */
  const open: { at: number; skipped: number }[] = [];
  let unmatched = 0;
  let skipped = 0;
  const missing = new Map<number, number>();
  let inCode = 0;
  /** Cut every `[` still open that `at` is more than the span past. */
  const expire = (at: number) => {
    let kept = 0;
    for (const start of open) {
      if (at - start.at - (skipped - start.skipped) > MAX_BRACKET_SPAN) cuts.push(start.at);
      else open[kept++] = start;
    }
    open.length = kept;
  };
  BRACKETS.lastIndex = 0;
  for (let match = BRACKETS.exec(text); match; match = BRACKETS.exec(text)) {
    const token = match[0];
    const at = match.index;
    if (token[0] === "`") {
      const end = codeSpanEnd(text, at, token.length, missing);
      if (end >= 0) {
        // Brackets in a code span are no label starts, but only MAX_CODE_BRACKETS a paragraph go uncounted:
        // a backtick micromark reads otherwise (in raw HTML, say) must not hide thousands of them.
        const inside = bracketsIn(text, at, end);
        if (inCode + inside <= MAX_CODE_BRACKETS) {
          inCode += inside;
          BRACKETS.lastIndex = end;
        }
      }
    } else if (token === "]") {
      expire(at);
      if (open.length > 0) open.pop();
      else if (unmatched < MAX_UNMATCHED_BRACKETS) unmatched++;
      else cuts.push(at);
    } else if (token === "[") {
      expire(at);
      if (open.length < MAX_OPEN_BRACKETS) open.push({ at, skipped });
      else cuts.push(at);
    } else if (token[0] === "\n") {
      expire(at);
      const next = at + token.length;
      if (next >= text.length || text[next] === "\n" || fenceAt(text, token, next)) {
        // A blank line (or a blank quote line) ends the paragraph; a fence line starts or ends code.
        open.length = 0;
        unmatched = 0;
        inCode = 0;
      }
      skipped += token.length;
    }
  }
  expire(text.length);
  if (cuts.length === 0) return text;
  cuts.sort((a, b) => a - b);
  let out = "";
  let copied = 0;
  for (const at of cuts) {
    out += text.slice(copied, at) + standIn(text[at]!);
    copied = at + 1;
  }
  return out + text.slice(copied);
}

/** `text` within every limit above: what the splitter and the parser see. */
export function capNesting(text: string): string {
  const clean = hasStandIns(text) ? text.replace(STAND_INS, "") : text;
  return capBrackets(capRuns(capPrefixes(clean)));
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
    const char = CUT[Number.parseInt(digit, 16) - 1] ?? "";
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
