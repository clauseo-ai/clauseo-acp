// The append fast path (#292) for the streaming last block. Most updates only add prose to the end of the
// tail's last paragraph, and micromark parsing the whole tail again was over half of an update. When the
// new parse input is the last update's tail input plus characters that are provably inert text, its mdast
// is the last update's with that paragraph's last text node extended: no new parse. What reads the mdast
// after (tail repair, the nesting depth check, mdast to hast, the tag pairing, the sanitiser) runs on it as
// it does on a parse, so the views stay what a fresh render makes; the tests hold the two equal over random
// streams. Any doubt is a full parse. The caller keeps it to the tail: not a part of a paragraph split.ts
// cut, nor one long enough to be cut, and never after an update whose tail repair found something pending.
import type { Paragraph, Root, Text } from "mdast";
import { parse, parseProbe } from "./parse.ts";

/**
 * The characters that are text whatever comes before or after them in a paragraph's line: letters, their
 * marks and digits of any script, the space, and `. , ; : ' " ? ! - ( ) /` and the danda `।`. Not a tab,
 * not a line ending, and none of CommonMark's or GFM's or the extras' syntax characters (`\` `` ` `` `*`
 * `_` `~` `=` `[` `]` `<` `>` `&` `#` `|` `+` `$` `^` `@`), nor nesting.ts's stand-ins.
 */
const PLAIN = /^[\p{L}\p{M}\p{N} .,;:'"?!()/\u0964-]*$/u;
/**
 * The last character (a code point) is one of PLAIN's. Taken out by code point and tested with PLAIN, not with
 * an end-anchored class (`/[\p{L}…]$/u`): V8 14.6 (Node 26) misses an astral letter at the end with that form,
 * so `Text 𝐀𝐁` read as not plain there and as plain on Node 24.
 */
function plainLast(text: string): boolean {
  const last = Array.from(text.slice(-2)).at(-1);
  return last !== undefined && PLAIN.test(last);
}
/**
 * A line inside a paragraph whose start this is starts no block, however it ends: a letter, after spaces and
 * at most three `*` or `_` or exactly two `~` (`**Clause 8.3** obliges`). A list marker needs a space or
 * tab after it; a thematic break and a setext underline are markers and spaces only; a fence needs three
 * `~` or backticks; a quote, heading, table, HTML block, definition or note starts with its own character;
 * and a letter is no link definition's title. Spaces never make it code: the paragraph is the parse's.
 */
const SETTLED_START = /^ *(?:[*_]{1,3}|~~)?\p{L}/u;
/**
 * What may read characters after it as part of itself, anywhere on the last line: a backslash (an escape),
 * `&` (a character reference ends at a `;`), `@` (an email literal's domain runs on), and a GFM literal
 * autolink's start. micromark-extension-gfm-autolink-literal links `www.` (any case) and `http://` or
 * `https://` (the `:/`) with no brackets, and an email only at its `@`; mdast-util-gfm-autolink-literal's
 * transform finds the same. `xmpp:` and `mailto:` are not links in the version we use, but may be later.
 */
const READS_ON = /[\\&@]|www\.|:\/|xmpp:|mailto:/i;
/** A paragraph that may be a link or footnote definition still arriving (`[a]:\nb "ti` + `tle"`). */
const MAY_DEFINE = /^[ \t]*\[/;
const SPACES = /^ *$/;

type Parsed = { readonly source: string; readonly root: Root };
type Point = NonNullable<Root["position"]>["start"];

/**
 * For the tests only: while `off`, every tail parse is a full one (the oracle the fast path must equal);
 * `check`, when set, sees each mdast the fast path makes, with its source; `replace`, when set, stands in
 * for it (a wrong one, to show the finished render never uses it).
 */
export const appendProbe: {
  off: boolean;
  check?: ((source: string, root: Root) => void) | undefined;
  replace?: ((root: Root) => Root) | undefined;
} = {
  off: false,
};

/** `point` moved along its line to `offset`. */
const along = (point: Point, offset: number): Point => ({
  line: point.line,
  column: point.column + offset - (point.offset ?? 0),
  offset,
});

/**
 * The mdast of `source`, which is `known.source` and more, as `known.root` with its last paragraph's last
 * text node extended; undefined unless that is provably what micromark makes of `source`.
 */
export function appended(known: Parsed, source: string): Root | undefined {
  const old = known.source;
  const delta = source.slice(old.length);
  const lineStart = old.lastIndexOf("\n") + 1;
  // Why the delta is text, as the rest of the paragraph reads it and as it reads the rest:
  // - Blocks: the delta holds no line ending, and the last line it joins was in the paragraph before and
  //   starts with a letter, maybe after spaces and a short emphasis run (SETTLED_START): no list marker
  //   (it needs a space after it), thematic break or setext underline (markers and spaces only), fence
  //   (three `~` or backticks) or other block start, so the paragraph stays one.
  // - Emphasis, strikethrough, highlight, superscript, code spans: the delta holds no delimiter (`*_~=^`,
  //   a backtick), so no opener in the paragraph gains a closer, and a run's flanking (CommonMark 6.2) reads
  //   only the characters on either side of it: the one character whose next character changes is the
  //   one before the delta, which must be allowlisted too, so no run touches the append point.
  // - Escapes, character references, literal autolinks: what reads on past itself (READS_ON) is nowhere
  //   on the last line, so nothing earlier swallows the delta, and the delta starts none.
  // - Links, images, tags, autolinks in angle brackets: the delta holds no `[` `]` `<` `>`, so it closes
  //   only an inline link's destination or title, and that only with a `)`: no `)` in the delta when the
  //   paragraph holds a `](`. Settled links do not settle it: `x [a [b](c "t](d) more` + `")` is a link to
  //   c, inside what was a link to d.
  // - Definitions: the paragraph does not start with `[` (MAY_DEFINE), and does not start on the line
  //   after a definition (`[a]: b\n"multi\nline` + `"` is that definition's title).
  // - The paragraph's last child is a text node running to the end, so the delta joins that text, as
  //   micromark joins adjacent text; tail repair, the depth check, hast, tags and the sanitiser run after.
  if (delta === "" || lineStart === old.length || !PLAIN.test(delta)) return undefined;
  if (!plainLast(old)) return undefined;
  const line = source.slice(lineStart);
  if (!SETTLED_START.test(line) || READS_ON.test(line)) return undefined;
  const { root } = known;
  const paragraph = root.children.at(-1);
  if (paragraph?.type !== "paragraph") return undefined;
  const text = paragraph.children.at(-1);
  if (text?.type !== "text") return undefined;
  const [r, p, t] = [root.position, paragraph.position, text.position];
  const textEnd = t?.end.offset;
  if (!r || !p || !t || textEnd === undefined || p.start.offset === undefined) return undefined;
  // The text runs to the end of the source but for the spaces a paragraph's last line ends with.
  if (r.end.offset !== old.length || p.end.offset !== old.length || textEnd <= lineStart)
    return undefined;
  const gap = old.slice(textEnd);
  if (!SPACES.test(gap) || MAY_DEFINE.test(old.slice(p.start.offset, p.start.offset + 8)))
    return undefined;
  if (delta.includes(")") && old.includes("](", p.start.offset)) return undefined;
  const before = root.children.at(-2);
  if (before?.type === "definition" && (before.position?.end.line ?? Infinity) >= p.start.line - 1)
    return undefined;
  const value = (text.value + gap + delta).replace(/ +$/, "");
  const end = textEnd + value.length - text.value.length;
  const extended: Text = {
    type: "text",
    value,
    position: { start: { ...t.start }, end: along(t.end, end) },
  };
  const grown: Paragraph = {
    ...paragraph,
    children: [...paragraph.children.slice(0, -1), extended],
    position: { start: { ...p.start }, end: along(p.end, source.length) },
  };
  return {
    ...root,
    children: [...root.children.slice(0, -1), grown],
    position: { start: { ...r.start }, end: along(r.end, source.length) },
  };
}

/**
 * The tail's parses for one answer: `parse` extends one the last update kept when it can (appended), and
 * parses otherwise. `add` keeps a parse the tail used this update; `end` passes them on to the next update.
 */
export function createAppender() {
  let before: Parsed[] = [];
  let now: Parsed[] = [];
  let open = false;
  let pending = false;
  const fast = new WeakSet<Root>();
  let kept: readonly string[] = [];
  return {
    /** Whether this update's tail may extend the last update's (the caller's limits, above). */
    open(may: boolean) {
      open = may;
    },
    /** The tail repair found something pending: the next update parses its tail whole. */
    pending() {
      pending = true;
    },
    parse(source: string): Root {
      let known: Parsed | undefined;
      if (open && !appendProbe.off)
        for (const one of before)
          if (
            source.length > one.source.length &&
            source.startsWith(one.source) &&
            one.source.length > (known?.source.length ?? -1)
          )
            known = one;
      const made = known && appended(known, source);
      if (!made) return parse(source);
      const counts = parseProbe.counts;
      if (counts) counts.fast++;
      appendProbe.check?.(source, made);
      const root = appendProbe.replace ? appendProbe.replace(made) : made;
      fast.add(root);
      return root;
    },
    add(source: string, root: Root) {
      if (now.at(-1)?.source !== source) now.push({ source, root });
    },
    /** The sources of the tail parses the fast path made that the last update kept (the tree cache may too). */
    made(): readonly string[] {
      return kept;
    },
    /** No tail parse kept so far is extended (the entity table has arrived). */
    forget() {
      before = [];
    },
    /** The update has ended; the next extends its tail parses only if it streamed and nothing was pending. */
    end(streaming: boolean) {
      kept = now.filter((one) => fast.has(one.root)).map((one) => one.source);
      before = streaming && !pending ? now : [];
      now = [];
      open = false;
      pending = false;
    },
  };
}
