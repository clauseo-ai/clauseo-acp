// A folder's path as the new matter form shows it, pure. The desktop gives a folder under the home folder as the part
// below home ("Documents/Acme", "Documents\Acme"), and any other folder in full ("/Volumes/Work/Acme", "D:\Cases"). A
// folder under home reads as its folders in order, "Documents › Acme", on a Mac and on Windows alike: no "~", which a
// Windows lawyer never sees, and no separator of either system. A folder elsewhere keeps the path its system writes.
// The form splits the path into its parent and its last part, so the row can cut the parent short and keep the
// folder's own name.

/** Whether `path` is written in full: from a root ("/", "\\server"), or a drive ("C:\", "D:"). */
const absolute = (path: string) => /^([\\/]|[A-Za-z]:)/.test(path);

/** The separator a path is written with: "\" where it has only those, else "/". */
export const separatorOf = (path: string) =>
  path.includes("\\") && !path.includes("/") ? "\\" : "/";

/** Between a folder under home and the one inside it, as the form shows them. */
export const STEP = " › ";

/** The path as the form shows it: "Documents/Acme" and "Documents\Acme" read "Documents › Acme"; a full path stays as
 * it is; "" stays "". */
export function homeShown(display: string): string {
  if (display === "" || absolute(display) || display.includes(STEP)) return display;
  return display
    .replace(/^~(?=[\\/]|$)/, "")
    .split(/[\\/]+/)
    .filter((part) => part !== "")
    .join(STEP);
}

/** `<location>/<name>`, in the way the location is written: a shown one with " › ", else its own separator. */
export function under(location: string, name: string): string {
  if (
    location.includes(STEP) ||
    (location !== "" && !absolute(location) && !/[\\/]/.test(location))
  )
    return `${location}${STEP}${name}`;
  return `${location.replace(/[\\/]+$/, "")}${separatorOf(location)}${name}`;
}

/** Controls (C0, DEL, C1), the line and paragraph separators and the bidi marks, which a matter's name may not hold. */
const UNFIT = /[\p{Cc}\u2028\u2029\u202A-\u202E\u2066-\u2069]/gu;
/** The most characters a matter's name may hold (the host's MAX_TITLE_CHARS). */
const MOST = 200;

/** A matter's name from its folder: the folder's own name, cleaned of what a name may not hold and cut to its length.
 * The folder keeps its name. */
export function folderName(display: string): string {
  // a drive's top, D:\ or D:, has no name of its own
  if (/^[A-Za-z]:[\\/]*$/.test(display)) return "";
  const name = splitPath(display).tail.replace(UNFIT, "").trim();
  return Array.from(name).slice(0, MOST).join("").trim();
}

/** The path cut before its last part: `head` up to and with the last separator, `tail` the folder's own name. A root
 * ("/", "C:\") is all head. */
export function splitPath(path: string): { head: string; tail: string } {
  const step = path.lastIndexOf(STEP);
  if (step >= 0)
    return { head: path.slice(0, step + STEP.length), tail: path.slice(step + STEP.length) };
  const trimmed = path.length > 1 ? path.replace(/[\\/]+$/, "") : path;
  const at = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (at < 0) return { head: "", tail: trimmed };
  if (at === trimmed.length - 1) return { head: trimmed, tail: "" };
  return { head: trimmed.slice(0, at + 1), tail: trimmed.slice(at + 1) };
}
