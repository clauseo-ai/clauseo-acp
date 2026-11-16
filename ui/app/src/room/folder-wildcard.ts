// A folder Claude's protection can't name: on macOS and Linux, Claude Code reads each folder its sandbox allows or denies
// as a pattern, with no way to escape a wildcard character, so the host refuses a Claude launch whose matter folder's
// path has one in a folder's name (matter.folder_wildcard). The thread's line names that folder, read from the matter's
// `folder_display`, which is below the home folder: a wildcard in the home folder's own path never shows here, and the
// line then says the general one (agent.sandbox_unavailable's). The host never sends the code on Windows.
//
// Another matter's folder with a wildcard stays in Claude's sandbox as the pattern it is, and the host refuses the
// launch (agent.sandbox_wildcard) only when that pattern would also deny this matter's folder: that line names the
// other matter's folder, found here the same way (coveringWildcard).

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

/** Claude Code 2.1.293's deny regex for a sandbox path (its bundled sandbox-runtime's globToRegex, W4t, with z4t's
 * `(/.*)?` tail), each `[` escaped as `[[]` first as the host writes it. Mirrors the host's `deny_covers` in
 * crates/host/src/agent/loads/sandbox/glob.rs; null for one that doesn't compile. */
function denyRegex(pattern: string): RegExp | null {
  const glob = pattern
    .replaceAll("[", "[[]")
    .replace(/[.^$+{}()|\\]/g, "\\$&")
    .replace(/\[([^\]]*?)$/g, "\\[$1")
    .replace(/\*\*\//g, "__GLOBSTAR_SLASH__")
    .replace(/\*\*/g, "__GLOBSTAR__")
    .replace(/\*/g, "[^/]*")
    .replace(/\?/g, "[^/]")
    .replace(/__GLOBSTAR_SLASH__/g, "(.*/)?")
    .replace(/__GLOBSTAR__/g, ".*");
  try {
    return new RegExp(`^${glob}(/.*)?$`);
  } catch {
    return null;
  }
}

/** Whether Claude's sandbox, denying `pattern` (a folder whose name has a * or ?), also denies `path`. */
export const denyCovers = (pattern: string, path: string): boolean =>
  denyRegex(pattern)?.test(path) ?? false;

/** A folder as the host shows it (folder_display: below the home folder, the parts below it; else in full), in one
 * form for denyCovers: below the home folder under "~/". */
const shown = (folder: string): string => (folder.startsWith("/") ? folder : `~/${folder}`);

/** agent.sandbox_wildcard's folder (errorWords' `place.covering`): of the other matters' folders (`others`, as
 * shown), the deepest folder with a * or ? (wildcardAt) of the first one whose deny would cover this matter's folder
 * (`own`, as shown); null with none, and the line then names none. */
export function coveringWildcard(
  own: string | null,
  others: readonly (string | undefined)[],
): string | null {
  if (own === null || own === "") return null;
  for (const other of others) {
    if (other === undefined || other === "" || other === own) continue;
    const name = wildcardAt(other);
    if (name !== null && denyCovers(shown(other), shown(own))) return name;
  }
  return null;
}

/** coveringWildcard for the room's open matter (`open`) among its matters (`matters`, the open one left out). */
export const coveringOf = (
  open: { matter: string; folder_display?: string } | null,
  matters: readonly { matter: string; folder_display?: string }[],
): string | null =>
  coveringWildcard(
    open?.folder_display ?? null,
    matters.filter((m) => m.matter !== open?.matter).map((m) => m.folder_display),
  );
