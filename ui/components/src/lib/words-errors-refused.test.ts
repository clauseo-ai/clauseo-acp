// matter.file_refused's line, picked from the error's files and refusal: by how much else the act did and why the
// first file was refused, the first file named by its bare name, and errorWords' line with no file to name.
import { expect, test } from "vitest";
import { errorWords, errorWordsOf } from "./words-errors.ts";
import { REFUSED, REFUSED_MIXED, refusedKey, type FileRefusal } from "./words-errors-refused.ts";

const CODE = "matter.file_refused";
/** A name as a line shows it: set apart from the words around it (words-core.ts's isolate). */
const shown = (name: string) => `\u2068${name}\u2069`;

const say = (files: readonly string[] | undefined, refusal?: FileRefusal) =>
  errorWordsOf("en", {
    code: CODE,
    ...(files === undefined ? {} : { files }),
    ...(refusal === undefined ? {} : { refusal }),
  });

const ONE = ["Leases/Lease.docx"];
const TWO = ["Leases/Lease.docx", "Notice.docx"];

test("each of the nine sentences, by how much else the act did and why", () => {
  const cases = [
    [ONE, 1, "permissions", REFUSED.only.permissions],
    [ONE, 1, "second_name", REFUSED.only.second_name],
    [ONE, 1, "saved_copy_unconfirmed", REFUSED.only.saved_copy],
    [ONE, 3, "permissions", REFUSED.one.permissions],
    [ONE, 3, "second_name", REFUSED.one.second_name],
    [ONE, 3, "saved_copy_unconfirmed", REFUSED.one.saved_copy],
    [TWO, 3, "permissions", REFUSED.many.permissions],
    [TWO, 3, "second_name", REFUSED.many.second_name],
    [TWO, 3, "saved_copy_unconfirmed", REFUSED.many.saved_copy],
  ] as const;
  for (const [files, of, because, key] of cases)
    expect(refusedKey({ files, refusal: { because, of } })?.key).toBe(key);
  expect(new Set(cases.map((c) => c[3])).size).toBe(9);
});

test("the sentences as they read, their slots filled", () => {
  expect(say(ONE, { because: "permissions", of: 1 })).toBe(
    `Clauseo couldn't change ${shown("Lease.docx")} because the file is set so it can't be changed. It's as it was.`,
  );
  expect(say(ONE, { because: "second_name", of: 2 })).toBe(
    `Clauseo couldn't change ${shown("Lease.docx")} because the same file also appears somewhere else on this computer. Everything else was done.`,
  );
  expect(say(TWO, { because: "saved_copy_unconfirmed", of: 5 })).toBe(
    `Clauseo left 2 papers as they were, including ${shown("Lease.docx")}, because it couldn't first check its saved copies. Everything else was done; try again to change them too.`,
  );
});

test("a cause this version doesn't know reads as the file's permissions", () => {
  expect(refusedKey({ files: TWO, refusal: { because: "unknown", of: 4 } })?.key).toBe(
    REFUSED.many.permissions,
  );
  expect(refusedKey({ files: ONE, refusal: { because: "unknown", of: 1 } })?.key).toBe(
    REFUSED.only.permissions,
  );
});

test("no refusal reads as an act on that file alone, for its permissions", () => {
  expect(refusedKey({ files: TWO })).toEqual({
    key: REFUSED.only.permissions,
    slots: { name: shown("Lease.docx"), count: 2 },
  });
});

test("the first file is named by its bare name, from a nested path, either separator", () => {
  expect(refusedKey({ files: ["a/b/c/Deed.docx"] })?.slots["name"]).toBe(shown("Deed.docx"));
  expect(refusedKey({ files: ["a\\b\\Deed.docx", "z.docx"] })?.slots["name"]).toBe(
    shown("Deed.docx"),
  );
});

test("with no file to name, the line never shows a slot: it reads as internal does", () => {
  const internal = errorWords("en", "internal");
  expect(refusedKey({})).toBeNull();
  expect(say(undefined, { because: "permissions", of: 1 })).toBe(internal);
  expect(say([])).toBe(internal);
  expect(errorWords("en", CODE)).toBe(internal);
  expect(errorWords("hi", CODE)).not.toMatch(/\{\w+\}/);
});

test("several files refused for different reasons read as the mixed line, which gives no reason", () => {
  expect(refusedKey({ files: TWO, refusal: { refused: 2, of: 3 } })?.key).toBe(REFUSED_MIXED);
  expect(say(TWO, { refused: 2, of: 3 })).toBe(
    `Clauseo couldn't change 2 papers, including ${shown("Lease.docx")}, for different reasons. Everything else was done.`,
  );
  // one file with no reason still reads as its permissions
  expect(refusedKey({ files: ONE, refusal: { refused: 1, of: 3 } })?.key).toBe(
    REFUSED.one.permissions,
  );
  expect(refusedKey({ files: ONE, refusal: { refused: 1, of: 1 } })?.key).toBe(
    REFUSED.only.permissions,
  );
});

test("{count} is the refusal's count of refused files, beyond the files the error names", () => {
  expect(
    refusedKey({ files: TWO, refusal: { because: "permissions", refused: 73, of: 90 } }),
  ).toEqual({
    key: REFUSED.many.permissions,
    slots: { name: shown("Lease.docx"), count: 73 },
  });
  // one file named of many refused reads as many, not one
  expect(refusedKey({ files: ONE, refusal: { refused: 60, of: 90 } })).toEqual({
    key: REFUSED_MIXED,
    slots: { name: shown("Lease.docx"), count: 60 },
  });
});

test("a reason all the refused files share picks that reason's line", () => {
  expect(
    refusedKey({ files: TWO, refusal: { because: "second_name", refused: 5, of: 9 } })?.key,
  ).toBe(REFUSED.many.second_name);
  expect(
    refusedKey({ files: TWO, refusal: { because: "saved_copy_unconfirmed", refused: 2, of: 2 } })
      ?.key,
  ).toBe(REFUSED.many.saved_copy);
});

test("another code's line from the whole error is errorWords'", () => {
  expect(errorWordsOf("en", { code: "matter.file_locked", files: ONE })).toBe(
    errorWords("en", "matter.file_locked"),
  );
});
