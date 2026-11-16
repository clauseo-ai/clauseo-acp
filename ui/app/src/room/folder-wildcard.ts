// A folder Claude's protection can't name: on macOS and Linux, Claude Code reads each folder its sandbox allows or denies
// as a pattern, with no way to escape a wildcard character, so the host refuses a Claude launch whose matter folder's
// path has one in a folder's name (matter.folder_wildcard). The thread's line names that folder, read from the matter's
// `folder_display`, which is below the home folder: a wildcard in the home folder's own path never shows here, and the
// line then says the general one (agent.sandbox_unavailable's). The host never sends the code on Windows.

/** The characters Claude's sandbox reads as a wildcard in a folder's name. Mirrors the host's `WILDCARDS` in
 * crates/host/src/agent/loads/sandbox.rs: a character added there is added here, in one line on each side. */
export const WILDCARDS: readonly string[] = ["*", "?"];

/** The name of the deepest folder in `path`, as the host shows it ("/" between names), whose name has a wildcard
 * character (WILDCARDS): the matter folder's own name, or a folder above it; null with none. */
export function wildcardAt(path: string): string | null {
  const names = path.split("/").filter((n) => n !== "");
  for (let i = names.length - 1; i >= 0; i--) {
    const name = names[i] ?? "";
    if (WILDCARDS.some((c) => name.includes(c))) return name;
  }
  return null;
}

/** matter.folder_wildcard's folder (errorWords' `place.wildcard`) for a matter whose folder shows as `folder`: null for
 * a folder not known, or one whose display shows no wildcard, and the line then names none. */
export const wildcardName = (folder: string | null): string | null =>
  folder === null ? null : wildcardAt(folder);
