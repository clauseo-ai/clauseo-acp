// The answer's top-level block split. marked's block lexer proposes where blocks start; a block is cut
// there only where CommonMark parses the two sides separately exactly as it parses them together, and
// otherwise the two merge (merging is always correct, only slower to re-render). The split is
// incremental: when the new text extends the old, every block but the last two is kept and only the
// rest is lexed again. Blocks are slices of the text, so they always concatenate back to it.
import { Lexer, Tokenizer } from "marked";

/**
 * marked's tokenizer without link definitions: marked drops a definition whose label it has seen before from
 * the token stream (its text then belongs to no token), which would make a fresh split of a text differ from
 * an incremental one. As paragraphs, definitions keep their text; cuts are decided below either way.
 */
class WithoutDefinitions extends Tokenizer {
  override def(): undefined {
    return undefined;
  }
}

/** What the parser says about the end of a block parsed on its own. */
export type BlockEnd = {
  /** A top-level fenced code block, or an HTML block of kinds 1 to 5, is still open at the end. */
  open: boolean;
  /** The marker of the block's last top-level node when it is a list ("-", "+", "*", "." or ")"), else "". */
  list: string;
};

/** The parser's verdict on a block's end; the splitter asks only when a cut depends on it. */
export type EndOf = (block: string) => BlockEnd;

/** A list item marker at the start of a line: a bullet, or 1 to 9 digits and `.` or `)`, then a space, tab or line end. */
const LIST_MARKER = /^(?:([-+*])|\d{1,9}([.)]))(?:[ \t\n]|$)/;

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
 * nested in a container close with it, at the column-0 line.
 */
function safeCut(text: string, from: number, at: number, endOf: EndOf): boolean {
  if (!endsWithBlankLine(text, from, at)) return false;
  const first = text.charCodeAt(at);
  if (first === 32 || first === 9 || first === 10) return false;
  const marker = LIST_MARKER.exec(text.slice(at, at + 12));
  const before = text.slice(from, at);
  const mayBeOpen = before.includes("```") || before.includes("~~~") || before.includes("<");
  if (!marker && !mayBeOpen) return true;
  const end = endOf(before);
  if (end.open) return false;
  return !(marker && end.list !== "" && end.list === (marker[1] ?? marker[2]));
}

/**
 * The block starts in `text` at or after `from` (which is itself a block start), using marked's top-level
 * tokens as the places to try. A token whose raw text is not the next slice of the input (marked can add a
 * line ending when it joins tokens, and a streamed list item ending in a space gets `\n` for it) still starts
 * where the tokens before it end, so it may be cut there; it ends the search: the rest stays one block.
 */
function startsFrom(text: string, from: number, endOf: EndOf): number[] {
  const starts = [from];
  const tokens = new Lexer({ gfm: true, tokenizer: new WithoutDefinitions() }).blockTokens(
    text.slice(from),
  );
  let blockStart = from;
  let at = from;
  for (const token of tokens) {
    if (token.type !== "space" && at > blockStart && safeCut(text, blockStart, at, endOf)) {
      starts.push(at);
      blockStart = at;
    }
    if (!text.startsWith(token.raw, at)) break;
    at += token.raw.length;
  }
  return starts;
}

/** One splitter per answer: its state is the previous split, which the next call extends. */
export function createSplitter(endOf: EndOf): (text: string) => readonly string[] {
  let input = "";
  let starts: number[] = [];
  let blocks: readonly string[] = [];
  return (text) => {
    if (text === input && blocks.length > 0) return blocks;
    const keep = blocks.length > 2 && text.startsWith(input) ? blocks.length - 2 : 0;
    const from = keep > 0 ? (starts[keep] ?? 0) : 0;
    const next = starts.slice(0, keep).concat(text.length > 0 ? startsFrom(text, from, endOf) : []);
    const kept = blocks.slice(0, keep);
    const fresh: string[] = [];
    for (let index = keep; index < next.length; index++) {
      fresh.push(text.slice(next[index] ?? 0, next[index + 1] ?? text.length));
    }
    input = text;
    starts = next;
    blocks = kept.concat(fresh);
    return blocks;
  };
}
