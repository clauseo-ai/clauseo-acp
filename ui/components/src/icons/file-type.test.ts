// A paper's file type by its name: the extension after the last dot, in any case, in any folder; anything else is the
// generic page. Its colour on the wall: each coloured type has its own token in day and night, and a dimmed tile's
// twin; everything else is neutral.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { blocks } from "../styles/css-blocks.ts";
import {
  FILE_TONE_TOKEN,
  fileToneOf,
  fileTypeIcon,
  fileTypeOf,
  type FileTone,
} from "./file-type.ts";
import {
  IconFileEmail,
  IconFileExcel,
  IconFileGeneric,
  IconFileMarkdown,
  IconFilePdf,
  IconFilePowerPoint,
  IconFileWord,
} from "./icons.tsx";

test.each([
  ["Reply to the notice.docx", "word"],
  ["OLD BRIEF.DOC", "word"],
  ["Notice to the bank.PDF", "pdf"],
  ["papers/Board minutes, 2 March 2001.pdf", "pdf"],
  ["List of dates.md", "markdown"],
  ["Notes.Markdown", "markdown"],
  ["Email from the bank, 4 March.eml", "email"],
  ["Re: hearing.MSG", "email"],
  ["Bank statement, March 2001.xlsx", "excel"],
  ["Ledger.XLS", "excel"],
  ["Cheque register.csv", "excel"],
  ["Hearing outline.pptx", "powerpoint"],
  ["OLD DECK.PPT", "powerpoint"],
  ["scan.jpeg", "generic"],
  ["README", "generic"],
  [".eml", "generic"],
  ["trailing dot.", "generic"],
  ["annexure.pdf.zip", "generic"],
  ["copy of reply.docx.pdf", "pdf"],
  ["Matters\\Acme\\Reply.docx", "word"],
  ["folder.pdf/notes", "generic"],
  ["  spaced.md  ", "markdown"],
  ["constructor", "generic"],
  ["x.toString", "generic"],
] as const)("%s is %s", (name, type) => {
  expect(fileTypeOf(name)).toBe(type);
});

test("each type has its glyph", () => {
  expect(fileTypeIcon("a.docx")).toBe(IconFileWord);
  expect(fileTypeIcon("a.pdf")).toBe(IconFilePdf);
  expect(fileTypeIcon("a.md")).toBe(IconFileMarkdown);
  expect(fileTypeIcon("a.eml")).toBe(IconFileEmail);
  expect(fileTypeIcon("a.xlsx")).toBe(IconFileExcel);
  expect(fileTypeIcon("a.csv")).toBe(IconFileExcel);
  expect(fileTypeIcon("a.pptx")).toBe(IconFilePowerPoint);
  expect(fileTypeIcon("a.jpg")).toBe(IconFileGeneric);
});

/* ---------- the colour of a file's tab on the wall ---------- */

test.each([
  ["Reply to the notice.docx", "word"],
  ["OLD BRIEF.DOC", "word"],
  ["Notice to the bank.PDF", "pdf"],
  ["Bank statement, March 2001.xlsx", "excel"],
  ["Ledger.xls", "excel"],
  ["Cheque register.CSV", "excel"],
  ["Hearing outline.pptx", "powerpoint"],
  ["Old deck.ppt", "powerpoint"],
  ["Scan of the cheque.jpg", "neutral"],
  ["photo.PNG", "neutral"],
  ["List of dates.md", "neutral"],
  ["Email from the bank.eml", "neutral"],
  ["Transcript.txt", "neutral"],
  ["README", "neutral"],
  ["annexure.pdf.zip", "neutral"],
  ["constructor", "neutral"],
] as const)("%s takes the %s tone", (name, tone) => {
  expect(fileToneOf(name)).toBe(tone);
});

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
const tokens = blocks(read("../styles/tokens-colour.css"));
/** A block's custom properties, by its selector, outside any at-rule. */
const block = (selector: string) =>
  tokens.find((b) => b.selector === selector && b.context === "")?.decls ??
  new Map<string, string>();
const day = block(":root");
const night = block(':root[data-theme="night"]');
const COLOURED = ["word", "pdf", "excel", "powerpoint"] as const satisfies readonly FileTone[];

test("each coloured tone has its own token, set by day and again at night, and neutral keeps the quiet ink", () => {
  expect(FILE_TONE_TOKEN).toEqual({
    word: "--file-word",
    pdf: "--file-pdf",
    excel: "--file-excel",
    powerpoint: "--file-powerpoint",
    neutral: "--quiet",
  });
  for (const tone of COLOURED) {
    const token = FILE_TONE_TOKEN[tone];
    expect(day.get(token), token).toMatch(/^oklch\(/);
    expect(night.get(token), token).toMatch(/^oklch\(/);
    expect(night.get(token), token).not.toBe(day.get(token));
  }
  // the four are four colours
  expect(new Set(COLOURED.map((tone) => day.get(FILE_TONE_TOKEN[tone]))).size).toBe(4);
  expect(new Set(COLOURED.map((tone) => night.get(FILE_TONE_TOKEN[tone]))).size).toBe(4);
});

const wall = blocks(read("../clauseo/wall.css"));
/** The --file-ink a wall.css rule sets, by its selector. */
const ink = (selector: string) =>
  wall.find((b) => b.selector === selector)?.decls.get("--file-ink");

test("the wall paints each coloured tone's token on its tile's tab, and a dimmed tile the token's dimmed twin", () => {
  for (const tone of COLOURED) {
    const token = FILE_TONE_TOKEN[tone];
    expect(ink(`.tile[data-file="${tone}"]`)).toBe(`var(${token})`);
    expect(ink(`.tile[data-away][data-file="${tone}"]`)).toBe(`var(${token}-away)`);
    // the dimmed twin: the colour half taken back to the quiet ink, declared once, so it follows the theme
    expect(day.get(`${token}-away`)).toBe(`color-mix(in oklab, var(${token}) 45%, var(--quiet))`);
    expect(night.has(`${token}-away`)).toBe(false);
  }
  // no rule for neutral: its tile carries no data-file and the glyph keeps the quiet ink
  expect(wall.some((b) => b.selector.includes('data-file="neutral"'))).toBe(false);
  expect(read("../clauseo/wall.css")).toContain(
    '.tile[data-state="uncited"][data-file] .tile-glyph .file-badge {\n    fill: var(--file-ink);',
  );
});
