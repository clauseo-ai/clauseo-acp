// What the wall shows of the files Clauseo doesn't read, pure, for paper-wall.tsx, the new conversation's line
// (new-conversation-line.tsx) and their tests: a dimmed tile for each file list_papers left out because it is only
// online (skipped, reason cloud_only; a file over the size cap stays out) and for each folder it couldn't list
// (unread), placed among the papers by path as the host sorts them, a folder where "<folder>/" sorts. Each says why on
// its tile, and in its tip how to keep it on this computer. The new conversation's count adds how many more are only
// online; folders never count, as what they hold isn't known. A listing that stops short counts what it lists.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { count } from "@clauseo/components/lib/money.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { rt } from "@clauseo/components/lib/words-room.ts";
import type { FolderPlace, PaperSummary } from "../transport/protocol.ts";
import type { Papers } from "./host-next.ts";
import { serviceName } from "./sync-services.ts";

/** One tile on the wall: a paper, or a file or folder Clauseo doesn't read, with a DOM-safe id of its own (by its
 * place among those, never its path). */
export type WallItem =
  | { kind: "paper"; paper: PaperSummary }
  | { kind: "away"; id: string; path: string; folder: boolean };

const bytes = new TextEncoder();

/** The host's order: by the path's UTF-8 bytes. */
function byBytes(a: Uint8Array, b: Uint8Array): number {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) if (a[i] !== b[i]) return a[i]! - b[i]!;
  return a.length - b.length;
}

/** The files only online in a listing, by path. */
const cloudOnly = (papers: Papers) =>
  papers.skipped.filter((s) => s.reason === "cloud_only").map((s) => s.path);

/** How many listed files are only online. */
export const cloudOnlyCount = (papers: Papers): number => cloudOnly(papers).length;

/** Whether the wall has no tile at all: no paper, no file only online, no folder it couldn't open. */
export const wallEmpty = (papers: Papers): boolean =>
  papers.papers.length === 0 && papers.unread.length === 0 && cloudOnlyCount(papers) === 0;

/** The wall's tiles in the host's order, the papers among the files and folders it doesn't read: the host sorts each
 * list by path, so a merge places each away tile where it sorts and keeps each list's own order as it came. */
export function wallItems(papers: Papers): WallItem[] {
  type Keyed = { key: Uint8Array; paper: PaperSummary | null; path: string; folder: boolean };
  const listed: Keyed[] = papers.papers.map((paper) => ({
    key: bytes.encode(paper.path),
    paper,
    path: paper.path,
    folder: false,
  }));
  const files: Keyed[] = cloudOnly(papers).map((path) => ({
    key: bytes.encode(path),
    paper: null,
    path,
    folder: false,
  }));
  const folders: Keyed[] = papers.unread.map((path) => ({
    key: bytes.encode(`${path}/`),
    paper: null,
    path,
    folder: true,
  }));
  const merged = merge(merge(listed, files), folders);
  let away = 0;
  return merged.map(({ paper, path, folder }): WallItem =>
    paper !== null
      ? { kind: "paper", paper }
      : { kind: "away", id: `away-${away++}`, path, folder },
  );
}

/** Two lists merged by key, each in its own order; on a tie the first list's comes first. */
function merge<T extends { key: Uint8Array }>(a: readonly T[], b: readonly T[]): T[] {
  const out: T[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length)
    out.push(byBytes(b[j]!.key, a[i]!.key) < 0 ? b[j++]! : a[i++]!);
  return [...out, ...a.slice(i), ...b.slice(j)];
}

/** A dimmed tile's words: the line on its face, and its tip, which assistive technology reads as its description. */
export function awayWords(
  lang: Lang,
  platform: Platform,
  place: FolderPlace,
  folder: boolean,
): { line: string; tip: string } {
  if (folder)
    return {
      line: rt(lang, "Not opened"),
      tip: rt(
        lang,
        "Clauseo couldn't look inside this folder. It may be only online, or Clauseo may not be allowed to open it.",
      ),
    };
  const service = serviceName(place);
  const oneDrive = typeof place === "object" && place.synced?.service === "one_drive";
  const how =
    oneDrive && platform === "windows"
      ? rt(
          lang,
          "To keep it here, right-click it in File Explorer and choose Always keep on this device.",
        )
      : oneDrive && platform === "mac"
        ? rt(
            lang,
            "To keep it here, Control-click it in Finder and choose Always Keep on This Device.",
          )
        : rt(lang, "Download it to this computer to change that.");
  return {
    line: service === null ? rt(lang, "Only online") : rt(lang, "Only in {service}", { service }),
    tip: `${rt(lang, "Not on this computer, so Clauseo doesn't open it or save new versions of it.")} ${how}`,
  };
}

/** What the new conversation's count adds: how many more papers are only online; null with none. */
export function moreWords(lang: Lang, place: FolderPlace, n: number): string | null {
  if (n <= 0) return null;
  const service = serviceName(place);
  if (service !== null)
    return n === 1
      ? mt(lang, "1 more is only in {service}.", { service })
      : mt(lang, "{n} more are only in {service}.", { n: count(n), service });
  return n === 1
    ? mt(lang, "1 more is only online.")
    : mt(lang, "{n} more are only online.", { n: count(n) });
}
