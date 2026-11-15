import { expect, test } from "vitest";
import { HI } from "../lib/words-errors.hi.ts";
import { HI as MATTERS_HI } from "../lib/words-matters.hi.ts";
import { createErrorLine, locateErrorLine, nestedRefusal } from "./matter-form-check.ts";

const CANT = "Clauseo can't make a matter of this folder. Choose another folder.";
/** A path or name as a line holds it: between FSI and PDI. */
const iso = (text: string) => `\u2068${text}\u2069`;

test("a refusal says why in the form's words, never the host's message", () => {
  // folder_refused, which doesn't say why: one line true of every reason behind it, naming the folder
  expect(createErrorLine("en", "matter.folder_refused", "C:\\")).toBe(
    `Clauseo can't make a matter of ${iso("C:\\")}. Choose another folder.`,
  );
  expect(createErrorLine("hi", "matter.folder_refused")).toBe(
    "Clauseo इस फ़ोल्डर से मामला नहीं बना सकता। दूसरा फ़ोल्डर चुनें।",
  );
  // a path is isolated between FSI and PDI
  expect(createErrorLine("en", "matter.folder_unreadable", "a\u202Efdp.exe")).toBe(
    "Clauseo can't read \u2068~/a\u202Efdp.exe\u2069. It may be locked, or on a drive that's no longer connected.",
  );
  expect(createErrorLine("en", "matter.folder_unreadable")).toBe(
    "Clauseo can't read that folder. It may be locked, or on a drive that's no longer connected.",
  );
  expect(createErrorLine("en", "command.unknown_matter", "Acme")).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
});

/** Each place refusal's line with the folder, and the line from before folder refusals where none is at hand. */
const PLACES = {
  "matter.folder_is_drive": [
    "{path} is the top of a drive. Choose a folder on it.",
    "This is the top of a drive. Choose a folder on it.",
  ],
  "matter.folder_is_home": [
    "{path} is your home folder. Choose a folder inside it.",
    "This is your home folder. Choose a folder inside it.",
  ],
  "matter.folder_is_system": [
    "{path} is a system folder. Choose another folder.",
    "This is a system folder. Choose another folder.",
  ],
  "matter.folder_in_clauseo_data": [
    "Clauseo keeps its own files in {path}. Choose another folder.",
    "This folder is part of Clauseo's own data. Choose another folder.",
  ],
  "matter.folder_too_deep": [
    "{path} is too deeply nested for Windows. Choose a folder with a shorter path.",
    "This folder is too deeply nested for Windows. Choose a shorter path.",
  ],
} as const;

test("each place refusal names the folder and says its reason, in either language; with no folder, the old line", () => {
  for (const [code, [named, bare]] of Object.entries(PLACES)) {
    expect(createErrorLine("en", code, "C:\\Matters\\Acme")).toBe(
      named.replace("{path}", iso("C:\\Matters\\Acme")),
    );
    // home written as "~", as the Folder row writes it
    expect(createErrorLine("hi", code, "Documents/Acme")).toBe(
      MATTERS_HI[named].replace("{path}", iso("~/Documents/Acme")),
    );
    expect(createErrorLine("en", code)).toBe(bare);
    expect(createErrorLine("en", code, "")).toBe(bare);
    // Locate folder says the same line for the same reason
    expect(locateErrorLine("en", code, "/System", undefined, false)).toBe(
      named.replace("{path}", iso("/System")),
    );
  }
  // the matters root keeps its lines
  expect(createErrorLine("hi", "matter.folder_in_matters_root")).toBe(
    `${MATTERS_HI["That folder is where Clauseo keeps matters."]} ${MATTERS_HI["Choose the folder for one matter."]}`,
  );
  // words-errors keeps its own too-deep line, for a turn refused in an open matter
  const TOO_DEEP =
    "This matter's folder is too deep inside other folders for Clauseo to work in it on Windows. Move it nearer the top of its drive.";
  expect(HI[TOO_DEEP]).toBeTruthy();
});

test("a folder nested with one being made, or a forgotten matter's, names the folder; with none, the refused line", () => {
  expect(createErrorLine("en", "matter.folder_being_made", "Documents/Mehta")).toBe(
    `${iso("~/Documents/Mehta")} is inside, or contains, a folder Clauseo is still making into a matter. Try again in a moment, or choose another folder.`,
  );
  expect(createErrorLine("en", "matter.folder_of_forgotten_matter", "D:\\Rao")).toBe(
    `${iso("D:\\Rao")} is inside, or contains, the folder of a matter you removed from the list. Choose another folder.`,
  );
  expect(createErrorLine("en", "matter.folder_being_made")).toBe(CANT);
  expect(createErrorLine("hi", "matter.folder_of_forgotten_matter")).toBe(MATTERS_HI[CANT]);
});

test("a folder refusal with its own line in words-errors gives that line, in the language", () => {
  expect(createErrorLine("en", "matter.folder_network")).toBe(
    "ChatGPT can't work on a network drive. Move the matter to this computer.",
  );
  expect(createErrorLine("en", "matter.something_new")).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
  // a code that isn't a folder refusal keeps the generic line, even where words-errors has one
  expect(createErrorLine("en", "agent.auth_required")).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
});

test("a folder nested with a matter, or one it already uses, names the folder and the matter when the list has it", () => {
  expect(
    createErrorLine("en", "matter.folder_inside_matter", "Z:\\Acme", "Acme Ltd v. State"),
  ).toBe(
    `${iso("Z:\\Acme")} is inside the folder of ${iso("Acme Ltd v. State")}. Choose another folder.`,
  );
  expect(createErrorLine("en", "matter.folder_holds_matter", "Z:\\", "Acme")).toBe(
    `${iso("Z:\\")} contains the folder of ${iso("Acme")}. Choose another folder.`,
  );
  // the folder in use: Open {name}, or another folder
  expect(createErrorLine("en", "matter.folder_is_matter", "Documents/Acme", "Acme")).toBe(
    `${iso("Acme")} already uses ${iso("~/Documents/Acme")}. Open ${iso("Acme")}, or choose another folder.`,
  );
  expect(createErrorLine("hi", "matter.folder_is_matter", "Documents/Acme", "Acme")).toBe(
    MATTERS_HI["{name} already uses {path}. Open {name}, or choose another folder."]
      .replaceAll("{name}", iso("Acme"))
      .replace("{path}", iso("~/Documents/Acme")),
  );
  // a name with right-to-left marks can't reorder the full stop; with no folder, the line names the matter alone
  expect(createErrorLine("en", "matter.folder_inside_matter", "", "a\u202Efdp.exe")).toBe(
    "This folder is inside the folder of \u2068a\u202Efdp.exe\u2069.",
  );
  expect(createErrorLine("hi", "matter.folder_holds_matter", "", "Acme")).toBe(
    MATTERS_HI["This folder holds the folder of {name}."].replace("{name}", iso("Acme")),
  );
  // no name (not on the list): the line of a folder refused, naming the folder where it can
  for (const code of [
    "matter.folder_inside_matter",
    "matter.folder_holds_matter",
    "matter.folder_is_matter",
  ]) {
    expect(createErrorLine("en", code, "Z:\\Acme")).toBe(
      `Clauseo can't make a matter of ${iso("Z:\\Acme")}. Choose another folder.`,
    );
    expect(createErrorLine("en", code, "", "")).toBe(CANT);
    expect(nestedRefusal(code)).toBe(true);
  }
  expect(createErrorLine("en", "matter.folder_is_matter", "", "Acme")).toBe(CANT);
  expect(nestedRefusal("matter.folder_being_made")).toBe(false);
});

test("a folder in Clauseo's matters folder is a choose-another refusal, with its path when the form has it", () => {
  expect(createErrorLine("en", "matter.folder_in_matters_root", "Documents/Clauseo/Matters")).toBe(
    "\u2068~/Documents/Clauseo/Matters\u2069 is the folder Clauseo keeps matters in. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_in_matters_root")).toBe(
    "That folder is where Clauseo keeps matters. Choose the folder for one matter.",
  );
  expect(createErrorLine("en", "matter.folder_in_matters_root", "")).toBe(
    "That folder is where Clauseo keeps matters. Choose the folder for one matter.",
  );
});

test("Locate folder's refusals name the folder picked, each by its reason", () => {
  const at = iso("~/Desktop/Khan");
  const line = (code: string, name?: string, same = false) =>
    locateErrorLine("en", code, "Desktop/Khan", name, same);
  expect(line("matter.folder_is_matter", "Khan", true)).toBe(
    `${at} is already this matter's folder.`,
  );
  expect(line("matter.folder_is_matter", "Acme")).toBe(
    `${iso("Acme")} already uses ${at}. Choose this matter's own folder.`,
  );
  expect(line("matter.folder_not_a_matter")).toBe(
    `${at} isn't this matter's folder. Choose the folder you moved it to.`,
  );
  expect(line("matter.folder_missing")).toBe(`Clauseo can't find ${at}. Choose the folder again.`);
  expect(line("matter.folder_inside_matter", "Acme")).toBe(
    `${at} is inside the folder of ${iso("Acme")}. Choose another folder.`,
  );
  expect(line("matter.folder_unreadable")).toBe(
    `Clauseo can't read ${at}. It may be locked, or on a drive that's no longer connected.`,
  );
  // no matter the list has, folder_refused, and any other code: Locate's own generic line
  const generic = `Clauseo can't use ${at} for this matter. Choose another folder.`;
  for (const code of [
    "matter.folder_is_matter",
    "matter.folder_holds_matter",
    "matter.folder_refused",
    "internal",
  ])
    expect(line(code)).toBe(generic);
  expect(
    locateErrorLine("hi", "matter.folder_not_a_matter", "Desktop/Khan", undefined, false),
  ).toBe(
    MATTERS_HI["{path} isn't this matter's folder. Choose the folder you moved it to."].replace(
      "{path}",
      at,
    ),
  );
});
