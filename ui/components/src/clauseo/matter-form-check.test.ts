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
  expect(createErrorLine("en", "matter.folder_refused", "C:\\")).toBe(
    "A matter is one folder of papers. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_is_drive", "C:\\")).toBe(
    "C:\\ is a whole drive. A matter is one folder of papers. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_is_home")).toBe(
    "It holds far more than one matter. Choose the folder for one matter.",
  );
  expect(createErrorLine("hi", "matter.folder_is_system", "/System")).toBe(
    "/System वह फ़ोल्डर है जिसे कंप्यूटर अपने लिए रखता है। वह फ़ोल्डर चुनें जिसमें मामले के दस्तावेज़ हैं।",
  );
  expect(createErrorLine("en", "matter.folder_unreadable")).toBe(
    "Clauseo can't read that folder. It may be locked, or on a drive that's no longer connected.",
  );
  expect(createErrorLine("en", "command.unknown_matter", "~/Acme")).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
});
