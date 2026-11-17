// UC's copy review entries for a Word document's marked-up changes in Versions (UC-113 to UC-116, and its notices and
// the Word tool's own refusals, UC-307 to UC-315, UC-368 to UC-371, UC-377 to UC-381, UC-388 to UC-391; its parts'
// headings and the tables whose formatting changed, UC-949 to UC-956 and UC-450 to UC-464), apart from
// registry-uc.tsx to keep that file under the size limit: the notice compared with now, over registry-u8.tsx's host,
// whose word_blackline runs, fails or cuts the document short as each entry asks. UC_ALL is registry-uc.tsx's UC
// entries, then registry-uc-blocks.tsx's UC_BLOCKS, then these, so registry.tsx lists UC in one place.
import { compared } from "./registry-u8.tsx";
import type { Entry } from "./entry.ts";
import { UC_BLOCKS } from "./registry-uc-blocks.tsx";
import { UC } from "./registry-uc.tsx";
import { versionWords } from "./words.ts";
import type { VersionKey } from "@clauseo/components/lib/words-versions.ts";
import type {
  BlacklineNotice,
  BlacklinePart,
  BlacklineParagraph,
  ChangedTable,
} from "../transport/protocol.ts";

const NOTICE = "Notice to the bank.docx";
const COMPARED = {
  compare: [
    {
      path: NOTICE,
      change: "modified" as const,
      size_bytes: 1200,
      may_include_lawyer_edits: false,
    },
  ],
};

/** The host's notices over a marked-up document, each with its line (UC-307 to UC-310, UC-368, UC-389 to UC-391, UC-451). */
const NOTICES: [string, BlacklineNotice, VersionKey][] = [
  ["UC-307", "part_not_shown", "Part of this document couldn't be marked up. The rest is below."],
  [
    "UC-308",
    "hidden_text",
    "This document has text that Word hides. It's shown here like the rest.",
  ],
  ["UC-309", "numbers_may_differ", "Some numbers here may not match the ones Word shows."],
  ["UC-310", "may_differ", "Some of this may look different in Word."],
  [
    "UC-368",
    "changes_outside_text",
    "Some changes aren't shown here, such as in a footnote or header Clauseo couldn't mark up. Open both versions in Word to see them.",
  ],
  [
    "UC-451",
    "header_or_footer_not_compared",
    "A header or footer couldn't be compared, so changes in it aren't shown. Open both versions in Word to see them.",
  ],
  ["UC-389", "comment_changed", "A comment's text changed. Open both versions in Word to see it."],
  [
    "UC-390",
    "text_box_changed",
    "Text in a text box changed. Open both versions in Word to see it.",
  ],
  ["UC-391", "cell_format_changed", "Formatting changed in some tables, rows or cells."],
];

/** A part's one changed paragraph: a word out and a word in. */
const changedPara = (
  before: string,
  out: string,
  into: string,
  after: string,
): BlacklineParagraph => ({
  change: "changed",
  runs: [
    { kind: "same", text: before },
    { kind: "del", text: out },
    { kind: "ins", text: into },
    { kind: "same", text: after },
  ],
});

/** A notice whose footnote, endnote, and each kind of header and footer changed, in one section (UC-949 to
 * UC-956). */
const PARTS: BlacklinePart[] = [
  {
    kind: { footnote: { number: 3 } },
    paragraphs: [changedPara("See the statement of ", "3 March", "5 March", ".")],
  },
  {
    kind: { endnote: { number: 1 } },
    paragraphs: [changedPara("Account no. ", "1102", "1120", ".")],
  },
  ...(["header", "footer"] as const).flatMap((side) =>
    (["default", "first_page", "even_pages"] as const).map((slot): BlacklinePart => ({
      kind:
        side === "header"
          ? { header: { uses: [{ slot, sections: [1] }] } }
          : { footer: { uses: [{ slot, sections: [1] }] } },
      paragraphs: [changedPara("Notice dated ", "1 April", "4 April", "")],
    })),
  ),
];

/** Two sections' headers, each on its own section's pages: their headings say which section (UC-450). */
const SECTIONS: BlacklinePart[] = [1, 2].map((n) => ({
  kind: { header: { uses: [{ slot: "default", sections: [n] }] } },
  paragraphs: [changedPara("Without prejudice ", "(draft)", "(copy)", "")],
}));

/** One table whose formatting changed (UC-454, UC-458, UC-459). */
const ONE_TABLE: ChangedTable[] = [{ place: "body", number: 2, formats: ["borders", "shading"] }];

/** Four tables, each named: one in the body, one in the first page's header, one in the footnotes and one in the
 * endnotes (UC-452, UC-455 to UC-457, UC-460 to UC-464). */
const FOUR_TABLES: ChangedTable[] = [
  { place: "body", number: 1, formats: ["widths", "alignment"] },
  {
    place: { header: { uses: [{ slot: "first_page", sections: [1] }] } },
    number: 1,
    formats: ["merged_cells"],
  },
  { place: "footnotes", number: 1, formats: ["row_height"] },
  { place: "endnotes", number: 2, formats: ["other"] },
];

/** Six tables: the first three named, then how many more (UC-453). */
const SIX_TABLES: ChangedTable[] = [
  ...ONE_TABLE,
  ...FOUR_TABLES,
  {
    place: { footer: { uses: [{ slot: "default", sections: [1] }] } },
    number: 1,
    formats: ["shading"],
  },
];

/** Each part's heading, with its id and the parts that show it. */
const HEADINGS: [string, VersionKey, BlacklinePart[]][] = [
  ["UC-949", "Footnote {n}", PARTS],
  ["UC-950", "Endnote {n}", PARTS],
  ["UC-951", "Header", PARTS],
  ["UC-952", "Header (first page)", PARTS],
  ["UC-953", "Header (even pages)", PARTS],
  ["UC-954", "Footer", PARTS],
  ["UC-955", "Footer (first page)", PARTS],
  ["UC-956", "Footer (even pages)", PARTS],
  ["UC-450", "{part}, section {n}", SECTIONS],
];

/** The table line's words, with their ids and the tables that show them. */
const TABLES: [string, VersionKey, ChangedTable[]][] = [
  ["UC-452", "Formatting changed in {tables}.", FOUR_TABLES],
  ["UC-453", "Formatting changed in {tables}, and in {n} more.", SIX_TABLES],
  ["UC-454", "table {n}", ONE_TABLE],
  ["UC-455", "table {n} in the {part}", FOUR_TABLES],
  ["UC-456", "table {n} in the footnotes", FOUR_TABLES],
  ["UC-457", "table {n} in the endnotes", FOUR_TABLES],
  ["UC-458", "borders", ONE_TABLE],
  ["UC-459", "shading", ONE_TABLE],
  ["UC-460", "widths", FOUR_TABLES],
  ["UC-461", "alignment", FOUR_TABLES],
  ["UC-462", "merged cells", FOUR_TABLES],
  ["UC-463", "row height", FOUR_TABLES],
  ["UC-464", "other formatting", FOUR_TABLES],
];

/** A marked-up notice whose second paragraph couldn't be shown, and whose third holds text Word hides, its formatting
 * alone changed (UC-369, UC-370; "Formatting changed" is the tracked changes' UC-122). */
const ODD_PARAGRAPHS: BlacklineParagraph[] = [
  { change: "same", runs: [{ kind: "same", text: "To the Branch Manager, State Bank of India." }] },
  { change: "not_shown", runs: [] },
  {
    change: "formatted",
    runs: [{ kind: "same", text: "This notice is without prejudice." }],
    hidden: true,
  },
];

/** The Word tool's own refusals, each with its line and no Try again (UC-311 to UC-315). */
const REFUSALS: [string, string, VersionKey][] = [
  [
    "UC-311",
    "blackline.nested_table",
    "Clauseo couldn't compare these two versions because of a table inside a table. Open both in Word to compare them.",
  ],
  [
    "UC-312",
    "blackline.cannot_compare",
    "Clauseo couldn't compare these two versions. Open both in Word to compare them.",
  ],
  [
    "UC-313",
    "blackline.not_readable",
    "Clauseo couldn't read one of these versions. It may be damaged, or it may not be a Word document.",
  ],
  [
    "UC-314",
    "blackline.too_large",
    "This document is too large to mark up. Open both versions in Word to compare them.",
  ],
  [
    "UC-315",
    "blackline.format_unknown",
    "This version of Clauseo can't mark up these changes. Update Clauseo to see them.",
  ],
];

/** A marked-up notice whose changes hold no text of their own: each object the host can name, inserted or deleted
 * (UC-371, UC-377 to UC-381). */
const OBJECT_PARAGRAPHS: BlacklineParagraph[] = [
  {
    change: "changed",
    runs: [
      { kind: "same", text: "Signed: " },
      { kind: "ins", text: "", object: "picture" },
      { kind: "same", text: " Annexed: " },
      { kind: "ins", text: "", object: "chart" },
      { kind: "del", text: "", object: "shape" },
      { kind: "same", text: " As stated" },
      { kind: "del", text: "", object: "note" },
      { kind: "same", text: ", page " },
      { kind: "ins", text: "", object: "field" },
      { kind: "del", text: "", object: "other" },
    ],
  },
];

/** Each object's bracketed word, with its id. */
const OBJECTS: [string, VersionKey][] = [
  ["UC-371", "[picture]"],
  ["UC-377", "[chart]"],
  ["UC-378", "[shape]"],
  ["UC-379", "[footnote]"],
  ["UC-380", "[field]"],
  ["UC-381", "[object]"],
];

export const UC_BLACKLINE: Entry[] = [
  compared("UC-113", versionWords("Marking up the changes…"), { ...COMPARED, mark: "hold" }),
  compared(
    "UC-114",
    versionWords("Clauseo's Word tool is missing. Reinstall Clauseo to see the marked-up changes."),
    { ...COMPARED, mark: { refused: "blackline.no_word_tool", retryable: false } },
  ),
  compared("UC-115", versionWords("The changes in this document couldn't be marked up."), {
    ...COMPARED,
    mark: { refused: "blackline.failed", retryable: true },
  }),
  compared("UC-116", versionWords("This document is very long, so only part of it is shown."), {
    ...COMPARED,
    mark: { truncated: true },
  }),
  ...NOTICES.map(([id, notice, words]) =>
    compared(id, versionWords(words), { ...COMPARED, mark: { notices: [notice] } }),
  ),
  ...HEADINGS.map(([id, words, parts]) =>
    compared(id, versionWords(words), { ...COMPARED, mark: { parts } }),
  ),
  ...TABLES.map(([id, words, changed_tables]) =>
    compared(id, versionWords(words), {
      ...COMPARED,
      mark: { notices: ["cell_format_changed"], changed_tables },
    }),
  ),
  compared("UC-388", versionWords("The web address behind '{text}' changed."), {
    ...COMPARED,
    mark: { changed_links: ["the account"] },
  }),
  ...OBJECTS.map(([id, words]) =>
    compared(id, versionWords(words), { ...COMPARED, mark: { paragraphs: OBJECT_PARAGRAPHS } }),
  ),
  compared("UC-369", versionWords("This part couldn't be shown."), {
    ...COMPARED,
    mark: { paragraphs: ODD_PARAGRAPHS },
  }),
  compared("UC-370", versionWords("Hidden in Word"), {
    ...COMPARED,
    mark: { paragraphs: ODD_PARAGRAPHS },
  }),
  ...REFUSALS.map(([id, code, words]) =>
    compared(id, versionWords(words), { ...COMPARED, mark: { refused: code, retryable: false } }),
  ),
];

/**
 * UC's entries: registry-uc.tsx's (UC-01 to UC-78), registry-uc-blocks.tsx's (UC-80 to UC-106), then
 * the Word blackline's (UC-113 to UC-116, UC-307 to UC-315, UC-368 to UC-371, UC-377 to UC-381, UC-388 to UC-391,
 * UC-450 to UC-464, UC-949 to UC-956).
 */
export const UC_ALL: Entry[] = [...UC, ...UC_BLOCKS, ...UC_BLACKLINE];
