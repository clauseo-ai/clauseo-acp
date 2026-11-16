import { expect, test } from "vitest";
import { HI } from "../lib/words-errors.hi.ts";
import { HI as MATTERS_HI } from "../lib/words-matters.hi.ts";
import { createErrorLine, nestedRefusal } from "./matter-form-check.ts";

const CANT = "Clauseo can't make a matter of this folder. Choose another folder.";

test("a refusal says why in the form's words, never the host's message", () => {
  // folder_refused, which doesn't say why: one line true of every reason behind it
  expect(createErrorLine("en", "matter.folder_refused", "C:\\")).toBe(
    "Clauseo can't make a matter of this folder. Choose another folder.",
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

test("each folder refusal says its reason (folder PR 2), in either language, path or none", () => {
  const LINES = {
    "matter.folder_is_drive": "This is the top of a drive. Choose a folder on it.",
    "matter.folder_is_home": "This is your home folder. Choose a folder inside it.",
    "matter.folder_is_system": "This is a system folder. Choose another folder.",
    "matter.folder_in_clauseo_data":
      "This folder is part of Clauseo's own data. Choose another folder.",
    "matter.folder_too_deep":
      "This folder is too deeply nested for Windows. Choose a shorter path.",
  } as const;
  for (const [code, line] of Object.entries(LINES)) {
    expect(createErrorLine("en", code, "C:\\Matters\\Acme")).toBe(line);
    expect(createErrorLine("en", code)).toBe(line);
    expect(createErrorLine("hi", code, "/System")).toBe(MATTERS_HI[line]);
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

test("a folder nested with a matter names it when the list has it; with no name, and folder_is_matter, the refused line", () => {
  // the matter's name is isolated in the line, as a path is
  expect(
    createErrorLine("en", "matter.folder_inside_matter", "Z:\\Acme", "Acme Ltd v. State"),
  ).toBe("This folder is inside the folder of \u2068Acme Ltd v. State\u2069.");
  expect(createErrorLine("en", "matter.folder_holds_matter", "Z:\\", "Acme")).toBe(
    "This folder holds the folder of \u2068Acme\u2069.",
  );
  // a name with right-to-left marks can't reorder the full stop
  expect(createErrorLine("en", "matter.folder_inside_matter", "", "a\u202Efdp.exe")).toBe(
    "This folder is inside the folder of \u2068a\u202Efdp.exe\u2069.",
  );
  // Hindi: the name isolated the same way
  expect(createErrorLine("hi", "matter.folder_inside_matter", "", "Acme")).toBe(
    MATTERS_HI["This folder is inside the folder of {name}."].replace("{name}", "\u2068Acme\u2069"),
  );
  expect(createErrorLine("hi", "matter.folder_holds_matter", "", "Acme")).toBe(
    MATTERS_HI["This folder holds the folder of {name}."].replace("{name}", "\u2068Acme\u2069"),
  );
  // no name (a matter being made, forgotten, or not on the list): the line of a folder refused
  for (const code of ["matter.folder_inside_matter", "matter.folder_holds_matter"]) {
    expect(createErrorLine("en", code, "Z:\\Acme")).toBe(CANT);
    expect(createErrorLine("en", code, "Z:\\Acme", "")).toBe(CANT);
  }
  expect(createErrorLine("hi", "matter.folder_holds_matter")).toBe(MATTERS_HI[CANT]);
  // folder_is_matter never names a matter, with or without one
  expect(createErrorLine("en", "matter.folder_is_matter", "Z:\\Acme", "Acme")).toBe(CANT);
  expect(createErrorLine("en", "matter.folder_is_matter")).toBe(CANT);
  expect(nestedRefusal("matter.folder_inside_matter")).toBe(true);
  expect(nestedRefusal("matter.folder_holds_matter")).toBe(true);
  expect(nestedRefusal("matter.folder_is_matter")).toBe(false);
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
