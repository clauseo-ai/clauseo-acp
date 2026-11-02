// Which file Word holds, from a refused act and the files it would write. The host's matter.file_locked doesn't name
// the file (H1 will), so the room can only say Word holds a file when the act writes one file, and that file is a Word
// document: an owner file (`~$name`) marks Excel's and others' files too, so any other lock gets the error map's line.

/** A Word document: a path ending .doc or .docx, in any case. */
export const isWordFile = (path: string): boolean => /\.docx?$/i.test(path);

/** The matter-relative path of the file Word holds: the refusal is matter.file_locked, and `files` (the files the act
 * writes, or the files the host names as locked) is exactly one Word document. Otherwise null. */
export function wordLock(code: string, files: readonly string[]): string | null {
  if (code !== "matter.file_locked" || files.length !== 1) return null;
  const only = files[0];
  return only !== undefined && isWordFile(only) ? only : null;
}
