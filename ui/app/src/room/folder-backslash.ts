// A folder Claude can't start in: on macOS and Linux, a path with a backslash (`\`) in a folder's name, where Claude
// Code exits with "Can't access working directory" (the host refuses it as matter.folder_backslash). On Windows `\` is the
// separator, so a path there always has one and says nothing. New matter's warning (folder-check.tsx) and Locate's step
// (locate-warning.tsx) read the picked folder's display (picked.rs), whose folder names are the folder's own.
import type { Platform } from "@clauseo/components/lib/display.ts";

/** Whether `path`, as the picker showed it, has a backslash Claude can't work with on `platform`. */
export const hasBackslash = (path: string, platform: Platform): boolean =>
  platform !== "windows" && path.includes("\\");
