// Each row of the tool line's popover, in the fewest words (DESIGN.md §Icons): the tool's glyph and name, an On or
// Off chip, the reason in a few words only when off, and under the browser with no Chrome, the download. Pure, so the
// tests can read every state; loaded with the popover only.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import type { ToolGlyphState } from "@clauseo/components/icons/tool-glyph.tsx";
import type { BrowserInstall, InstallFailCode } from "./browser-install.ts";
import { glyphState, reasonOf, toolName, type KnownTool } from "./tools.ts";

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
  /** [break] the cause in a few words, and Try again unless this computer can't download it. */
  | { kind: "failed"; cause: string; retry: boolean };

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
  /** Off in Settings, with the settings window to turn it on: the popover's foot offers Open Settings. */
  settings: boolean;
  download: DownloadPart | null;
};

export type ToolRowRoom = {
  /** Whether the host has the settings window, for Open Settings. */
  settings: boolean;
  /** The app's Chrome download; null when the host reports none. */
  install: BrowserInstall | null;
};

const mb = (bytes: number) => Math.round(bytes / 1e6);

/** What a failed download says, by the host's code; never the host's own message. */
export const FAILED: Record<InstallFailCode, ToolsKey> = {
  network: "Connection dropped",
  size_mismatch: "File didn't match",
  signature: "File didn't match",
  disk_space: "Needs 1\u00a0GB free",
  host_refused: "Network blocked it",
  unpack: "Couldn't unpack",
  unsupported: "Install Chrome to use it",
  cancelled: "Download stopped",
  unknown: "Download failed",
};

/** The download under the browser row with no Chrome, by its state; null without a state from the host. */
export function downloadPart(lang: Lang, install: BrowserInstall | null): DownloadPart | null {
  if (install === null) return null;
  const p = install.progress;
  const name = tw(lang, "Download Chrome");
  if (p === "unpacking") return { kind: "unpacking", text: tw(lang, "Unpacking") };
  if (p === "done") return { kind: "done" };
  if (p !== null && "downloading" in p) {
    const { bytes, total } = p.downloading;
    return {
      kind: "progress",
      value: Math.min(bytes, total),
      max: Math.max(total, 1),
      text: tw(lang, "{done} of {total}\u00a0MB", { done: mb(bytes), total: mb(total) }),
      name,
    };
  }
  if (p !== null) {
    const code = p.failed.code;
    return { kind: "failed", cause: tw(lang, FAILED[code]), retry: code !== "unsupported" };
  }
  if (install.offer?.offer === "download")
    return {
      kind: "offer",
      button: name,
      size: tw(lang, "200\u00a0MB"),
      tip: tw(lang, "From Google, once. Needs 1\u00a0GB free."),
    };
  if (install.offer?.offer === "none")
    return { kind: "none", line: tw(lang, "Install Chrome to use it") };
  return null;
}

/** One row, for a tool the line speaks for. */
export function toolRow(lang: Lang, tool: KnownTool, room: ToolRowRoom): ToolRowWords {
  const on = tool.state === "available";
  const reason = reasonOf(tool);
  const download =
    tool.tool === "browser" && tool.state === "browser_not_installed"
      ? downloadPart(lang, room.install)
      : null;
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
    settings: tool.state === "off" && room.settings,
    download,
  };
}
