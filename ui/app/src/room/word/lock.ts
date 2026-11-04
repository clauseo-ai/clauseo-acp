// Which files Word holds, from a refused act. The host's matter.file_locked names the files a program holds (its
// `files`, since #376), so those are the ones the room names; the act's own files (what it would write) are only the
// guess for an older host that doesn't name them. The room says Word holds the files only when each is a Word document:
// an owner file (`~$name`) marks Excel's and others' files too, so any other lock gets the error map's line.

/** A Word document: a path ending .doc or .docx, in any case. */
export const isWordFile = (path: string): boolean => /\.docx?$/i.test(path);

/** The matter-relative paths a refused act's lock is on: the files the host names for matter.file_locked, when it names
 * some; else `known`, files the host already named as locked (a restore's preview). A host that names none (older than
 * #376) leaves only a guess: the act's one file when it writes exactly one, and none when it writes several, since any
 * of them may be the one held. */
export function lockedFiles(
  error: { code: string; files?: readonly string[] | undefined },
  writes: readonly string[],
  known: readonly string[] = [],
): readonly string[] {
  const named = error.files;
  if (error.code !== "matter.file_locked") return writes;
  if (named !== undefined && named.length > 0) return named;
  if (known.length > 0) return known;
  return writes.length === 1 ? writes : [];
}

/** The matter-relative paths of the Word documents Word holds: the refusal is matter.file_locked, and `files` (from
 * lockedFiles) is non-empty and every one a Word document. Otherwise null, and the error map's line says why. */
export function wordLocks(code: string, files: readonly string[]): readonly string[] | null {
  if (code !== "matter.file_locked" || files.length === 0) return null;
  return files.every(isWordFile) ? files : null;
}
