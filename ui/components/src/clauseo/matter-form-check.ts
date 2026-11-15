// The folder refusals' lines, pure: what New matter's form, the first screen and Locate folder say when the host
// refuses a folder or a matter can't be made. Never the host's own message. Each line names the folder as the lawyer
// picked it (folder refusals: the host gives the reason, the app has the folder).
import type { Lang } from "../lib/display.ts";
import { isolate } from "../lib/bidi.ts";
import { homeShown } from "./folder-path.ts";
import { mt } from "../lib/words-matters.ts";
import { ERROR_WORDS, errorWords } from "../lib/words-errors.ts";

/** The folder as a line names it: home as "~", as the form's Folder row writes it (folder-path.ts), and isolated, so a
 * name with right-to-left marks can't reorder the line; "" with none. */
const pathOf = (shown: string | undefined) =>
  shown === undefined || shown === "" ? "" : isolate(homeShown(shown));

/** A matter's name as a line names it, isolated; "" with none. */
const nameOf = (name: string | undefined) =>
  name === undefined || name === "" ? "" : isolate(name);

/** A folder Clauseo can't make a matter of, said by New matter where the host doesn't say why. */
const cant = (lang: Lang, path: string) =>
  path === ""
    ? mt(lang, "Clauseo can't make a matter of this folder. Choose another folder.")
    : mt(lang, "Clauseo can't make a matter of {path}. Choose another folder.", { path });

/** The line New matter and Locate folder both say for a refusal of the folder at `path` (isolated, "" when none is
 * known), naming `name`, the matter on the list a nested folder sits with (isolated, "" when the list hasn't it). Null
 * for a code with no shared line, a nested folder with no name, and a folder made or forgotten with no path: each
 * caller has its own line for those. Where no path is known (never from a pick, which always reads), the lines from
 * before folder refusals named the folder. */
function sharedLine(lang: Lang, code: string, path: string, name: string): string | null {
  const at = path !== "";
  const both = (first: string, second: string) => `${first} ${second}`;
  switch (code) {
    case "matter.folder_inside_matter":
      if (name === "") return null;
      return at
        ? mt(lang, "{path} is inside the folder of {name}. Choose another folder.", { path, name })
        : mt(lang, "This folder is inside the folder of {name}.", { name });
    case "matter.folder_holds_matter":
      if (name === "") return null;
      return at
        ? mt(lang, "{path} contains the folder of {name}. Choose another folder.", { path, name })
        : mt(lang, "This folder holds the folder of {name}.", { name });
    case "matter.folder_of_forgotten_matter":
      return at
        ? mt(
            lang,
            "{path} is inside, or contains, the folder of a matter you removed from the list. Choose another folder.",
            { path },
          )
        : null;
    case "matter.folder_being_made":
      return at
        ? mt(
            lang,
            "{path} is inside, or contains, a folder Clauseo is still making into a matter. Try again in a moment, or choose another folder.",
            { path },
          )
        : null;
    case "matter.folder_is_drive":
      return at
        ? mt(lang, "{path} is the top of a drive. Choose a folder on it.", { path })
        : mt(lang, "This is the top of a drive. Choose a folder on it.");
    case "matter.folder_is_home":
      return at
        ? mt(lang, "{path} is your home folder. Choose a folder inside it.", { path })
        : mt(lang, "This is your home folder. Choose a folder inside it.");
    case "matter.folder_is_system":
      return at
        ? mt(lang, "{path} is a system folder. Choose another folder.", { path })
        : mt(lang, "This is a system folder. Choose another folder.");
    case "matter.folder_in_clauseo_data":
      return at
        ? mt(lang, "Clauseo keeps its own files in {path}. Choose another folder.", { path })
        : mt(lang, "This folder is part of Clauseo's own data. Choose another folder.");
    case "matter.folder_too_deep":
      return at
        ? mt(
            lang,
            "{path} is too deeply nested for Windows. Choose a folder with a shorter path.",
            { path },
          )
        : mt(lang, "This folder is too deeply nested for Windows. Choose a shorter path.");
    case "matter.folder_in_matters_root":
      return both(
        at
          ? mt(lang, "{path} is the folder Clauseo keeps matters in.", { path })
          : mt(lang, "That folder is where Clauseo keeps matters."),
        mt(lang, "Choose the folder for one matter."),
      );
    case "matter.folder_unreadable":
      return both(
        at
          ? mt(lang, "Clauseo can't read {path}.", { path })
          : mt(lang, "Clauseo can't read that folder."),
        mt(lang, "It may be locked, or on a drive that's no longer connected."),
      );
    default:
      return null;
  }
}

/** The form's line for a matter the host wouldn't make, from the error's code and, where the caller has it, the
 * folder's display ("Documents/Acme", "C:\\") and the name of the matter on the list the refusal names (HostError's
 * `matter`). A folder another matter on the list already uses (matter.folder_is_matter) names that matter, the form
 * offering Open {name}; a folder nested with a matter on the list names it the same way. With no name, those, a folder
 * nested with one being made or forgotten with no path, and matter.folder_refused, which doesn't say why, take the
 * line of a folder Clauseo can't make a matter of. Any other matter.folder_ code words-errors has a line for gives that
 * line, in the language; every other code gives the generic line. */
export function createErrorLine(
  lang: Lang,
  code: string,
  shown?: string,
  matterName?: string,
): string {
  const path = pathOf(shown);
  const name = nameOf(matterName);
  if (code === "matter.folder_is_matter" && name !== "" && path !== "")
    return mt(lang, "{name} already uses {path}. Open {name}, or choose another folder.", {
      name,
      path,
    });
  const shared = sharedLine(lang, code, path, name);
  if (shared !== null) return shared;
  switch (code) {
    case "matter.folder_refused":
    case "matter.folder_is_matter":
    case "matter.folder_inside_matter":
    case "matter.folder_holds_matter":
    case "matter.folder_of_forgotten_matter":
    case "matter.folder_being_made":
      return cant(lang, path);
    default:
      // a folder refusal with its own line in words-errors says the host's reason; anything else gives the generic line
      return code.startsWith("matter.folder_") && Object.hasOwn(ERROR_WORDS, code)
        ? errorWords(lang, code)
        : mt(lang, "Clauseo couldn't create the matter. Try again, or choose another folder.");
  }
}

/** Locate folder's line for a folder the host wouldn't take for the matter being located: from the code, the folder
 * picked as it reads, the name of the matter on the list the refusal names (HostError's `matter`), and whether that
 * matter is the one being located (`same`). The folder already the matter's own, another matter's folder, one that
 * isn't the matter's, and one that's gone have Locate's own lines; the place and nesting refusals say New matter's;
 * anything else, and a refusal naming no matter the list has, the line of a folder Clauseo can't use for this matter. */
export function locateErrorLine(
  lang: Lang,
  code: string,
  shown: string,
  matterName: string | undefined,
  same: boolean,
): string {
  const path = pathOf(shown);
  const name = nameOf(matterName);
  if (code === "matter.folder_is_matter" && same)
    return mt(lang, "{path} is already this matter's folder.", { path });
  if (code === "matter.folder_is_matter" && name !== "")
    return mt(lang, "{name} already uses {path}. Choose this matter's own folder.", { name, path });
  if (code === "matter.folder_not_a_matter")
    return mt(lang, "{path} isn't this matter's folder. Choose the folder you moved it to.", {
      path,
    });
  if (code === "matter.folder_missing")
    return mt(lang, "Clauseo can't find {path}. Choose the folder again.", { path });
  return (
    sharedLine(lang, code, path, name) ??
    mt(lang, "Clauseo can't use {path} for this matter. Choose another folder.", { path })
  );
}

/** Whether a refusal names a matter on the list: a folder nested with its folder, or the folder it already uses. Its
 * line names that matter when the list has it, and Open {name} opens it. */
export const nestedRefusal = (code: string | undefined) =>
  code === "matter.folder_inside_matter" ||
  code === "matter.folder_holds_matter" ||
  code === "matter.folder_is_matter";
