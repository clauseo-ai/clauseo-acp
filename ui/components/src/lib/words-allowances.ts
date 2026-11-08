// R18's matter-level words: the remembered answers (Settings' section and the room's protection
// note), switching the Safe workspace off and on from the matter, and "Use files from another
// matter". As in words.ts the English is the key, the exact string on screen. Both windows import
// this file: Settings and the room.
import type { Lang } from "./display.ts";
import { isolate } from "./isolate.ts";
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-allowances.hi.ts";

/** Every string of R18's matter-level parts, in English. */
export type AllowancesKey = keyof typeof HI;

/** An R18 matter-level string in the language, its slots filled. */
export const aw = wordsOf("allowances");

/** What a remembered answer covers, as its row shows it: the folder, file or site (isolated by the
 * caller), and the kind's words. A target from a newer Clauseo is a bare string. */
export type AllowTargetLike =
  | { folder: { path: string } }
  | { file: { path: string } }
  | { other_matter_path: { path: string; is_file?: boolean } }
  | { site: { host: string } }
  | string;

/** The folder, file or host a remembered answer names; null for a target from a newer Clauseo. */
export function targetText(target: AllowTargetLike): string | null {
  if (typeof target === "string") return null;
  if ("folder" in target) return target.folder.path;
  if ("file" in target) return target.file.path;
  if ("other_matter_path" in target) return target.other_matter_path.path;
  if ("site" in target) return target.site.host;
  return null;
}

/** The kind's words: a folder or file allowed is read only; a site is a site. */
export function targetKind(lang: Lang, target: AllowTargetLike, allowed: boolean): string {
  if (typeof target === "string") return aw(lang, "From a newer version of Clauseo");
  if ("site" in target) return aw(lang, "Site");
  if ("folder" in target) return aw(lang, allowed ? "Folder, read only" : "Folder");
  if ("file" in target) return aw(lang, allowed ? "File, read only" : "File");
  return aw(lang, allowed ? "From another matter, read only" : "From another matter");
}

/** Forget's accessible name: the target, sanitised and isolated (isolate.ts); the kind for a target with none. */
export const forgetLabel = (lang: Lang, target: AllowTargetLike, allowed: boolean): string =>
  aw(lang, "Forget {target}", {
    target: isolate(targetText(target) ?? targetKind(lang, target, allowed)),
  });
