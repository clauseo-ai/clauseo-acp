// The new matter form's check, pure: the line the form shows when the host refuses a folder or a matter can't be
// made. Never the host's own message.
import type { Lang } from "../lib/display.ts";
import { isolate } from "../lib/bidi.ts";
import { homeShown } from "./folder-path.ts";
import { mt } from "../lib/words-matters.ts";
import { ERROR_WORDS, errorWords } from "../lib/words-errors.ts";

/** The form's line for a matter the host wouldn't make, from the error's code and, where the caller has it, the
 * folder's display ("Documents/Acme", "C:\\"), home written as "~" and isolated in the line, and for matter.folder_is_matter the name of the
 * matter that has the folder, when the matters list has it. matter.folder_refused, which doesn't say why, takes a line
 * true of every reason the host gives it: a folder already a matter, an unreadable matter file, the matters' own
 * folder, a drive, the home folder, a system folder. H5's codes each say their reason; any other matter.folder_ code
 * words-errors has a line for gives that line, in the language; every other code gives the generic line. */
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
  switch (code) {
    case "matter.folder_refused":
      return mt(lang, "Clauseo can't make a matter of this folder. Choose another folder.");
    case "matter.folder_is_matter":
      return matterName === undefined || matterName === ""
        ? mt(lang, "This folder is already a matter.")
        : mt(lang, "This folder is already a matter: {name}.", { name: isolate(matterName) });
    case "matter.folder_in_matters_root":
      return both(
        named
          ? mt(lang, "{path} is the folder Clauseo keeps matters in.", { path })
          : mt(lang, "That folder is where Clauseo keeps matters."),
        mt(lang, "Choose the folder for one matter."),
      );
    case "matter.folder_is_drive":
      return both(
        named ? mt(lang, "{path} is a whole drive.", { path }) : null,
        mt(lang, "A matter is one folder of papers. Choose the folder for one matter."),
      );
    case "matter.folder_is_home":
      return both(
        named ? mt(lang, "{path} is your home folder.", { path }) : null,
        mt(lang, "It holds far more than one matter. Choose the folder for one matter."),
      );
    case "matter.folder_is_system":
      return both(
        named ? mt(lang, "{path} is a folder the computer keeps for itself.", { path }) : null,
        mt(lang, "Choose the folder that holds the matter's papers."),
      );
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
