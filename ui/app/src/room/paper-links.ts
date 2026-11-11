// Which paper on the wall a file link in an answer names (the renderer's PaperLinksContext). Agents write a file they
// made or read as a link in many forms: an absolute path (`/Users/…/Smith/Reply%20to%20notice.docx`), a `file://` URL,
// ChatGPT's `sandbox:/mnt/data/…`, a matter-relative path or a bare name. Only the wall's papers ever match, never an
// arbitrary path, and a match is the paper's matter-relative path, which AnswerHost.openPath opens by its file ref as
// the wall does. Pure: the matter's folder comes in as the host shows it (`folder_display`, below the home folder the
// part after it, elsewhere the whole path), and is only ever matched as text.

import type { PaperLinks } from "@clauseo/components/markdown/host.ts";
import { useMemo } from "react";
import type { MatterId } from "../transport/protocol.ts";
import type { ListedPapers } from "./open.ts";

/** A scheme (`https:`, `sandbox:`), but not a Windows drive (`C:`). */
const SCHEME = /^([a-z][a-z0-9+.-]+):/i;
/** A Windows drive's root, `C:/`. */
const DRIVE = /^[a-z]:\//i;
/** A home folder an absolute path may start with, before `folder_display` (which is below it): exactly one name under
 * `/Users`, `/home` or `X:/Users`, or `~`. */
const HOME = /^(?:\/Users\/[^/]+|\/home\/[^/]+|[a-z]:\/Users\/[^/]+|~)\//i;

/** `text` percent-decoded, or as written when that is malformed (`%E0%A4`). */
function decoded(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/** `path` with `.` and empty parts dropped and `..` applied; null when a `..` climbs above where it starts. The root
 * (`/`, `//`, `C:/`, `~/`) stays as written. */
function normal(path: string): string | null {
  const root = /^(?:[a-z]:\/|~\/|\/\/|\/)/i.exec(path)?.[0] ?? "";
  const parts: string[] = [];
  for (const part of path.slice(root.length).split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.pop() === undefined) return null;
    } else parts.push(part);
  }
  return parts.length === 0 ? null : root + parts.join("/");
}

/** A link's target: what kind of link it was, and its path with `/` separators, `.` and `..` applied. */
type Target = { kind: "absolute" | "relative" | "sandbox"; path: string };

/** The path a file link names, percent-decoded, its query and fragment dropped; null for a link that names no file
 * (another scheme, a fragment, nothing at all). */
function targetOf(href: string): Target | null {
  const bare = (href.split(/[?#]/)[0] ?? "").trim();
  let kind: Target["kind"] = "relative";
  let rest = bare;
  const scheme = SCHEME.exec(bare)?.[1]?.toLowerCase();
  if (scheme === "file") {
    // file:///Users/…, file://localhost/Users/…, file:///C:/…
    rest = bare.slice("file:".length).replace(/^\/\/(?:localhost)?(?=\/)/i, "");
    if (!rest.startsWith("/")) return null;
    kind = "absolute";
  } else if (scheme === "sandbox") {
    rest = bare.slice("sandbox:".length);
    kind = "sandbox";
  } else if (scheme !== undefined) return null;
  let path = decoded(rest).replaceAll("\\", "/");
  // file:///C:/… names the drive's path
  if (/^\/[a-z]:\//i.test(path)) path = path.slice(1);
  if (kind === "relative" && (path.startsWith("/") || DRIVE.test(path) || path.startsWith("~/")))
    kind = "absolute";
  const clean = normal(path);
  return clean === null ? null : { kind, path: clean };
}

/** The last part of a path. */
const nameOf = (path: string): string => path.slice(path.lastIndexOf("/") + 1);

/** The matter-relative part of an absolute path inside the matter's folder, or null: the path is exactly
 * `<folder>/<rest>` for a folder shown whole, or `<home>/<folder>/<rest>` for one shown below the home folder. */
function insideFolder(path: string, folder: string): { rest: string; windows: boolean } | null {
  const shown = normal(folder.replaceAll("\\", "/"));
  if (shown === null) return null;
  const whole = shown.startsWith("/") || DRIVE.test(shown);
  let below = path;
  if (!whole) {
    const home = HOME.exec(path)?.[0];
    if (home === undefined) return null;
    below = path.slice(home.length);
  }
  // a Windows path compares as Windows does, whatever its case
  const windows = DRIVE.test(path);
  const fold = (s: string) => (windows ? s.toLowerCase() : s);
  const prefix = `${shown}/`;
  if (!fold(below).startsWith(fold(prefix))) return null;
  return { rest: below.slice(prefix.length), windows };
}

/** The one paper path among `paths` for which `same` holds, or undefined when none or more than one does. */
function only(paths: readonly string[], same: (path: string) => boolean): string | undefined {
  const found = paths.filter(same);
  return found.length === 1 ? found[0] : undefined;
}

/**
 * The matter-relative path of the paper on the wall a file link names, or undefined. `paths` are the wall's papers'
 * matter-relative paths, `folder` the matter's `folder_display` ("" or null when the host gives none). In order:
 * (a) an absolute path (or `file://` URL) exactly inside the matter's folder, at a paper's path; (b) a relative path
 * that is exactly a paper's path; (c) for a relative bare name or a `sandbox:` link, the one paper with that file name.
 * A path that climbs out of the matter, a name two papers share, a path outside the matter and a file the wall doesn't
 * show all give undefined.
 */
export function paperLinkOf(
  href: string,
  paths: readonly string[],
  folder: string | null,
): string | undefined {
  const target = targetOf(href);
  if (target === null) return undefined;
  const papers = paths.map((p) => p.replaceAll("\\", "/"));
  const back = (found: string | undefined) =>
    found === undefined ? undefined : paths[papers.indexOf(found)];
  const { kind, path } = target;
  if (kind !== "relative" && (path.startsWith("/") || DRIVE.test(path) || path.startsWith("~/"))) {
    const inside = folder ? insideFolder(path, folder) : null;
    if (inside !== null) {
      const { rest, windows } = inside;
      const found = windows
        ? only(papers, (p) => p.toLowerCase() === rest.toLowerCase())
        : papers.find((p) => p === rest);
      if (found !== undefined) return back(found);
    }
    // an absolute path elsewhere names nothing on the wall; ChatGPT's sandbox paths are its own, not the matter's
    if (kind === "absolute") return undefined;
  } else {
    const exact = papers.find((p) => p === path);
    if (exact !== undefined) return back(exact);
    if (kind === "relative" && path.includes("/")) return undefined;
  }
  const name = nameOf(path);
  return back(only(papers, (p) => nameOf(p) === name));
}

/** Which paper on the wall a file link in an answer names (paperLinkOf): the open matter's listed papers, in its
 * folder as the host shows it. A new list makes a new one, so a link to a paper that just arrived becomes a link. */
export function usePaperLinks(
  matter: MatterId | null,
  listed: ListedPapers,
  folder: string | null,
): PaperLinks {
  return useMemo(() => {
    const paths =
      matter !== null && listed?.matter === matter ? listed.papers.papers.map((p) => p.path) : [];
    return (href: string) => paperLinkOf(href, paths, folder);
  }, [matter, listed, folder]);
}
