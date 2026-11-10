// A folder's path as the new matter form shows it, pure. The desktop gives a folder under the home folder as the part
// below home ("Documents/Acme", "Documents\Acme"), and any other folder in full ("/Volumes/Work/Acme", "D:\Cases"); the
// form writes home as "~", the way a Mac or Linux lawyer reads it in Finder's Go to Folder or a terminal, and splits
// the path into its parent and its last part, so the row can cut the parent short and keep the folder's own name.

/** Whether `path` is written in full: from a root ("/", "\\server"), or a drive ("C:\", "D:"). */
const absolute = (path: string) => /^([\\/]|[A-Za-z]:)/.test(path);

/** The separator a path is written with: "\" where it has only those, else "/". */
export const separatorOf = (path: string) =>
  path.includes("\\") && !path.includes("/") ? "\\" : "/";

/** The path with home as "~": "Documents/Acme" reads "~/Documents/Acme"; a full path stays as it is; "" stays "". */
export function homeShown(display: string): string {
  if (display === "" || absolute(display) || display.startsWith("~")) return display;
  return `~${separatorOf(display)}${display}`;
}

/** `<location>/<name>`, with the separator the location is written in. */
export function under(location: string, name: string): string {
  return `${location.replace(/[\\/]+$/, "")}${separatorOf(location)}${name}`;
}

/** The path cut before its last part: `head` up to and with the last separator, `tail` the folder's own name. A root
 * ("/", "C:\") is all head. */
export function splitPath(path: string): { head: string; tail: string } {
  const trimmed = path.length > 1 ? path.replace(/[\\/]+$/, "") : path;
  const at = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (at < 0) return { head: "", tail: trimmed };
  if (at === trimmed.length - 1) return { head: trimmed, tail: "" };
  return { head: trimmed.slice(0, at + 1), tail: trimmed.slice(at + 1) };
}
