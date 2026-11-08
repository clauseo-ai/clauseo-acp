// R17 PR-E's refused turns that name where: pool_owned and cant_label by `at` and by the kind in the detail, the IT
// line with or without where, {where} set apart in both languages, and every other code as errorWords says it.
import { beforeAll, expect, test } from "vitest";
import { loadHindi } from "./words-core.ts";
import { errorWords, ownershipLine, whereOf } from "./words-errors.ts";
import { failureWords, labelKindOf } from "./words-errors-where.ts";

beforeAll(async () => {
  await loadHindi();
});

const AT = "Pleadings/Draft reply.docx";
const FSI = "\u2068";
const PDI = "\u2069";

test("{where} is the matter path as given, set apart", () => {
  expect(whereOf(AT)).toBe(`${FSI}${AT}${PDI}`);
});

test("pool_owned names where with its IT line; without at it says #712's words", () => {
  expect(failureWords("en", "sandbox.pool_owned", { at: AT })).toBe(
    `${FSI}${AT}${PDI} in this matter still belongs to the Safe workspace, so Clauseo can't answer here. Ask your IT team to give you back ownership of ${FSI}${AT}${PDI}.`,
  );
  expect(failureWords("hi", "sandbox.pool_owned", { at: AT })).toBe(
    `इस मामले में ${FSI}${AT}${PDI} अभी भी सुरक्षित कार्यक्षेत्र का है, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता। अपनी IT टीम से ${FSI}${AT}${PDI} का स्वामित्व आपको वापस देने को कहें।`,
  );
  expect(failureWords("en", "sandbox.pool_owned")).toBe(errorWords("en", "sandbox.pool_owned"));
  expect(failureWords("en", "sandbox.pool_owned", { at: "" })).toBe(
    errorWords("en", "sandbox.pool_owned"),
  );
});

test("the IT line drops where when there is none", () => {
  expect(ownershipLine("en", null)).toBe("Ask your IT team to give you back ownership of it.");
  expect(ownershipLine("hi")).toBe("अपनी IT टीम से इसका स्वामित्व आपको वापस देने को कहें।");
  expect(ownershipLine("en", "Pleadings")).toBe(
    `Ask your IT team to give you back ownership of ${FSI}Pleadings${PDI}.`,
  );
});

test.each([
  [{ detail: "Matter", at: "Pleadings" }, "matter"],
  [{ detail: "matter" }, "matter"],
  [{ detail: "Notes" }, "notes"],
  [{ detail: "notes", at: "Pleadings" }, "notes"],
  [{ detail: "State" }, "other"],
  [{ detail: "ClaudeStore", at: "x" }, "other"],
  [{ detail: "credential_link" }, "other"],
  [{ at: "Pleadings" }, "matter"],
  [{ detail: "access_denied", at: "Pleadings" }, "matter"],
  [{ detail: "access_denied" }, "other"],
  [{}, "other"],
] as const)("cant_label's kind from %j is %s", (place, kind) => {
  expect(labelKindOf(place)).toBe(kind);
});

test("cant_label: Matter with where, Notes, and everything else (Matter without at too) as not prepared", () => {
  expect(failureWords("en", "sandbox.cant_label", { at: "Pleadings" })).toBe(
    `The Safe workspace couldn't be given access to ${FSI}Pleadings${PDI}, so Clauseo can't answer here. If it keeps happening, ask your IT team.`,
  );
  expect(failureWords("hi", "sandbox.cant_label", { detail: "Matter", at: "Pleadings" })).toBe(
    `सुरक्षित कार्यक्षेत्र को ${FSI}Pleadings${PDI} तक पहुँच नहीं दी जा सकी, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता। अगर ऐसा बार-बार हो, तो अपनी IT टीम से पूछें।`,
  );
  expect(failureWords("en", "sandbox.cant_label", { detail: "Notes" })).toBe(
    "The Safe workspace couldn't be given access to this matter's notes, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
  );
  const prepared =
    "The Safe workspace couldn't be prepared for this matter, so Clauseo can't answer here. If it keeps happening, ask your IT team.";
  expect(failureWords("en", "sandbox.cant_label", { detail: "Matter" })).toBe(prepared);
  expect(failureWords("en", "sandbox.cant_label", { detail: "ClaudeStore" })).toBe(prepared);
  expect(failureWords("en", "sandbox.cant_label")).toBe(prepared);
  expect(failureWords("hi", "sandbox.cant_label")).toBe(
    "इस मामले के लिए सुरक्षित कार्यक्षेत्र तैयार नहीं हो सका, इसलिए Clauseo यहाँ उत्तर नहीं दे सकता। अगर ऐसा बार-बार हो, तो अपनी IT टीम से पूछें।",
  );
});

test("every other code says errorWords' sentence, at or no at", () => {
  for (const code of ["sandbox.pool_full", "agent.exited", "a.code_from_later"])
    expect(failureWords("en", code, { at: AT })).toBe(errorWords("en", code));
  expect(failureWords("en", "agent.launch_failed", {}, { kind: "codex", name: null })).toBe(
    errorWords("en", "agent.launch_failed", { kind: "codex", name: null }),
  );
});
