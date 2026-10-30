import { expect, test } from "vitest";
import { runs } from "./runs.ts";
import { ui } from "./words-ui.ts";

const en = (lang: "en" | "hi", s: string) =>
  runs(lang, s)
    .filter((r) => r.en)
    .map((r) => r.text);

test("in English, or with no Latin letter, the text is one run and not marked", () => {
  expect(runs("en", "Lease.docx changed after Clauseo's edit.")).toEqual([
    { text: "Lease.docx changed after Clauseo's edit.", en: false },
  ]);
  expect(runs("hi", "सूची से हटाएँ")).toEqual([{ text: "सूची से हटाएँ", en: false }]);
});

test("wholly Latin text inside Hindi is one English run", () => {
  expect(runs("hi", "Acme Traders v. Kapoor Mills")).toEqual([
    { text: "Acme Traders v. Kapoor Mills", en: true },
  ]);
});

test("every Latin run inside a Hindi string is marked, whole, and the rest is kept as it was", () => {
  const s = ui("hi", "Actions for {name}", { name: "Lease.docx" }) + " Clauseo";
  expect(en("hi", s)).toEqual(["Lease.docx", "Clauseo"]);
  expect(
    runs("hi", s)
      .map((r) => r.text)
      .join(""),
  ).toBe(s);
  expect(en("hi", ui("hi", "Show in File Explorer"))).toEqual(["File Explorer"]);
  expect(en("hi", ui("hi", "Show in Finder"))).toEqual(["Finder"]);
  expect(en("hi", "Claude प्लान की सीमा पूरी हो गई")).toEqual(["Claude"]);
  expect(en("hi", "S.P. Mani का भी हवाला")).toEqual(["S.P. Mani"]);
  expect(en("hi", "a का")).toEqual(["a"]);
});
