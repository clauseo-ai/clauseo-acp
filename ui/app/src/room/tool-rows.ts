// Each row of the tool line's popover, in the fewest words (DESIGN.md §Icons): the tool's glyph and name, an On or
// Off chip, the reason in a few words only when off, and under the browser with no Chrome or an
// out-of-date one, the download; under the browser the firm's policy blocks, whose policy Clauseo
// couldn't check, whose Chrome version it couldn't check, or whose Chrome is too old and can't be
// updated yet, the reason in full; under a browser
// that is on with a notice, the notice. Pure, so the tests can read every state; loaded with the
// popover only.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import type { ToolGlyphState } from "@clauseo/components/icons/tool-glyph.tsx";
import type { BrowserInstall, InstallFailCode } from "./browser-install.ts";
import {
  glyphState,
  noticeOfTool,
  reasonOf,
  toolName,
  type BrowserState,
  type KnownTool,
} from "./tools.ts";

/** The Chrome download as the browser row shows it, its words resolved. */
export type DownloadPart =
  /** [download] Download Chrome, its size muted beside it, its tooltip and description `tip`. */
  | { kind: "offer"; button: string; size: string; tip: string }
  /** This computer can't download it: a muted line. */
  | { kind: "none"; line: string }
  /** [download] the bar, and how far in words, which say it for screen readers too. */
  | { kind: "progress"; value: number; max: number; text: string; name: string }
  /** [download] Unpacking, with no bar: DESIGN's bars are determinate only, and nothing spins. */
  | { kind: "unpacking"; text: string }
  /** Chrome is in: the row's reason says so (ToolRowWords.reason), so nothing more under it. */
  | { kind: "done" }
  /** [break] the cause in a few words, and Try again. */
  | { kind: "failed"; cause: string };

export type ToolRowWords = {
  tool: KnownTool["tool"];
  glyph: ToolGlyphState;
  name: string;
  on: boolean;
  /** The chip's word: On or Off. */
  chip: string;
  /** Why it's off, a few words; null while on. Once Chrome is in (download.kind "done"), what it is ready for. */
  reason: string | null;
  /** The reason sits in the chip's row, after it (In Settings), not on a line of its own. */
  beside: boolean;
  /** The reason's tooltip and description: once Chrome is in, when the browser comes; else null. */
  reasonTip: string | null;
  /** A muted line under the reason saying it in full: the firm's policy blocks the browser,
   * Clauseo couldn't check that policy, couldn't check Chrome's version, or can't update a Chrome
   * too old to use; on a browser that is
   * on, its notice (a newer Chrome may be out); else null. */
  note: string | null;
  /** Off in Settings, with the settings window to turn it on: the popover's foot offers Open Settings. */
  settings: boolean;
  download: DownloadPart | null;
};

export type ToolRowRoom = {
  /** Whether the host has the settings window, for Open Settings. */
  settings: boolean;
  /** The app's Chrome download; null when the window has no app feed to hear it on. */
  install: BrowserInstall | null;
};

const mb = (bytes: number) => Math.round(bytes / 1e6);

/** What a failed download says, by the host's code; never the host's own message. `unsupported` is no failure line
 * (the computer can't download it: a muted line), and `cancelled` is no failure at all (the offer again). */
export const FAILED: Record<Exclude<InstallFailCode, "unsupported" | "cancelled">, ToolsKey> = {
  network: "Connection dropped",
  size_mismatch: "File didn't match",
  signature: "File didn't match",
  hash_mismatch: "File didn't match",
  list_unavailable: "Can't download Chrome yet",
  disk_space: "Needs 1\u00a0GB free",
  host_refused: "Network blocked it",
  unpack: "Couldn't unpack",
  unknown: "Download failed",
};

/** The download under the browser row with no Chrome, by its state: the offer until a download starts or after a
 * cancel; null only without an app feed to hear the state on. */
export function downloadPart(lang: Lang, install: BrowserInstall | null): DownloadPart | null {
  if (install === null) return null;
  const p = install.progress;
  const name = tw(lang, "Download Chrome");
  if (p === null)
    return {
      kind: "offer",
      button: name,
      size: tw(lang, "200\u00a0MB"),
      tip: tw(lang, "From Google, once. Needs 1\u00a0GB free."),
    };
  if (p === "unpacking") return { kind: "unpacking", text: tw(lang, "Unpacking") };
  if (p === "done") return { kind: "done" };
  if ("downloading" in p) {
    const { bytes, total } = p.downloading;
    return {
      kind: "progress",
      value: Math.min(bytes, total),
      // the first Downloading brings the total: until then the bar has no words
      max: Math.max(total, 1),
      text:
        total > 0
          ? tw(lang, "{done} of {total}\u00a0MB", { done: mb(bytes), total: mb(total) })
          : "",
      name,
    };
  }
  if (p.failed.code === "unsupported")
    return { kind: "none", line: tw(lang, "Install Chrome to use it") };
  return { kind: "failed", cause: tw(lang, FAILED[p.failed.code]) };
}

const POLICY: ToolsKey =
  "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.";
const UNREADABLE: ToolsKey =
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.";
const NO_LIST: ToolsKey =
  "Clauseo couldn't confirm that its copy of Chrome is up to date, so it isn't using the browser for now. It keeps checking on its own.";
const LIST_EXPIRED: ToolsKey =
  "Clauseo's copy of Chrome is too old to use, and Clauseo can't update it right now. It keeps trying on its own.";
const LIST_OLD: ToolsKey =
  "Clauseo hasn't checked for a newer Chrome in a while. The browser still works.";

/** The browser's reason in full when no download can help (its policy keeps it off, Clauseo can't check its
 * Chrome's version, or its list of versions is too old to trust a Chrome by); else null. */
function fullNote(lang: Lang, state: BrowserState): string | null {
  if (state === "browser_blocked_by_policy") return tw(lang, POLICY);
  if (state === "browser_policy_unreadable") return tw(lang, UNREADABLE);
  if (state === "browser_list_unavailable") return tw(lang, NO_LIST);
  if (state === "browser_list_expired") return tw(lang, LIST_EXPIRED);
  return null;
}

/** The download offers Chrome to a browser with none, or with one too old to use. */
const needsDownload = (t: KnownTool) =>
  t.tool === "browser" &&
  (t.state === "browser_not_installed" || t.state === "browser_out_of_date");

/** One row, for a tool the line speaks for. */
export function toolRow(lang: Lang, tool: KnownTool, room: ToolRowRoom): ToolRowWords {
  const on = tool.state === "available";
  const reason = reasonOf(tool);
  const download = needsDownload(tool) ? downloadPart(lang, room.install) : null;
  return {
    tool: tool.tool,
    glyph: glyphState(tool),
    name: toolsLine(lang, toolName(tool)),
    on,
    chip: tw(lang, on ? "On" : "Off"),
    // once Chrome is in, needing it is no longer so: the reason says what it is ready for
    reason:
      reason === null
        ? null
        : download?.kind === "done"
          ? tw(lang, "Ready for new conversations")
          : toolsLine(lang, reason),
    beside: tool.state === "off",
    reasonTip:
      download?.kind === "done" ? tw(lang, "The browser comes with your next conversation.") : null,
    // a download can't help here: the firm's block (or a policy not read) holds for every browser, and with no
    // list of versions (or one too old) a download would fail the same way; a browser that is on says its notice, if any
    note:
      tool.tool !== "browser"
        ? null
        : noticeOfTool(tool) === "browser_list_old" && on
          ? tw(lang, LIST_OLD)
          : fullNote(lang, tool.state),
    settings: tool.state === "off" && room.settings,
    download,
  };
}
