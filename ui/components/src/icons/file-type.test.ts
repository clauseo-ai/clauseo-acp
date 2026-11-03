// A paper's file type by its name: the extension after the last dot, in any case, in any folder; anything else is the
// generic page.
import { expect, test } from "vitest";
import { fileTypeIcon, fileTypeOf } from "./file-type.ts";
import {
  IconFileEmail,
  IconFileGeneric,
  IconFileMarkdown,
  IconFilePdf,
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
  ["Bank statement, March 2001.xlsx", "generic"],
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
  expect(fileTypeIcon("a.xlsx")).toBe(IconFileGeneric);
});
