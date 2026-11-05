import { expect, test } from "vitest";
import { isWordFile, lockedFiles, wordLocks } from "./lock.ts";

const LOCKED = "matter.file_locked";

test("a Word document is a path ending .doc or .docx, in any case", () => {
  for (const p of ["a.doc", "a.docx", "dir/B.DOCX", "x.Doc"]) expect(isWordFile(p), p).toBe(true);
  for (const p of ["a.txt", "a.pdf", "a.docm", "a.docx.txt", "doc", "a.xlsx", "~$a"])
    expect(isWordFile(p), p).toBe(false);
});

test("a lock names the files the host names, when it names some", () => {
  const writes = ["a/Lease.docx", "a/Notice.docx", "a/Index.xlsx"];
  expect(lockedFiles({ code: LOCKED, files: ["a/Lease.docx"] }, writes)).toEqual(["a/Lease.docx"]);
  expect(lockedFiles({ code: LOCKED, files: ["a/Lease.docx", "a/Notice.docx"] }, writes)).toEqual([
    "a/Lease.docx",
    "a/Notice.docx",
  ]);
});

test("a lock the host names no files for falls back to the act's file only when it writes one", () => {
  const one = ["a/Lease.docx"];
  expect(lockedFiles({ code: LOCKED }, one)).toEqual(one);
  expect(lockedFiles({ code: LOCKED, files: undefined }, one)).toEqual(one);
  expect(lockedFiles({ code: LOCKED, files: [] }, one)).toEqual(one);
  // several writes: any of them may be the one held, so no file is named and the error map's line says why
  const writes = ["a/Lease.docx", "a/Notice.docx"];
  expect(lockedFiles({ code: LOCKED }, writes)).toEqual([]);
  expect(lockedFiles({ code: LOCKED, files: [] }, writes)).toEqual([]);
  // files the host already named as locked (a restore's preview) stand in for the refusal's own
  expect(lockedFiles({ code: LOCKED }, writes, ["a/Notice.docx"])).toEqual(["a/Notice.docx"]);
  expect(lockedFiles({ code: LOCKED, files: ["a/Lease.docx"] }, writes, ["a/Notice.docx"])).toEqual(
    ["a/Lease.docx"],
  );
});

test("the host's files count only for matter.file_locked", () => {
  const writes = ["a/Lease.docx"];
  expect(lockedFiles({ code: "matter.turn_running", files: ["b/Other.docx"] }, writes)).toEqual(
    writes,
  );
  expect(lockedFiles({ code: "internal", files: ["b/Other.docx"] }, writes)).toEqual(writes);
});

test("a lock on Word documents names them, one or several", () => {
  expect(wordLocks(LOCKED, ["drafts/Reply.docx"])).toEqual(["drafts/Reply.docx"]);
  expect(wordLocks(LOCKED, ["old.DOC"])).toEqual(["old.DOC"]);
  expect(wordLocks(LOCKED, ["a.docx", "b.docx"])).toEqual(["a.docx", "b.docx"]);
});

test("a lock on a file that isn't a Word document, or on no file, names none", () => {
  expect(wordLocks(LOCKED, ["brief.txt"])).toBeNull();
  expect(wordLocks(LOCKED, ["a.docx", "Index.xlsx"])).toBeNull();
  expect(wordLocks(LOCKED, [])).toBeNull();
});

test("another refusal names no file, even for Word documents", () => {
  expect(wordLocks("matter.turn_running", ["a.docx"])).toBeNull();
  expect(wordLocks("internal", ["a.docx", "b.docx"])).toBeNull();
});
