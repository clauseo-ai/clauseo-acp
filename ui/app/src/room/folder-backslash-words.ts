// The words of New matter's warning, Locate's step and the first screen's step for a folder Claude can't start in
// (FR24 to FR30; folder-backslash.ts): drafts, Rohan to approve. They name the folder whose name has the backslash
// when it is a folder above the one picked, and the computer's file manager: Finder on the Mac, "your file manager" on
// Linux (Windows never warns). Loaded with the warning and the steps, never on first paint.
import { isolate } from "@clauseo/components/lib/words-core.ts";
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { fr, type FolderKey } from "@clauseo/components/lib/words-folder.ts";
import type { BackslashAt } from "./folder-backslash.ts";

/** Each body, by the folder that has the backslash and the file manager. */
const BODY: Readonly<Record<"own" | "parent", Record<"finder" | "files", FolderKey>>> = {
  own: {
    finder:
      "Claude can't work in a folder like this; ChatGPT can. Rename it in Finder first, or choose another folder.",
    files:
      "Claude can't work in a folder like this; ChatGPT can. Rename it in your file manager first, or choose another folder.",
  },
  parent: {
    finder:
      "Claude can't work in a folder like this; ChatGPT can. Rename that folder in Finder first, or choose another folder.",
    files:
      "Claude can't work in a folder like this; ChatGPT can. Rename that folder in your file manager first, or choose another folder.",
  },
};

/** The warning's name (its tooltip and accessible name), title and body, in `lang`, for the folder `at` names. */
export function backslashWords(
  lang: Lang,
  at: BackslashAt,
  platform: Platform,
): { name: string; title: string; body: string } {
  const files = platform === "linux" ? "files" : "finder";
  if (at.self)
    return {
      name: fr(lang, "This folder's name has a backslash"),
      title: fr(lang, "This folder's name has a backslash (\\)."),
      body: fr(lang, BODY.own[files]),
    };
  return {
    name: fr(lang, "A folder in this folder's path has a backslash"),
    title: fr(lang, 'The folder "{folder}" in this folder\'s path has a backslash (\\).', {
      folder: isolate(at.name),
    }),
    body: fr(lang, BODY.parent[files]),
  };
}
