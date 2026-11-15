// A folder Claude can't start in: on macOS and Linux, a path with a backslash (`\`) in a folder's name, where Claude
// Code exits with "Can't access working directory" (the host refuses it as matter.folder_backslash). On Windows `\` is the
// separator, so a path there always has one and says nothing. New matter's warning (folder-check.tsx), Locate's step
// (locate-warning.tsx) and the first screen's Open the folder read the picked folder's display (picked.rs), whose folder
// names are the folder's own; the thread's refusal reads the matter's `folder_display`. Both are below the home folder,
// so a backslash in the home folder's own path never shows here.
import type { Platform } from "@clauseo/components/lib/display.ts";

/** Which folder's name has the backslash: the folder's own (the path's last name), or the nearest folder above it
 * that has one, by its name. */
export type BackslashAt = { self: true } | { self: false; name: string };

/** Where `path`, as the host shows it, has a backslash Claude can't work with on `platform`: null with none (and always
 * on Windows), else the deepest folder whose name has one. */
export function backslashAt(path: string, platform: Platform): BackslashAt | null {
  if (platform === "windows") return null;
  const names = path.split("/").filter((n) => n !== "");
  for (let i = names.length - 1; i >= 0; i--) {
    const name = names[i] ?? "";
    if (!name.includes("\\")) continue;
    return i === names.length - 1 ? { self: true } : { self: false, name };
  }
  return null;
}

/** Whether `path`, as the picker showed it, has a backslash Claude can't work with on `platform`. */
export const hasBackslash = (path: string, platform: Platform): boolean =>
  backslashAt(path, platform) !== null;

/** The refusal's place (words-errors-backslash.ts) for a matter whose folder shows as `folder`: the folder above it
 * named when its name has the backslash, else the matter folder's own name, also for a folder not known. */
export const backslashPlace = (folder: string | null, platform: Platform) => {
  const at = folder === null ? null : backslashAt(folder, platform);
  return { platform, parent: at !== null && !at.self ? at.name : null };
};
