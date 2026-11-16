// matter.file_write_failed's line, picked from the error's files and write failure: put back with nothing else
// changed, put back after other papers may have changed, or possibly damaged (also with no write failure), the file
// named by its bare name, and errorWords' internal line with no file to name.
import { expect, test } from "vitest";
import { errorWords, errorWordsOf, type ErrorCode } from "./words-errors.ts";
import type { WriteFailure } from "./words-errors-refused.ts";
import { WRITTEN, writtenKey } from "./words-errors-written.ts";

const CODE = "matter.file_write_failed" satisfies ErrorCode;
/** A name as a line shows it: set apart from the words around it (words-core.ts's isolate). */
const shown = (name: string) => `\u2068${name}\u2069`;

const say = (files: readonly string[] | undefined, write_failure?: WriteFailure) =>
  errorWordsOf("en", {
    code: CODE,
    ...(files === undefined ? {} : { files }),
    ...(write_failure === undefined ? {} : { write_failure }),
  });

const FILE = ["Leases/Lease.docx"];

test("each of the three lines, by whether the file was put back and whether other files changed", () => {
  expect(writtenKey({ files: FILE, write_failure: { put_back: true, of: 1 } })?.key).toBe(
    WRITTEN["only.put_back"],
  );
  expect(writtenKey({ files: FILE, write_failure: { put_back: true, of: 3 } })?.key).toBe(
    WRITTEN["others.put_back"],
  );
  expect(writtenKey({ files: FILE, write_failure: { put_back: false, of: 1 } })?.key).toBe(
    WRITTEN.damaged,
  );
  expect(writtenKey({ files: FILE, write_failure: { put_back: false, of: 3 } })?.key).toBe(
    WRITTEN.damaged,
  );
});

test("the lines as they read, the file's bare name filled in", () => {
  expect(say(FILE, { put_back: true, of: 1 })).toBe(
    `Clauseo couldn't finish changing ${shown("Lease.docx")}, so it put the file back as it was. Nothing else was changed.`,
  );
  expect(say(FILE, { put_back: true, of: 2 })).toBe(
    `Clauseo couldn't finish changing ${shown("Lease.docx")}, so it put that file back as it was. It may already have changed other papers. To put them back, restore the version from just before this in Versions.`,
  );
  expect(say(FILE, { put_back: false, of: 2 })).toBe(
    `Clauseo couldn't finish changing ${shown("Lease.docx")}, and the file may be damaged. To put it right, restore the version from just before this in Versions.`,
  );
});

test("with no write failure, the line says the file may be damaged", () => {
  expect(writtenKey({ files: FILE })).toEqual({
    key: WRITTEN.damaged,
    slots: { name: shown("Lease.docx") },
  });
});

test("with no file to name, the line never shows a slot: it reads as internal does", () => {
  const internal = errorWords("en", "internal");
  expect(writtenKey({ write_failure: { put_back: true, of: 1 } })).toBeNull();
  expect(say(undefined, { put_back: true, of: 1 })).toBe(internal);
  expect(say([])).toBe(internal);
  expect(errorWords("en", CODE)).toBe(internal);
  expect(errorWords("hi", CODE)).not.toMatch(/\{\w+\}/);
});
