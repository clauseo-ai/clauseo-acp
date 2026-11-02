import { expect, test } from "vitest";
import { isWordFile, wordLock } from "./lock.ts";

const LOCKED = "matter.file_locked";

test("a Word document is a path ending .doc or .docx, in any case", () => {
  for (const p of ["a.doc", "a.docx", "dir/B.DOCX", "x.Doc"]) expect(isWordFile(p), p).toBe(true);
  for (const p of ["a.txt", "a.pdf", "a.docm", "a.docx.txt", "doc", "a.xlsx", "~$a"])
    expect(isWordFile(p), p).toBe(false);
});

test("a lock on one Word document names it", () => {
  expect(wordLock(LOCKED, ["drafts/Reply.docx"])).toBe("drafts/Reply.docx");
  expect(wordLock(LOCKED, ["old.DOC"])).toBe("old.DOC");
});

test("a lock on two files, on one non-Word file, or on none names no file", () => {
  expect(wordLock(LOCKED, ["a.docx", "b.docx"])).toBeNull();
  expect(wordLock(LOCKED, ["brief.txt"])).toBeNull();
  expect(wordLock(LOCKED, [])).toBeNull();
});

test("another refusal names no file, even for one Word document", () => {
  expect(wordLock("matter.turn_running", ["a.docx"])).toBeNull();
  expect(wordLock("internal", ["a.docx"])).toBeNull();
});
