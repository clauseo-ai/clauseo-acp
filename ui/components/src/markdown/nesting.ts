// Limits on hostile input, applied before lexing, so one injected message cannot crash or freeze the
// answer. marked lexes block quotes recursively and mdast-util-to-hast, the sanitiser and the JSX mapping
// walk the tree recursively: `"> ".repeat(3000)` threw "Maximum call stack size exceeded". micromark's
// parse time grows with nesting depth times lines (nesting built through 2,000 columns of tabs: 18.8 s
// for 35 KB) and with the square of an emphasis paragraph's delimiter runs (5,000 `*`, `a`, 5,000 `*`:
// 1.4 s), and with the square of the link and image brackets open at once (5,000 `[`, `a`, 5,000 `](u)`:
// 4.2 s). So, line by line and run by run:
// - A line's leading container prefix (block quote and list markers, spaces and tabs) stops at the 33rd
//   marker or at column MAX_INDENT, whichever comes first: the rest of the line is text.
// - A run of `*`, `_`, `~`, `=` or `^` keeps its first MAX_RUN characters as delimiters, and a paragraph
//   (text between blank lines) its first MAX_SEQUENCES delimiter sequences; the rest become text. Each `^`
//   is a sequence of its own (a superscript's marker is one caret). A `~~~` fence line and a setext
//   underline are left alone, and so are a run of `=` that is not two long (only `==` delimits highlight)
//   and the caret of a footnote's `[^`.
// - A paragraph keeps at most MAX_OPEN_BRACKETS `[` (link or image openers) open at once, and
//   MAX_UNMATCHED_BRACKETS `]` with none open; past those a bracket is text.
// - A definition list keeps its first MAX_DEFINITIONS definitions; the `:` of each one after that is text,
//   so the rest of a longer glossary shows as paragraphs with a visible `: `.
// - The `<` that would start an HTML block of kinds 1 to 5 (CommonMark 4.6: `<script`, `<pre`, `<style`,
//   `<textarea`, `<!--`, `<?`, `<!` and a letter, `<![CDATA[`) at the start of a line is text. Such a block
//   runs on to its end marker, across blank lines, so one left open (an agent's unclosed `<pre>` or `<!--`)
//   took in the rest of the answer: shown as raw text, one block the splitter cannot cut, parsed whole on
//   every update. Raw HTML only ever shows as text here, so the line reads the same, and what follows it
//   parses as Markdown.
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
/** The `*`, `_`, `~`, `=` and `^` characters one run keeps as delimiters (the longest legitimate run is a few). */
export const MAX_RUN = 64;
/**
 * The `*`, `_`, `~`, `==` and `^` delimiter sequences one paragraph keeps: micromark matches each closer
 * against every opener before it, so `"a* ".repeat(5000)` took 560 ms (`"a~ "`, 400 to 560 ms); at 400
 * sequences the worst is a few ms, and a paragraph of advice has tens. It also bounds the inline elements
 * one paragraph's delimiters make: `"^a^".repeat(6000)` made 6,000 `<sup>`, and streamed in WebView2 on
 * 2 vCPUs a flush took over a second.
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
/**
 * The definitions one definition list keeps (the rest of its `: ` markers are text). The definition-list
 * extension splices each term's events into the middle of the document's, so a list costs the square of
 * its length (in Node, 50 terms 19 ms, 300 terms 276 ms, against 20 ms as paragraphs), and the splitter
 * never cuts one, so a streaming glossary is parsed whole on every update. 32 keeps one under about 10 ms.
 */
export const MAX_DEFINITIONS = 32;
/** Levels of mdast below a block's root: 32 nested lists are 64 (list and item), plus their inline content. */
export const MAX_TREE_DEPTH = 128;

/** The characters a cut replaces, and their stand-ins, the noncharacters U+FDD1 to U+FDE0. */
const CUT = "*_-+>.) \t~[]=:<^";
const STAND_IN =
  "\uFDD1\uFDD2\uFDD3\uFDD4\uFDD5\uFDD6\uFDD7\uFDD8\uFDD9\uFDDA\uFDDB\uFDDC\uFDDD\uFDDE\uFDDF\uFDE0";
const STAND_INS = /[\uFDD1-\uFDE0]/g;
/** A stand-in in a URL, which micromark percent-encodes (U+FDD1 is EF B7 91, U+FDE0 is EF B7 A0). */
const ENCODED_STAND_INS = /%EF%B7%(9[1-9A-F]|A0)/gi;

/** `char` (one of CUT) as its stand-in. */
export function standIn(char: string): string {
  return STAND_IN[CUT.indexOf(char)] ?? char;
}

/** Whether `value` holds a stand-in, so its rendering needs restoreText. */
export function hasStandIns(value: string): boolean {
  STAND_INS.lastIndex = 0;
  return STAND_INS.test(value);
}

/**
 * hasStandIns for a whole answer, which capNesting is given on every update: one indexOf per stand-in is
 * a vectorised scan, and in Node fourteen took 24 µs on an 80 KB memo, where STAND_INS' character
 * class takes 57 µs (on the 30 KB legal answer, 9 µs against 21).
 */
function answerHasStandIns(text: string): boolean {
  for (const char of STAND_IN) if (text.includes(char)) return true;
  return false;
}

const THEMATIC_BREAK = /^[ \t>]*([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
/** A footnote definition's label and colon, as micromark reads one: no whitespace, at most 999 characters. */
const FOOTNOTE_LABEL = /\[\^(?:[^\s[\]\\]|\\[\s\S]){1,999}\]:/y;

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
    else if (char === 91 /* [ */) {
      // A footnote definition's label holds a flow container like a list item's marker does, so the
      // markers after it count on (`[^a]: - - - …` nested as deep as `- - - …`). It indents its content
      // 4 columns.
      FOOTNOTE_LABEL.lastIndex = at;
      if (!FOOTNOTE_LABEL.test(text) || FOOTNOTE_LABEL.lastIndex > end) return -1;
      next = FOOTNOTE_LABEL.lastIndex;
      width = 4;
      count++;
    } else if (char !== 32) return -1;
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

const RUN = /[*_~=]+|\^+/g;
/** A setext heading's `=` underline, whose run is no delimiter. */
const SETEXT_EQUALS = /^ {0,3}=+[ \t]*$/;
const EQUALS_ONLY = /^=+$/;
/** A fenced code block's `~~~` opening or closing line, whose run is no delimiter. */
const TILDE_FENCE = /^ {0,3}~{3,}/;
const BLANK_LINE = /\n[ \t]*\n/;

/**
 * Delimiter sequences in a run of `*`, `_`, `~` and `=`: where the character changes, a new one starts.
 * In a run of `^`, each caret is one.
 */
function sequencesIn(run: string): number {
  if (run[0] === "^") return run.length;
  let sequences = 1;
  for (let at = 1; at < run.length; at++) if (run[at] !== run[at - 1]) sequences++;
  return sequences;
}

/** Every run and paragraph of `*`, `_`, `~`, `=` and `^` delimiters in bounds (MAX_RUN, MAX_SEQUENCES). */
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
    if (run.length !== 2 && EQUALS_ONLY.test(run)) continue;
    // A footnote's `[^` delimits nothing.
    if (run[0] === "^" && text.charCodeAt(at - 1) === 91 /* [ */) continue;
    let keep = Math.min(run.length, MAX_RUN);
    let kept = sequencesIn(run.slice(0, keep));
    while (sequences + kept > MAX_SEQUENCES && keep > 0) {
      keep--;
      kept = keep === 0 ? 0 : sequencesIn(run.slice(0, keep));
    }
    sequences += kept;
    if (keep === run.length) continue;
    lineIsBreak ??=
      THEMATIC_BREAK.test(text.slice(lineStart, lineEnd)) ||
      SETEXT_EQUALS.test(text.slice(lineStart, lineEnd));
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

/** A definition's marker line (after any block quote markers), and a line with nothing in it. */
const DEFINITION_LINE = /^(?:[ \t]*>)*[ \t]{0,3}:[ \t]/;
const EMPTY_LINE = /^[ \t>]*$/;

/**
 * How many lines of `text` may hold a definition (`:` then a space or tab, after nothing but spaces, tabs
 * and `>`), counting no further than `limit + 1`. It runs over the whole answer on every update, so it
 * finds each `:` with indexOf rather than a regex that tries every line start: in Node, a 80 KB memo takes
 * 4.5 µs, where `/(?:^|\n)[ \t>]*:[ \t]/` took 46 µs (a twentieth of a streaming update).
 */
function definitionLines(text: string, limit: number): number {
  let count = 0;
  for (let at = text.indexOf(":"); at >= 0 && count <= limit; at = text.indexOf(":", at + 1)) {
    const next = text.charCodeAt(at + 1);
    if (next !== 32 && next !== 9) continue;
    let back = at - 1;
    for (let char = text.charCodeAt(back); char === 32 || char === 9 || char === 62;)
      char = text.charCodeAt(--back);
    if (back < 0 || text.charCodeAt(back) === 10) count++;
  }
  return count;
}

/**
 * Every definition list within MAX_DEFINITIONS. A list runs on while each paragraph of it (text between
 * blank lines) holds a definition, or is a term whose definition follows the blank line after it.
 */
function capDefinitions(text: string): string {
  // No list can be too long when the whole text has no more definition lines than one list may keep.
  if (definitionLines(text, MAX_DEFINITIONS) <= MAX_DEFINITIONS) return text;
  const lines = text.split("\n");
  let count = 0;
  let defines = false;
  let changed = false;
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index]!;
    if (EMPTY_LINE.test(line)) {
      while (index + 1 < lines.length && EMPTY_LINE.test(lines[index + 1]!)) index++;
      const next = lines[index + 1];
      if (!defines && !(next !== undefined && DEFINITION_LINE.test(next))) count = 0;
      defines = false;
      continue;
    }
    if (!DEFINITION_LINE.test(line)) continue;
    defines = true;
    if (++count <= MAX_DEFINITIONS) continue;
    const colon = line.indexOf(":", line.search(/[^ \t>]/));
    lines[index] = line.slice(0, colon) + standIn(":") + line.slice(colon + 1);
    changed = true;
  }
  return changed ? lines.join("\n") : text;
}

/**
 * The `<` that starts an HTML block of kinds 1 to 5, at the start of a line after any container prefix
 * (quote markers, list markers, spaces and tabs). It may also match a line that is no such block (one in
 * an indented code block, or indented 4 columns past its list item): the stand-in is put back in code and
 * text alike, so that reads the same. Each alternative of the prefix starts with its own character, so a
 * failed match backtracks once per prefix character.
 */
const HTML_BLOCK_START =
  /(?<=^|\n)(?:[ \t>]|[-+*][ \t]|\d{1,9}[.)][ \t])*<(?=(?:script|pre|style|textarea)(?:[ \t>\n]|$)|!--|\?|![A-Za-z]|!\[CDATA\[)/gi;

/** No HTML block of kinds 1 to 5 can start (HTML_BLOCK_START). */
function capHtmlBlocks(text: string): string {
  if (!text.includes("<")) return text;
  return text.replace(HTML_BLOCK_START, (start) => start.slice(0, -1) + standIn("<"));
}

/** `text` within every limit above: what the splitter and the parser see. */
export function capNesting(text: string): string {
  const clean = answerHasStandIns(text) ? text.replace(STAND_INS, "") : text;
  return capDefinitions(capBrackets(capRuns(capPrefixes(capHtmlBlocks(clean)))));
}

/**
 * `value` with the characters capNesting replaced put back. One native replaceAll per stand-in present: a
 * replace with a callback per character took a fifth of each update of `"a==b==".repeat(3000)` in
 * Chromium (thousands of stand-ins in the streaming tail, restored in its source and its text).
 */
export function restore(value: string): string {
  if (!hasStandIns(value)) return value;
  let out = value;
  for (let index = 0; index < STAND_IN.length; index++) {
    const char = STAND_IN[index]!;
    if (out.includes(char)) out = out.replaceAll(char, CUT[index]!);
  }
  return out;
}

/** A URL with the characters capNesting replaced put back (a space or tab stays percent-encoded). */
function restoreUrl(value: string): string {
  return restore(value).replace(ENCODED_STAND_INS, (_, byte: string) => {
    const char = CUT[Number.parseInt(byte, 16) - 0x91] ?? "";
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
