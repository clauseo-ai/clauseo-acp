// The legal extras (issue clauseo-acp-private#137 §3 and §4): one row per feature lawyers use beyond
// CommonMark and GFM's base, in English, Hindi and Tamil, as G4's legal.md has for the base, and the DOM each
// row's finished render must show (legal-extras-spec.json). Written before any of these features was built,
// so the expectations are what a lawyer should see, not what the code happens to do. One home for the vitest suites here and the
// private real-engine harness, which imports this file by path (fixtures are not package exports).
//
// Choices the expectations encode:
// - `^text^` is superscript only when nothing between the carets is whitespace (Pandoc's rule): `2^10` with no
//   closer, and a footnote's `[^label]`, stay as they are.
// - `~text~` stays GFM strikethrough, so subscript is written `<sub>`; insert and underline are `<ins>` and
//   `<u>` (`++text++` is not supported).
// - The allowlisted inline tags (sup, sub, u, ins, del, s, mark, br, small, kbd) render as their elements
//   with no attributes; any other tag shows as text, as written (`<Party A>`, `<insert date>`).
// - Heading ids follow GitHub's slugs, scoped to the answer (`user-content-<scope>-<slug>`), repeats
//   numbered (`notes`, `notes-1`); a link to `#<slug>` points at its heading.
// - A wide table scrolls inside its own box (`[data-clauseo="table"]`, the table's parent), and its long
//   cells wrap; the wrapping itself needs layout (the private harness).
// - Indian case citations stay plain text of their paragraph: no link, footnote, list, superscript,
//   subscript, emphasis or strikethrough.
import LEGAL_EXTRAS_MD from "./legal-extras.md?raw";
import EXTRAS_JSON from "./legal-extras-spec.json";

export type ExtraScript = "en" | "hi" | "ta";

/** One element inside a block of text: its tag, and its own text (`""` for `br`). */
export type Mark = { tag: string; text: string };

export type ExtraExpect =
  /** The block element (p, li or dd) whose text is `paragraph` holds exactly these elements of TAGS, in order. */
  | { kind: "phrasing"; paragraph: string; marks: Mark[] }
  /** A task list with these items. */
  | { kind: "tasks"; items: { text: string; checked: boolean }[] }
  /** One definition list with these terms, each with its definitions. */
  | { kind: "definitions"; entries: { term: string; definitions: string[] }[] }
  /** The heading, then an ordered list whose items each hold these paragraphs. */
  | {
      kind: "list-paragraphs";
      heading: { level: number; text: string };
      items: string[][];
    }
  /** Block quotes nested inside each other, with each level's own paragraphs, outermost first. */
  | { kind: "quotes"; levels: string[] }
  /** Headings with these slugs, and links (by text) to the heading of each slug. */
  | {
      kind: "anchors";
      headings: { level: number; text: string; slug: string }[];
      links: { text: string; slug: string }[];
    }
  /** A table in its own scroll box, with this header, rows and column alignment. */
  | { kind: "table"; header: string[]; rows: string[][]; align: string[] }
  /** A paragraph of exactly this text with no element inside it at all. */
  | { kind: "plain"; paragraph: string }
  /** A footnote reference after `before` in its paragraph, numbered `number`, whose note reads `note`. */
  | { kind: "footnote"; paragraph: string; number: string; note: string }
  /** A `br` element between the paragraphs `before` and `after`, outside any paragraph. */
  | { kind: "block-break"; before: string; after: string };

export type ExtraEntry = { id: string; feature: string; script: ExtraScript; expect: ExtraExpect };

/** The extras answer's Markdown. */
export const LEGAL_EXTRAS: string = LEGAL_EXTRAS_MD;

/** The tags a "phrasing" row counts: the allowlist, and the base's inline elements a row could gain by mistake. */
export const TAGS = "sup,sub,u,ins,del,s,mark,br,small,kbd,em,strong,a,code";

export const EXTRA_FEATURES: Readonly<Record<string, string>> = {
  X1: "superscript and subscript (statute and section references, ordinals)",
  X2: "strikethrough, insert and underline (redlines)",
  X3: "highlight (marking passages)",
  X4: "task lists (due-diligence checklists)",
  X5: "definition lists (defined terms)",
  X6: "list items with several paragraphs, under any heading level",
  X7: "nested block quotes (quoting judgments)",
  X8: "heading anchors and links within the answer",
  X9: "tables with column alignment and long cells that wrap",
  X10: "Indian case citations stay intact",
  X11: "keys, small print and line breaks (the rest of the allowlist)",
  X12: "tags not on the allowlist show as text (placeholders)",
};

/**
 * Text the finished extras answer must not show (raw syntax and allowlisted tags), outside the paragraphs
 * of LITERALS, which hold some of it on purpose.
 */
export const EXTRA_RAW: readonly string[] = [
  "^",
  "==",
  "~~",
  "<sup",
  "<sub",
  "<u>",
  "<ins",
  "<del",
  "<s>",
  "<mark",
  "<br",
  "<small",
  "<kbd",
  "</",
  "[x]",
  "[ ]",
  "\n: ",
];

/** Paragraphs that show syntax characters or tags on purpose, exactly as written (X1's literal, X12). */
export const LITERALS: readonly string[] = [
  "The escrow holds 2^10 shares, the code samples are in C++, and a == b stays as typed.",
  "This Agreement is made on <insert date> between <Party A> and <Party B>.",
  "यह अनुबंध <insert date> को <Party A> और <Party B> के बीच किया गया है।",
  "இந்த ஒப்பந்தம் <insert date> அன்று <Party A> மற்றும் <Party B> இடையே செய்யப்படுகிறது.",
];

const CITATIONS =
  "(1973) 4 SCC 225|(2019) 3 SCC 1|AIR 1950 SC 124|[1950] SCR 88|2019 SCC OnLine Del 1234".split(
    "|",
  );

/** The citation forms that must stay intact, finished and mid-stream. */
export const CITATION_FORMS: readonly string[] = CITATIONS;

/** The expected DOM of every row, finished render (legal-extras-spec.json). */
export const EXTRAS_SPEC: readonly ExtraEntry[] = EXTRAS_JSON.spec as ExtraEntry[];
