import { expect, test } from "vitest";
import { createErrorLine, nextDateError, parseNextDate } from "./matter-form-check.ts";

test("the next date reads as the lawyer writes it, or as ISO, into a calendar date", () => {
  expect(parseNextDate("30 October 2026")).toBe("2026-10-30");
  expect(parseNextDate("  3 march 2027 ")).toBe("2027-03-03");
  expect(parseNextDate("30 Oct 2026")).toBe("2026-10-30");
  expect(parseNextDate("30 अक्टूबर 2026")).toBe("2026-10-30");
  expect(parseNextDate("2026-10-30")).toBe("2026-10-30");
  expect(parseNextDate("29 February 2028")).toBe("2028-02-29");
});

test("anything else reads as no date: a day that doesn't exist, words, or nothing", () => {
  for (const s of [
    "",
    "next Tuesday",
    "31 November 2026",
    "29 February 2027",
    "2026-13-01",
    "30/10/2026",
    "30 Octo 26",
  ])
    expect(parseNextDate(s), s).toBeNull();
  expect(nextDateError("en", "")).toBeNull();
  expect(nextDateError("en", "30 October 2026")).toBeNull();
  expect(nextDateError("en", "soon")).toBe("Write the date as 30 October 2026.");
  expect(nextDateError("hi", "soon")).toBe("तारीख इस तरह लिखें: 30 अक्टूबर 2026।");
});

test("a refusal says why in the form's words, never the host's message", () => {
  // folder_refused, which doesn't say why: one line true of every reason behind it
  expect(createErrorLine("en", "matter.folder_refused", "C:\\")).toBe(
    "Clauseo can't make a matter of this folder. Choose another folder.",
  );
  expect(createErrorLine("hi", "matter.folder_refused")).toBe(
    "Clauseo इस फ़ोल्डर से मामला नहीं बना सकता। दूसरा फ़ोल्डर चुनें।",
  );
  // a path is isolated between FSI and PDI
  expect(createErrorLine("en", "matter.folder_is_drive", "C:\\")).toBe(
    "\u2068C:\\\u2069 is a whole drive. A matter is one folder of papers. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_unreadable", "a\u202Efdp.exe")).toBe(
    "Clauseo can't read \u2068a\u202Efdp.exe\u2069. It may be locked, or on a drive that's no longer connected.",
  );
  expect(createErrorLine("en", "matter.folder_is_home")).toBe(
    "It holds far more than one matter. Choose the folder for one matter.",
  );
  expect(createErrorLine("hi", "matter.folder_is_system", "/System")).toBe(
    "\u2068/System\u2069 वह फ़ोल्डर है जिसे कंप्यूटर अपने लिए रखता है। वह फ़ोल्डर चुनें जिसमें मामले के दस्तावेज़ हैं।",
  );
  expect(createErrorLine("en", "matter.folder_unreadable")).toBe(
    "Clauseo can't read that folder. It may be locked, or on a drive that's no longer connected.",
  );
  expect(createErrorLine("en", "command.unknown_matter", "Acme")).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
});

test("a folder already a matter names that matter, with Open it beside the line; without a name, no name is said", () => {
  // the matter's name is isolated in the line, as a path is
  expect(createErrorLine("en", "matter.folder_is_matter", "Z:\\Acme", "Acme Ltd v. State")).toBe(
    "This folder is already a matter: \u2068Acme Ltd v. State\u2069.",
  );
  expect(createErrorLine("en", "matter.folder_is_matter", "Z:\\Acme")).toBe(
    "This folder is already a matter.",
  );
  expect(createErrorLine("en", "matter.folder_is_matter", "Z:\\Acme", "")).toBe(
    "This folder is already a matter.",
  );
  expect(createErrorLine("en", "matter.folder_is_matter")).toBe("This folder is already a matter.");
  // a name with right-to-left marks can't reorder the full stop
  expect(createErrorLine("en", "matter.folder_is_matter", "", "a\u202Efdp.exe")).toBe(
    "This folder is already a matter: \u2068a\u202Efdp.exe\u2069.",
  );
  // Hindi, English until the Hindi comes (hi: later)
  expect(createErrorLine("hi", "matter.folder_is_matter", "", "Acme")).toBe(
    "This folder is already a matter: \u2068Acme\u2069.",
  );
});

test("a folder in Clauseo's matters folder is a choose-another refusal, with its path when the form has it", () => {
  expect(createErrorLine("en", "matter.folder_in_matters_root", "Documents/Clauseo/Matters")).toBe(
    "\u2068Documents/Clauseo/Matters\u2069 is the folder Clauseo keeps matters in. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_in_matters_root")).toBe(
    "That folder is where Clauseo keeps matters. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_in_matters_root", "")).toBe(
    "That folder is where Clauseo keeps matters. Choose the folder for one matter.",
  );
});
