// What the blackline sheet says over and in a marked-up Word document, composed from the host's answer (WordBlackline)
// and the words of Versions (words-versions.ts): a line for each notice, the table notice naming its tables, a line for
// each link whose web address changed, why the marking up failed, and the heading over each part after the body. Apart
// from words.ts to keep that file under the size limit. Pure: the language comes in as an argument.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { vt, type VersionKey } from "@clauseo/components/lib/words-versions.ts";
import type {
  BlacklineNotice,
  BlacklinePart,
  ChangedTable,
  HeaderSlot,
  HeaderUse,
  TableFormat,
} from "../../transport/protocol.ts";

/** What the lawyer should know about a marked-up document, by the host's notice, said over the sheet: a part not
 * shown (the rest is), hidden text shown like the rest, numbers that may differ from Word's, changes outside what is
 * shown (a footnote, endnote, header or footer whose view failed, or more changes than the sheet shows), a header or
 * footer compare couldn't compare (in one version only, with no one-to-one partner, or on a section the other
 * version lacks), a comment's or a text box's text or some tables' formatting that changed (which the sheet doesn't
 * mark; tablesLine names the tables when the host does); anything else, and a notice this version doesn't know, says
 * only that some of it may look different in Word. */
export function noticeLine(lang: Lang, notice: BlacklineNotice): string {
  switch (notice) {
    case "part_not_shown":
      return vt(lang, "Part of this document couldn't be marked up. The rest is below.");
    case "hidden_text":
      return vt(lang, "This document has text that Word hides. It's shown here like the rest.");
    case "numbers_may_differ":
      return vt(lang, "Some numbers here may not match the ones Word shows.");
    case "changes_outside_text":
      return vt(
        lang,
        "Some changes are outside what's shown here, such as in a footnote or header that couldn't be shown. Open both versions in Word to see them.",
      );
    case "header_or_footer_not_compared":
      return vt(
        lang,
        "A header or footer couldn't be compared, so changes in it aren't shown. Open both versions in Word to see them.",
      );
    case "comment_changed":
      return vt(lang, "A comment's text changed. Open both versions in Word to see it.");
    case "text_box_changed":
      return vt(lang, "Text in a text box changed. Open both versions in Word to see it.");
    case "cell_format_changed":
      return vt(lang, "The formatting of some tables, rows or cells changed.");
    case "may_differ":
    case "unknown":
      return vt(lang, "Some of this may look different in Word.");
  }
}

/** A header or a footer, as a part or a table's place names it: which, and the pages it is on. */
type Side = { side: "header" | "footer"; uses: readonly HeaderUse[] };

/** A header's or a footer's heading, by the kind of page it is on: one on several kinds takes its first, and one on a
 * kind this version doesn't know, or on none the host names, reads as the plain heading. */
const SIDE_HEADINGS: Readonly<Record<Side["side"], Readonly<Record<HeaderSlot, VersionKey>>>> = {
  header: {
    default: "Header",
    first_page: "Header (first page)",
    even_pages: "Header (even pages)",
    unknown: "Header",
  },
  footer: {
    default: "Footer",
    first_page: "Footer (first page)",
    even_pages: "Footer (even pages)",
    unknown: "Footer",
  },
};

/** The header or footer a part is, or a table is in; null for a note, the body, or a place this version doesn't
 * know. */
function sideOf(kind: BlacklinePart["kind"] | ChangedTable["place"]): Side | null {
  if (typeof kind === "string") return null;
  if ("header" in kind && kind.header !== undefined)
    return { side: "header", uses: kind.header.uses };
  if ("footer" in kind && kind.footer !== undefined)
    return { side: "footer", uses: kind.footer.uses };
  return null;
}

/** The headings of the headers and footers a marked-up document names, in its parts and its tables' places: a heading
 * two of them share, in different sections, adds the first section each is on, as Word names a section's header;
 * one that stands alone says no section. */
function sidesOf(
  lang: Lang,
  parts: readonly BlacklinePart[],
  tables: readonly ChangedTable[],
): (s: Side) => string {
  const base = (s: Side) => vt(lang, SIDE_HEADINGS[s.side][s.uses[0]?.slot ?? "default"]);
  const first = (s: Side) => s.uses[0]?.sections[0];
  const sections = new Map<string, Set<number | undefined>>();
  for (const s of [...parts.map((p) => sideOf(p.kind)), ...tables.map((t) => sideOf(t.place))]) {
    if (s === null) continue;
    const seen = sections.get(base(s)) ?? new Set<number | undefined>();
    seen.add(first(s));
    sections.set(base(s), seen);
  }
  return (s) => {
    const n = first(s);
    return n !== undefined && (sections.get(base(s))?.size ?? 0) > 1
      ? vt(lang, "{part}, section {n}", { part: base(s), n })
      : base(s);
  };
}

/** The heading over each part shown after the body, in the host's order: "Footnote 3", "Endnote 1", "Header (first
 * page)"; null for a part of a kind this version doesn't know, which the sheet shows under no heading. */
export function partHeadings(
  lang: Lang,
  parts: readonly BlacklinePart[],
  tables: readonly ChangedTable[],
): (string | null)[] {
  const sides = sidesOf(lang, parts, tables);
  return parts.map(({ kind }) => {
    if (typeof kind === "string") return null;
    if ("footnote" in kind && kind.footnote !== undefined)
      return vt(lang, "Footnote {n}", { n: kind.footnote.number });
    if ("endnote" in kind && kind.endnote !== undefined)
      return vt(lang, "Endnote {n}", { n: kind.endnote.number });
    const side = sideOf(kind);
    return side === null ? null : sides(side);
  });
}

/** Each kind of table formatting, as the table line names it; one this version doesn't know is `other`'s. */
const FORMAT_WORDS: Readonly<Record<TableFormat, VersionKey>> = {
  borders: "borders",
  shading: "shading",
  widths: "widths",
  alignment: "alignment",
  merged_cells: "merged cells",
  row_height: "row height",
  other: "other formatting",
  unknown: "other formatting",
};

/** One changed table in words: its place and number, then what changed after a colon, "table 2: borders, shading",
 * "table 1 in the header (first page): merged cells"; null for a place this version doesn't know. A table outside the
 * body always says where it is, so it can't be read as the body's. */
function tableWords(lang: Lang, t: ChangedTable, sides: (s: Side) => string): string | null {
  const n = t.number;
  const { place } = t;
  const side = sideOf(place);
  const where =
    place === "body"
      ? vt(lang, "table {n}", { n })
      : place === "footnotes"
        ? vt(lang, "table {n} in the footnotes", { n })
        : place === "endnotes"
          ? vt(lang, "table {n} in the endnotes", { n })
          : side === null
            ? null
            : // the part's heading as a common noun inside the line: "table 1 in the header (first page)"
              vt(lang, "table {n} in the {part}", { part: sides(side).toLocaleLowerCase(lang), n });
  if (where === null) return null;
  const formats = [...new Set(t.formats.map((f) => vt(lang, FORMAT_WORDS[f])))];
  return formats.length === 0 ? where : `${where}: ${formats.join(", ")}`;
}

/** The most tables the table line names before it ends with how many more. */
const TABLES_NAMED = 4;

/** The line for the table notice (cell_format_changed): each table the host names, with what changed in it, the
 * tables apart by semicolons, since each lists its groups with commas, up to four; past four, the first three and how
 * many more, counted from total (the host's count of every changed table, of which it names only the first ones; the
 * tables named when it gives none). A table in a place this version doesn't know counts among the more. With none
 * named, the notice's own line, which names no table. */
export function tablesLine(
  lang: Lang,
  tables: readonly ChangedTable[],
  parts: readonly BlacklinePart[],
  total: number = tables.length,
): string {
  const all = Math.max(total, tables.length);
  const sides = sidesOf(lang, parts, tables);
  const named = tables.map((t) => tableWords(lang, t, sides)).filter((w) => w !== null);
  if (named.length === 0) return noticeLine(lang, "cell_format_changed");
  if (named.length === all && all <= TABLES_NAMED)
    return vt(lang, "Formatting changed in {tables}.", { tables: named.join("; ") });
  const shown = named.slice(0, TABLES_NAMED - 1);
  return vt(lang, "Formatting changed in {tables}; and {n} more.", {
    tables: shown.join("; "),
    n: all - shown.length,
  });
}

/** Over a marked-up document, for a link whose web address changed while its text stayed (the sheet marks no change
 * for it): the link's text, as the document has it, in the line's own quotes. */
export const linkLine = (lang: Lang, text: string): string =>
  vt(lang, "The web address behind '{text}' changed.", { text });

/** The Word tool's refusals that say why, each with its own sentence (words-errors.ts). */
const MARK_REFUSALS: readonly string[] = [
  "blackline.no_word_tool",
  "blackline.nested_table",
  "blackline.cannot_compare",
  "blackline.not_readable",
  "blackline.too_large",
  "blackline.format_unknown",
];

/** Why a Word document's changes couldn't be marked up, by the code (words-errors.ts's sentence): the Word tool
 * missing says to reinstall, and a refusal of the Word tool's own says why (a table inside a table, a version it can't
 * read, one too large); any other failure, the host's or the transport's, says only that it couldn't be done (the
 * sheet that says the file changed stands under it). */
export const markFailedLine = (lang: Lang, code: string): string =>
  errorWords(lang, MARK_REFUSALS.includes(code) ? code : "blackline.failed");
