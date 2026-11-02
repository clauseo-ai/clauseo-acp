// The answer's top-level block split. marked's block lexer proposes where blocks start; a block is cut
// there only where CommonMark parses the two sides separately exactly as it parses them together, and
// otherwise the two merge (merging is always correct, only slower to re-render). The split is
// incremental: when the new text extends the old, every block but the last two is kept and only the
// rest is lexed again. Blocks are slices of the text, so they always concatenate back to it. A paragraph
// longer than MAX_PARAGRAPH is the one exception to the merging: it is cut into parts (softCut).
import { Lexer, Tokenizer } from "marked";

/**
 * marked's tokenizer without link definitions or setext headings; as paragraphs, both keep their text, and
 * cuts are decided below either way. marked drops a definition whose label it has seen before from the token
 * stream (its text then belongs to no token), which would make a fresh split of a text differ from an
 * incremental one. marked's setext-heading regex walks its text one group iteration per character, under
 * lookaheads holding repeated groups, which JavaScriptCore's regex JIT does not compile: WebKit interprets it
 * over every paragraph of every update, 18 to 60 ms for 15 KB where V8 takes 0.1 ms (`"a~ ".repeat(5000)`
 * took 98 s to stream in WebKitGTK).
 */
class BlockStartTokenizer extends Tokenizer {
  override def(): undefined {
    return undefined;
  }

  override lheading(): undefined {
    return undefined;
  }
}

/**
 * The characters of one paragraph that one block holds; past this the paragraph is cut (softCut), and each
 * part is a block of its own, parsed and shown as a paragraph of its own. Every update parses the streaming
 * last block whole, so its length bounds an update's work: one 18 KB paragraph of `"a==b==".repeat(3000)`
 * made each update parse 18 KB (in Chromium at a 6x CPU throttle 75 ms a flush on average and up to 700 ms;
 * in WebView2 on 2 vCPUs one flush took 1.2 s, and the one that finished it, as the next block began,
 * 1.5 s). The
 * longest paragraph of the legal answer is 719 characters, of the memo 1,025, of the spec examples 67. A
 * part is longer only when the paragraph has no cut (softCut) in the second half of its MAX_PARAGRAPH
 * characters; it then ends at the first cut after them. A footnote's definition is not cut (its note
 * stays whole).
 */
export const MAX_PARAGRAPH = 4096;

/** What the parser says about the end of a block parsed on its own. */
export type BlockEnd = {
  /** A top-level fenced code block, or an HTML block of kinds 1 to 5, is still open at the end. */
  open: boolean;
  /**
   * The marker of the block's last top-level node when it is a list ("-", "+", "*", "." or ")"), ":" when
   * it is a definition list, else "".
   */
  list: string;
};

/** The parser's verdict on a block's end; the splitter asks only when a cut depends on it. */
export type EndOf = (block: string) => BlockEnd;

/** A list item marker at the start of a line: a bullet, or 1 to 9 digits and `.` or `)`, then a space, tab or line end. */
const LIST_MARKER = /^(?:([-+*])|\d{1,9}([.)]))(?:[ \t\n]|$)/;
/** A definition's marker at the start of a line, which joins the term before it, across a blank line too. */
const DEFINITION = /^:[ \t]/;
/** One ATX heading line, with its line ending. */
const ATX_HEADING = /^ {0,3}#{1,6}(?:[ \t][^\n]*)?\n$/;
/** A line that may hold a definition, so a block may end with a definition list. */
const MAY_DEFINE = /(?:^|\n) {0,3}:[ \t]/;

/** True when `text[from, to)` ends with a line ending followed by a blank line (or is one blank line). */
function endsWithBlankLine(text: string, from: number, to: number): boolean {
  if (to <= from || text.charCodeAt(to - 1) !== 10) return false;
  let at = to - 2;
  while (at >= from && (text.charCodeAt(at) === 32 || text.charCodeAt(at) === 9)) at--;
  return at < from || text.charCodeAt(at) === 10;
}

/**
 * Whether the text may be cut at `at`, the start of a block, with the current block starting at `from`.
 * CommonMark lets nothing reach across such a cut when: the block before ends with a blank line (which
 * ends paragraphs, block quotes, tables, setext and lazy continuation); the next block starts at column 0
 * with a character that is not a space or tab (so no list item, footnote or indented code continues);
 * the next block does not start with a marker that would continue the list the block before ends with;
 * and no fenced code block or HTML block of kinds 1 to 5 is still open at the end of the block before
 * (those are the only blocks that run on past a blank line and a column-0 line). Fences and HTML blocks
 * nested in a container close with it, at the column-0 line. A block that is one ATX heading line and
 * nothing else needs no blank line after it: nothing continues a heading, and nothing before it reaches it
 * (`# a\n# b\n…` with no blank lines would otherwise be one block, parsed whole on every update). A
 * definition list is never cut either: a
 * block starting with a definition's `: ` joins the term before it, and a block after a definition list
 * may be its next term.
 */
function safeCut(text: string, from: number, at: number, last: number, endOf: EndOf): boolean {
  if (
    !endsWithBlankLine(text, from, at) &&
    !(last === from && ATX_HEADING.test(text.slice(from, at)))
  )
    return false;
  const first = text.charCodeAt(at);
  if (first === 32 || first === 9 || first === 10) return false;
  const head = text.slice(at, at + 12);
  if (DEFINITION.test(head)) return false;
  const before = text.slice(from, at);
  // A block whose last token holds a definition, or is indented (a definition's continuation) after one,
  // may end with a definition list: merged, without a parse (a glossary of many terms is one block, and
  // asking the parser at each of its terms would cost the square of its length).
  const lastToken = text.slice(last, at);
  if (MAY_DEFINE.test(lastToken)) return false;
  if ((lastToken.startsWith(" ") || lastToken.startsWith("\t")) && MAY_DEFINE.test(before))
    return false;
  const marker = LIST_MARKER.exec(head);
  const mayBeOpen = before.includes("```") || before.includes("~~~") || before.includes("<");
  if (!marker && !mayBeOpen) return true;
  const end = endOf(before);
  if (end.open || end.list === ":") return false;
  return !(marker && end.list !== "" && end.list === (marker[1] ?? marker[2]));
}

const SPACE = /[ \t\n]/;
const LETTER = /\p{L}/u;
/**
 * A letter, a digit or one of nesting.ts's stand-ins (U+FDD1 to U+FDE0): part of a word, as text reads. A
 * part never starts with a digit (`1. ` would start a list).
 */
const WORD = /[\p{L}\p{N}\uFDD1-\uFDE0]/u;
/** A tag's start (opening or closing), and the tags that start an HTML block running past a blank line. */
const TAG_START = /^<\/?[A-Za-z]/;
const BLOCK_TAG = /^<(?:script|pre|style|textarea)[\s>]/i;
const DIGIT = /\p{N}/u;
const STAND_IN = /[\uFDD1-\uFDE0]/;
/** A footnote definition's start: its note must stay one block. */
const FOOTNOTE_DEFINITION = /^ {0,3}\[\^/;

/**
 * Where to cut a paragraph in (`low`, `high`], or -1: the last place of the first kind there is.
 * 1. After a space, tab or line ending and before a letter: the next part starts with a word, never a
 *    list, heading, quote or fence marker.
 * 2. Before an opening tag's `<` and a letter, then before a closing tag's `</`, not after a backslash
 *    (`"<u>a</u>".repeat(5000)` keeps every pair whole), and never the start of an HTML block of kinds 1 to 5.
 * 3. Between two characters of a word: a letter or a stand-in after a letter, digit or stand-in.
 * 4. Before a stand-in, not after a backslash: what capNesting made text (`"](u)".repeat(5000)`).
 * It reads no further than text[high], so once that has arrived a cut never moves as the text grows. Each
 * kind starts the next part with text, so the part's line starts are the paragraph's own, whose container
 * prefixes capNesting has already bounded. A cut can break what spans it (a link, a highlight, an
 * emphasis) into the text of its two sides; only a paragraph this long, which no answer writes, is cut.
 */
function softCut(text: string, low: number, high: number): number {
  for (let at = high; at > low; at--)
    if (SPACE.test(text[at - 1]!) && LETTER.test(text[at]!)) return at;
  for (const closing of [false, true])
    for (let at = high - 10; at > low; at--)
      if (text[at] === "<" && text[at - 1] !== "\\" && (text[at + 1] === "/") === closing) {
        const tag = text.slice(at, at + 10);
        if (TAG_START.test(tag) && !BLOCK_TAG.test(tag)) return at;
      }
  for (let at = high; at > low; at--)
    if (WORD.test(text[at - 1]!) && WORD.test(text[at]!) && !DIGIT.test(text[at]!)) return at;
  for (let at = high; at > low; at--)
    if (STAND_IN.test(text[at]!) && text[at - 1] !== "\\") return at;
  return -1;
}

/** Block starts, and for each whether it is a cut inside a paragraph (softCut). */
type Starts = { at: number[]; soft: boolean[] };

/**
 * The block starts in `text` at or after `from` (which is itself a block start), using marked's top-level
 * tokens as the places to try. A token whose raw text is not the next slice of the input (marked can add a
 * line ending when it joins tokens, and a streamed list item ending in a space gets `\n` for it) still starts
 * where the tokens before it end, so it may be cut there; it ends the search: the rest stays one block.
 */
function startsFrom(text: string, from: number, endOf: EndOf): Starts {
  const starts = [from];
  const soft = [false];
  const tokens = new Lexer({ gfm: true, tokenizer: new BlockStartTokenizer() }).blockTokens(
    text.slice(from),
  );
  let blockStart = from;
  let at = from;
  // Where the last token before `at` that is not a blank line starts.
  let last = from;
  for (const token of tokens) {
    if (token.type !== "space" && at > blockStart && safeCut(text, blockStart, at, last, endOf)) {
      starts.push(at);
      soft.push(false);
      blockStart = at;
    }
    if (!text.startsWith(token.raw, at)) break;
    if (token.type !== "space") last = at;
    if (token.type === "paragraph" && !FOOTNOTE_DEFINITION.test(token.raw)) {
      // Each part ends at the first cut in the second half of the MAX_PARAGRAPH characters after its
      // start, or in the half after that when there is none, and so on.
      const end = at + token.raw.length;
      let base = Math.max(blockStart, at);
      for (let high = base + MAX_PARAGRAPH; end > high;) {
        const cut = softCut(text, high - MAX_PARAGRAPH / 2, high);
        if (cut < 0) {
          high += MAX_PARAGRAPH / 2;
          continue;
        }
        starts.push(cut);
        soft.push(true);
        blockStart = last = base = cut;
        high = base + MAX_PARAGRAPH;
      }
    }
    at += token.raw.length;
  }
  return { at: starts, soft };
}

/**
 * The block as a string of its own. A slice of a long string can keep the whole of it alive (V8's sliced
 * strings and JSC's substrings point into their parent), and a block cut from one update's text outlives
 * that text: in its views, in the answer's cache keys, and in this splitter's kept blocks. Two 40 KB answers
 * streamed in 1,300 deltas each kept about 250 old copies of the whole answer, 7 to 9 MB in Node. Joining a
 * character first makes the engine flatten the block into a new string, which the slice then points into.
 */
function ownCopy(block: string): string {
  return ` ${block}`.slice(1);
}

/**
 * One splitter per answer: its state is the previous split, which the next call extends. Lexing starts
 * again at a block start that is not a paragraph's cut (marked would read the part after one as a block
 * of its own, which it is not), so a long paragraph is lexed whole on every update, as it was before it
 * was cut; its parts that did not change keep their strings.
 */
export function createSplitter(endOf: EndOf): (text: string) => readonly string[] {
  let input = "";
  let starts: number[] = [];
  let soft: boolean[] = [];
  let blocks: readonly string[] = [];
  return (text) => {
    if (text === input && blocks.length > 0) return blocks;
    const grows = text.startsWith(input);
    let keep = blocks.length > 2 && grows ? blocks.length - 2 : 0;
    while (keep > 0 && soft[keep]) keep--;
    const from = keep > 0 ? (starts[keep] ?? 0) : 0;
    const found = text.length > 0 ? startsFrom(text, from, endOf) : { at: [], soft: [] };
    const next = starts.slice(0, keep).concat(found.at);
    const kept = blocks.slice(0, keep);
    const fresh: string[] = [];
    for (let index = keep; index < next.length; index++) {
      const start = next[index] ?? 0;
      const end = next[index + 1] ?? text.length;
      // A block that was not the last, with the same start and end, is the same slice of the same text.
      const same =
        grows && index + 1 < blocks.length && starts[index] === start && starts[index + 1] === end;
      fresh.push(same ? blocks[index]! : ownCopy(text.slice(start, end)));
    }
    input = text;
    starts = next;
    soft = soft.slice(0, keep).concat(found.soft);
    blocks = kept.concat(fresh);
    return blocks;
  };
}
