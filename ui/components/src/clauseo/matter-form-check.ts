// The new matter form's check, pure: the line the form shows when the host refuses a folder or a matter can't be
// made. Never the host's own message.
import type { Lang } from "../lib/display.ts";
import { isolate } from "../lib/bidi.ts";
import { homeShown } from "./folder-path.ts";
import { mt } from "../lib/words-matters.ts";
import { ERROR_WORDS, errorWords } from "../lib/words-errors.ts";

/** The form's line for a matter the host wouldn't make, from the error's code and, where the caller has it, the
 * folder's display ("Documents/Acme", "C:\\"), home written as "~" and isolated in the line, and for
 * matter.folder_inside_matter and matter.folder_holds_matter the name of the matter on the list whose folder it is
 * nested with. Each folder refusal says its reason (folder PR 2). A folder nested with a matter the list doesn't have
 * (one being made, or forgotten), matter.folder_is_matter (now only a rare same-folder case) and
 * matter.folder_refused, which doesn't say why, take the line of a folder Clauseo can't make a matter of. Any other
 * matter.folder_ code words-errors has a line for gives that line, in the language; every other code gives the generic
 * line. */
export function createErrorLine(
  lang: Lang,
  code: string,
  shown?: string,
  matterName?: string,
): string {
  const both = (first: string | null, second: string) =>
    first === null ? second : `${first} ${second}`;
  const named = shown !== undefined && shown !== "";
  // home as "~", as the form's Folder row writes it (folder-path.ts)
  const path = named ? isolate(homeShown(shown)) : "";
  const cant = () => mt(lang, "Clauseo can't make a matter of this folder. Choose another folder.");
  const known = matterName !== undefined && matterName !== "";
  switch (code) {
    case "matter.folder_refused":
    case "matter.folder_is_matter":
      return cant();
    case "matter.folder_inside_matter":
      return known
        ? mt(lang, "This folder is inside the folder of {name}.", { name: isolate(matterName) })
        : cant();
    case "matter.folder_holds_matter":
      return known
        ? mt(lang, "This folder holds the folder of {name}.", { name: isolate(matterName) })
        : cant();
    case "matter.folder_in_matters_root":
      return both(
        named
          ? mt(lang, "{path} is the folder Clauseo keeps matters in.", { path })
          : mt(lang, "That folder is where Clauseo keeps matters."),
        mt(lang, "Choose the folder for one matter."),
      );
    case "matter.folder_in_clauseo_data":
      return mt(lang, "This folder is part of Clauseo's own data. Choose another folder.");
    case "matter.folder_is_drive":
      return mt(lang, "This is the top of a drive. Choose a folder on it.");
    case "matter.folder_is_home":
      return mt(lang, "This is your home folder. Choose a folder inside it.");
    case "matter.folder_is_system":
      return mt(lang, "This is a system folder. Choose another folder.");
    case "matter.folder_too_deep":
      return mt(lang, "This folder is too deeply nested for Windows. Choose a shorter path.");
    case "matter.folder_unreadable":
      return both(
        named
          ? mt(lang, "Clauseo can't read {path}.", { path })
          : mt(lang, "Clauseo can't read that folder."),
        mt(lang, "It may be locked, or on a drive that's no longer connected."),
      );
    default:
      // a folder refusal with its own line in words-errors says the host's reason; anything else gives the generic line
      return code.startsWith("matter.folder_") && Object.hasOwn(ERROR_WORDS, code)
        ? errorWords(lang, code)
        : mt(lang, "Clauseo couldn't create the matter. Try again, or choose another folder.");
  }
}

/** Whether a refusal says the folder is nested with a matter's: its line names that matter when the list has it, and
 * Open {name} opens it. */
export const nestedRefusal = (code: string | undefined) =>
  code === "matter.folder_inside_matter" || code === "matter.folder_holds_matter";
