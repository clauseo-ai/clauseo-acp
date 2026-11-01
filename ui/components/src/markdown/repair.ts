// Tail repair for the streaming last block: what to change in its text so that nothing half-arrived shows
// as raw syntax. It reads the block's own parse (micromark's verdict on what is matched), so it never
// guesses at what CommonMark would do with the rest of the text:
// - An emphasis, strikethrough or code-span opener still waiting for its closer gets the closer, after the
//   last character of its paragraph, heading or table cell (before trailing whitespace): the text shows
//   formatted. One with nothing after it yet is hidden. Only a run that can open counts (CommonMark's
//   flanking rules), so `Section 2(a)_ and` gains nothing, and escaped literals (`\*\*`) are never runs.
// - A half link (`[text`, `[text]`, `[text](url`, `[text][ref`) shows its text, plain: never a link that
//   could later turn back into text. A half image, a half footnote reference (`[^lab`) and a trailing
//   backslash are hidden.
// - A last line holding only a block marker (`-`, `1.`, `#`, `>`, `---`, a fence's first backticks) is
//   hidden until its content arrives; so are the lines of a table whose delimiter row has not arrived.
// A half-arrived HTML tag hides nothing: raw HTML is text here, so it cannot restyle what follows.
import type { Nodes, Parent, Root } from "mdast";

type Edit = { start: number; end: number; text: string };

/**
 * The parts of a text a pass cuts: the edits, and a mask of the characters they remove, so whether a
 * character stays is one lookup however many cuts there are (a hostile `[`×16,000 cuts every one).
 */
type Cuts = {
  readonly edits: Edit[];
  cut(start: number, end: number): void;
  kept(at: number): boolean;
};

function createCuts(length: number): Cuts {
  const edits: Edit[] = [];
  const mask = new Uint8Array(length);
  return {
    edits,
    cut(start, end) {
      edits.push({ start, end, text: "" });
      mask.fill(1, start, end);
    },
    kept: (at) => mask[at] !== 1,
  };
}

/**
 * The openers that get a closer, innermost first: at most this many, and the outer rest stay as they are,
 * as text. Four covers every nesting that reads as prose (strong, emphasis and strikethrough at once, plus
 * one more of another character; no prefix of the legal answer or the spec examples needs more than 3),
 * and keeps micromark's work on the repaired text close to its work on the raw text: it matches each
 * closer against every opener before it, so closing all of `*a `×2,000 took 2 s to parse.
 */
const MAX_CLOSERS = 4;

/** A delimiter run in the block's text: its character, where it starts, and how many are unmatched. */
type Run = { char: string; start: number; length: number };

const LEAVES = new Set(["paragraph", "heading", "tableCell"]);
const DELIMITERS = "*_~`";
const PUNCTUATION = /[\p{P}\p{S}]/u;
const WHITESPACE = /\s/u;
const ASCII_PUNCTUATION = /[!-/:-@[-`{-~]/;

/** A last line that is a block marker and nothing yet: list item, heading, quote, rule, fence start. */
const MARKER_LINE =
  /^(?:[ \t]*>)*[ \t]*(?:[-+*]|\d{1,9}[.)]|#{1,6}|[-=_*]{2,}|`{1,2}|~{1,2}|\[\^[^\]\s]*\]?)[ \t]*$/;
/** A line that may be a table's header row, and a delimiter row still arriving. */
const PIPE_LINE = /^(?:[ \t]*>)*[ \t]*\|/;
const PARTIAL_DELIMITER_ROW = /^(?:[ \t]*>)*[ \t]*\|?[ \t:|-]*$/;
/** Constructs that are only half there at the end of the text. */
const HALF_FOOTNOTE = /\[\^(?:[^[\]\s\\]|\\.)*$/;
const HALF_IMAGE = /!\[(?:[^[\]\\]|\\.)*(?:\](?:\([^)]*|\[[^\]]*)?)?$/;
const HALF_LINK = /\[((?:[^[\]\\]|\\.)*)(?:\](?:\([^)]*|\[[^\]]*)?)?$/;

/** Whether the character at `at` is escaped by an odd run of backslashes before it. */
function escaped(text: string, at: number): boolean {
  let count = 0;
  for (let index = at - 1; index >= 0 && text[index] === "\\"; index--) count++;
  return count % 2 === 1;
}

type Placed = { node: Nodes; start: number; end: number };

/** The plain text of a node without position: its value, or its children's. */
function textOf(node: Nodes): string {
  if ("value" in node) return node.value;
  return "children" in node ? (node.children as Nodes[]).map(textOf).join("") : "";
}

/**
 * Where each child of `parent` sits in `input`. GFM's literal autolinks found after parsing (an email
 * after `[`) split a text node into nodes without positions; those are found by their text, in order,
 * between their positioned neighbours, and left out when not found.
 */
function placed(parent: Parent, input: string): Placed[] {
  const out: Placed[] = [];
  let cursor = parent.position?.start.offset ?? 0;
  for (const child of parent.children as Nodes[]) {
    let start = child.position?.start.offset;
    let end = child.position?.end.offset;
    if (start === undefined || end === undefined) {
      const text = textOf(child);
      const at = text === "" ? -1 : input.indexOf(text, cursor);
      if (at < 0) continue;
      start = at;
      end = at + text.length;
    }
    out.push({ node: child, start, end });
    cursor = end;
  }
  return out;
}

/** The nodes from `root` down to the innermost one whose source holds `offset`. */
function pathTo(root: Root, input: string, offset: number): Nodes[] {
  const path: Nodes[] = [root];
  let node: Nodes = root;
  for (;;) {
    if (!("children" in node)) return path;
    const next: Placed | undefined = placed(node as Parent, input).find(
      (child) => child.start <= offset && offset < child.end,
    );
    if (!next) return path;
    path.push(next.node);
    node = next.node;
  }
}

/** Where a run of `char` around `at` begins and ends in `text`, honouring escapes. */
function runAround(text: string, at: number, char: string, from: number, to: number) {
  let start = at;
  while (start > from && text[start - 1] === char && !escaped(text, start - 1)) start--;
  let end = at;
  while (end < to && text[end] === char) end++;
  return { start, end };
}

type Kind = "whitespace" | "punctuation" | "other";
function kindOf(char: string | undefined): Kind {
  if (char === undefined || WHITESPACE.test(char)) return "whitespace";
  return PUNCTUATION.test(char) ? "punctuation" : "other";
}

/** CommonMark 6.2: whether a delimiter run of `char` with these neighbours can open emphasis. */
function canOpen(char: string, before: string | undefined, after: string | undefined): boolean {
  const b = kindOf(before);
  const a = kindOf(after);
  const left = a !== "whitespace" && (a !== "punctuation" || b !== "other");
  if (char !== "_") return left;
  const right = b !== "whitespace" && (b !== "punctuation" || a !== "other");
  return left && (!right || b === "punctuation");
}

/** The unmatched delimiter runs in the direct text children of `leaf`, in order. */
function runsIn(leaf: Parent, text: string, from: number): Run[] {
  const runs: Run[] = [];
  const leafStart = leaf.position?.start.offset ?? from;
  const leafEnd = leaf.position?.end.offset ?? text.length;
  for (const { node, start, end } of placed(leaf, text)) {
    if (node.type !== "text") continue;
    for (let at = start; at < end; at++) {
      const char = text[at]!;
      if (char === "\\" && ASCII_PUNCTUATION.test(text[at + 1] ?? "")) {
        at++;
        continue;
      }
      if (!DELIMITERS.includes(char) || escaped(text, at)) continue;
      let length = 1;
      while (at + length < end && text[at + length] === char) length++;
      runs.push({ char, start: at, length });
      at += length - 1;
    }
  }
  // The whole run in the source decides flanking: part of it may belong to a matched neighbour.
  return runs.filter((run) => run.start >= from && run.start < leafEnd && run.start >= leafStart);
}

/** Inline nodes a marker belongs to once it is matched: such a line is a closer, not a new block. */
const MATCHED = new Set(["inlineCode", "strong", "emphasis", "delete"]);
/** After other edits: a last line left holding only a list or heading marker. */
const BARE_MARKER = /\n(?:[ \t]*>)*[ \t]*(?:[-+*]|\d{1,9}[.)]|#{1,6})[ \t]*$/;

/** Blocks whose lines are what they are: nothing in them is a marker, and no table forms there. */
const LITERAL = new Set(["code", "html"]);

/**
 * Where the last lines that must wait start, or -1: a lone block marker, and a pipe-led line that may be
 * a table's header row (with the delimiter row still arriving after it). A table can form only where the
 * pipe-led line's text is paragraph text, a paragraph it opens or one it continues (a table interrupts a
 * paragraph), never in code, HTML or a table already there. Such a line stays hidden until the next line
 * shows whether it is a table, so a stream that pauses after it shows it only once more text arrives.
 */
function hiddenLines(input: string, root: Root, from: number, end: number): number {
  if (end <= from) return -1;
  const lineStart = Math.max(from, input.lastIndexOf("\n", end - 1) + 1);
  const line = input.slice(lineStart, end);
  const pathAt = (start: number, text: string) =>
    pathTo(root, input, start + Math.max(0, text.search(/[^ \t>]/)));
  const tableMayForm = (start: number, text: string) => {
    const path = pathAt(start, text);
    return (
      path.some((node) => node.type === "paragraph") &&
      !path.some((node) => node.type === "table" || LITERAL.has(node.type))
    );
  };
  if (PARTIAL_DELIMITER_ROW.test(line) && lineStart > from) {
    const previousStart = Math.max(from, input.lastIndexOf("\n", lineStart - 2) + 1);
    const previous = input.slice(previousStart, lineStart - 1);
    if (PIPE_LINE.test(previous) && tableMayForm(previousStart, previous)) return previousStart;
  }
  if (PIPE_LINE.test(line) && tableMayForm(lineStart, line)) return lineStart;
  // A marker line still open at the end (no line ending after it), unless it closes something or is
  // code or HTML.
  if (!MARKER_LINE.test(line) || input.includes("\n", end)) return -1;
  const path = pathAt(lineStart, line);
  const inner = path.at(-1);
  if (path.some((node) => LITERAL.has(node.type))) return -1;
  return inner && MATCHED.has(inner.type) ? -1 : lineStart;
}

/** CommonMark 6.2: whether a run of `char` with these neighbours can close emphasis. */
function canClose(char: string, before: string | undefined, after: string | undefined): boolean {
  const b = kindOf(before);
  const a = kindOf(after);
  const right = b !== "whitespace" && (b !== "punctuation" || a !== "other");
  if (char !== "_") return right;
  const left = a !== "whitespace" && (a !== "punctuation" || b !== "other");
  return right && (!left || a === "punctuation");
}

/**
 * The first pass: what is only half there at the end is cut. The last lines that must wait, a trailing
 * backslash, a half footnote reference or image, and a half link's syntax (its text stays), plus any `[` in
 * the last paragraph with no `]` after it. Returns the edits.
 */
function halfConstructs(input: string, root: Root, from: number): Edit[] {
  const cuts = createCuts(input.length);
  const { edits, cut } = cuts;
  let end = keptEnd(input, from, cuts);
  const hideFrom = hiddenLines(input, root, from, end);
  if (hideFrom >= 0) cut(hideFrom, input.length);
  end = keptEnd(input, from, cuts);
  // A trailing backslash in text: the start of an escape, or of a hard break (then one line ending after it).
  if (
    end > from &&
    input[end - 1] === "\\" &&
    !escaped(input, end - 1) &&
    pathTo(root, input, end - 1).at(-1)?.type === "text"
  ) {
    const after = input.slice(end, hideFrom >= 0 ? hideFrom : input.length);
    if (/^(?:\n[ \t]*)?$/.test(after)) cut(end - 1, end);
  }
  end = keptEnd(input, from, cuts);
  const head = input.slice(from, end);
  const inText = (at: number, reference = false) => {
    if (escaped(input, at)) return false;
    const inner = pathTo(root, input, at).at(-1);
    if (inner?.type === "text") return true;
    return (
      reference &&
      (inner?.type === "linkReference" || inner?.type === "imageReference") &&
      inner.position?.start.offset === at
    );
  };
  // `[text]` alone may be a link waiting for its destination, if nothing at all follows it yet.
  const open = end === input.length;
  const footnote = HALF_FOOTNOTE.exec(head);
  const image = HALF_IMAGE.exec(head);
  const link = HALF_LINK.exec(head);
  if (footnote && inText(from + footnote.index)) cut(from + footnote.index, end);
  else if (
    image &&
    !escaped(input, from + image.index) &&
    inText(from + image.index + 1, true) &&
    (open || !image[0].endsWith("]"))
  )
    cut(from + image.index, end);
  else if (link) {
    const start = from + link.index;
    const close = start + 1 + link[1]!.length;
    // `[text]` and nothing more (an empty one is no link).
    const bare = link[0].length === link[1]!.length + 2;
    if (bare && link[1] === "") return edits;
    const half =
      close >= end
        ? inText(start)
        : bare
          ? open && inText(start)
          : inText(start, true) &&
            (inText(close) || pathTo(root, input, close).at(-1)?.type === "linkReference");
    if (half) {
      cut(start, start + 1);
      if (close < end) cut(close, end);
    }
  }
  end = keptEnd(input, from, cuts);
  const leaf = leafAt(input, root, from, end);
  if (!leaf) return edits;
  // A `[` with no `]` after it in the paragraph's text: a link whose text is still arriving. Cutting a
  // `[` moves no `]`, so the last `]` that stays is found once.
  let lastClose = input.length - 1;
  while (
    lastClose >= 0 &&
    !(input[lastClose] === "]" && cuts.kept(lastClose) && !escaped(input, lastClose))
  )
    lastClose--;
  const stack: Nodes[] = [leaf as Nodes];
  for (let node = stack.pop(); node; node = stack.pop()) {
    if (!("children" in node)) continue;
    for (const { node: child, start, end: stop } of placed(node as Parent, input)) {
      if (child.type !== "text") {
        if (child.type !== "inlineCode") stack.push(child);
        continue;
      }
      for (let at = Math.max(start, lastClose + 1); at < stop; at++)
        if (input[at] === "[" && !escaped(input, at) && cuts.kept(at)) cut(at, at + 1);
    }
  }
  return edits;
}

/** The paragraph, heading or table cell holding `input[end - 1]`, if its inline content runs to there. */
function leafAt(input: string, root: Root, from: number, end: number): Parent | undefined {
  if (end <= from) return undefined;
  let leaf: Nodes | undefined;
  for (const node of pathTo(root, input, end - 1)) if (LEAVES.has(node.type)) leaf = node;
  if (!leaf || !("children" in leaf)) return undefined;
  const children = placed(leaf as Parent, input);
  return (children.at(-1)?.end ?? 0) < end ? undefined : (leaf as Parent);
}

/** The end of what `cuts` keep of `input`, before trailing whitespace. */
function keptEnd(input: string, from: number, cuts: Cuts): number {
  let at = input.length;
  while (at > from && (!cuts.kept(at - 1) || WHITESPACE.test(input[at - 1]!))) at--;
  return at;
}

/**
 * The second pass, on a text with nothing half there: each emphasis, strikethrough or code-span opener of
 * the last paragraph, heading or cell gets its closer at the end; an opener with nothing after it, and a
 * delimiter run at the very end that closes nothing, are cut. Returns the edits and what to insert where.
 */
function closers(input: string, root: Root, from: number) {
  const cuts = createCuts(input.length);
  const { edits, cut } = cuts;
  const leaf = leafAt(input, root, from, keptEnd(input, from, cuts));
  if (!leaf) return { edits, insert: "", end: input.length };
  const leafStart = leaf.position?.start.offset ?? from;
  const leafEnd = leaf.position?.end.offset ?? input.length;
  const runs = runsIn(leaf, input, from);
  const whole = (run: Run) => runAround(input, run.start, run.char, leafStart, leafEnd);
  const code = runs.find((run) => run.char === "`");
  const inCode = (run: Run) => code !== undefined && run.start > code.start;
  const openers = runs.filter((run) => {
    if (run.char === "`" || inCode(run)) return false;
    const { start, end } = whole(run);
    if (run.char === "~" && end - start > 2) return false;
    return canOpen(run.char, input[start - 1], input[end]);
  });
  // A run at the very end: the start of the last opener's closer, or else cut until it shows what it is.
  const last = runs.at(-1);
  let partial = 0;
  if (last && last !== code && !inCode(last) && !openers.includes(last)) {
    const stop = last.start + last.length;
    if (stop === input.length) {
      const opener = openers.at(-1);
      const { start } = whole(last);
      if (
        opener?.char === last.char &&
        last.length < opener.length &&
        canClose(last.char, input[start - 1], undefined)
      )
        partial = last.length;
      else cut(last.start, stop);
    }
  }
  // The last character that is content: kept, not whitespace, and not an opener's or the code span's
  // marker. An opener with no content after it is cut, and so is such a code span's.
  const marker = new Uint8Array(input.length);
  for (const run of code ? [...openers, code] : openers)
    marker.fill(1, run.start, run.start + run.length);
  let lastContent = input.length - 1;
  while (
    lastContent >= 0 &&
    (!cuts.kept(lastContent) || marker[lastContent] === 1 || WHITESPACE.test(input[lastContent]!))
  )
    lastContent--;
  const hasContent = (run: Run) => lastContent >= run.start + run.length;
  if (code && !hasContent(code)) cut(code.start, code.start + code.length);
  const withContent = openers.filter((run) => {
    if (hasContent(run)) return true;
    cut(run.start, run.start + run.length);
    return false;
  });
  const closing = withContent.slice(-MAX_CLOSERS);
  const end = keptEnd(input, from, cuts);
  const codeCloser =
    code && cuts.kept(code.start)
      ? `${input[end - 1] === "`" ? " " : ""}${"`".repeat(code.length)}`
      : "";
  // A closer right after a delimiter that stays (`*foo bar *` and a line ending) would join its run:
  // then the openers show plain.
  const before = input[end - 1]!;
  const joined = "*_~".includes(before)
    ? runAround(input, end - 1, before, leafStart, leafEnd).start
    : -1;
  if (
    closing.length > 0 &&
    !codeCloser &&
    partial === 0 &&
    joined >= 0 &&
    !canClose(before, input[joined - 1], undefined)
  ) {
    for (const run of closing) cut(run.start, run.start + run.length);
    return { edits, insert: "", end: keptEnd(input, from, cuts) };
  }
  let insert = codeCloser;
  for (let index = closing.length - 1; index >= 0; index--) {
    const run = closing[index]!;
    insert += run.char.repeat(index === closing.length - 1 ? run.length - partial : run.length);
  }
  return { edits, insert, end };
}

/** `input` with `edits` applied and `insert` placed at `end`. */
function apply(input: string, edits: Edit[], end: number, insert: string): string {
  edits.sort((a, b) => a.start - b.start);
  let out = "";
  let at = 0;
  let inserted = insert === "";
  for (const edit of edits) {
    if (edit.end <= at) continue;
    const start = Math.max(at, edit.start);
    if (!inserted && end <= start) {
      out += input.slice(at, end) + insert;
      at = end;
      inserted = true;
    }
    out += input.slice(at, start) + edit.text;
    at = edit.end;
  }
  if (!inserted) {
    out += input.slice(at, Math.max(at, end)) + insert;
    at = Math.max(at, end);
  }
  return out + input.slice(at);
}

/**
 * The repaired text of the streaming last block, or undefined when it needs nothing. `input` is what is
 * parsed for the block (definition lines, then the block from offset `from`); `parse` parses such an
 * input (cached by the caller). When the first pass cuts something, its result is parsed again, so the
 * second pass sees what CommonMark makes of the text that stays.
 */
export function repairTail(
  input: string,
  from: number,
  parse: (text: string) => Root,
): string | undefined {
  const cuts = halfConstructs(input, parse(input), from);
  const text = cuts.length === 0 ? input : apply(input, cuts, input.length, "");
  const { edits, insert, end } = closers(
    text,
    cuts.length === 0 ? parse(input) : parse(text),
    from,
  );
  if (cuts.length === 0 && edits.length === 0 && insert === "") return undefined;
  const out = edits.length === 0 && insert === "" ? text : apply(text, edits, end, insert);
  const block = out.slice(from);
  const bare = BARE_MARKER.exec(`\n${block}`);
  return bare ? block.slice(0, bare.index) : block;
}
