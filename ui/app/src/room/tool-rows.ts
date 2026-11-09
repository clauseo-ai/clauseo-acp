// Each row of the tool line's popover, in the fewest words (DESIGN.md §Icons): the tool's glyph and name, an On or
// Off chip, the reason in a few words only when off; under the browser with no Chrome (or Edge, on Windows), what to
// install and that Clauseo then needs reopening; under the browser the firm's policy blocks, or whose policy Clauseo
// couldn't check, the reason in full, and for the policy not checked, Try again (browser-policy.ts). Pure, so the
// tests can read every state; loaded with the popover only.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import type { ToolGlyphState } from "@clauseo/components/icons/tool-glyph.tsx";
import type { BrowserPolicy } from "./browser-policy.ts";
import { glyphState, reasonOf, toolName, type BrowserState, type KnownTool } from "./tools.ts";

/** Try again under the browser whose policy Clauseo couldn't check: its words, and whether the read it asked for runs. */
export type RecheckPart = { button: string; waiting: boolean };

export type ToolRowWords = {
  tool: KnownTool["tool"];
  glyph: ToolGlyphState;
  name: string;
  on: boolean;
  /** The chip's word: On or Off. */
  chip: string;
  /** Why it's off, a few words; null while on. Once a policy read found nothing blocking, what it is ready for. */
  reason: string | null;
  /** The reason sits in the chip's row, after it (In Settings), not on a line of its own. */
  beside: boolean;
  /** The reason's tooltip and description: once a policy read found nothing blocking, when the browser comes; else
   * null. */
  reasonTip: string | null;
  /** A muted line under the reason saying it in full: what to install and that Clauseo then needs reopening, the
   * firm's policy blocks the browser, or Clauseo couldn't check it; else null. */
  note: string | null;
  /** Off in Settings, with the settings window to turn it on: the popover's foot offers Open Settings. */
  settings: boolean;
  /** Under the browser whose policy Clauseo couldn't check, Try again; else null. */
  recheck: RecheckPart | null;
};

export type ToolRowRoom = {
  /** Whether the host has the settings window, for Open Settings. */
  settings: boolean;
  /** The computer's OS: what to install for the browser (Chrome, or Chrome or Edge on Windows). */
  platform: Platform;
  /** The re-read of the firm's browser policy; null when the window has no app feed to hear it on. */
  policy?: BrowserPolicy | null;
};

const POLICY: ToolsKey =
  "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.";
const UNREADABLE: ToolsKey =
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.";

/** What to install, in full: the browser's skill loads as Clauseo starts, so a browser installed later needs Clauseo
 * reopened. Windows takes Edge too. */
const INSTALL_MAC: ToolsKey = "Install Google Chrome, then close and reopen Clauseo.";
const INSTALL_WINDOWS: ToolsKey =
  "Install Google Chrome or Microsoft Edge, then close and reopen Clauseo.";

/** The browser's reason in full: what to install with none on the computer, or its policy keeping it off; else null. */
function browserNote(lang: Lang, platform: Platform, state: BrowserState): string | null {
  if (state === "browser_not_installed")
    return tw(lang, platform === "windows" ? INSTALL_WINDOWS : INSTALL_MAC);
  if (state === "browser_blocked_by_policy") return tw(lang, POLICY);
  if (state === "browser_policy_unreadable") return tw(lang, UNREADABLE);
  return null;
}

/** Try again under the browser whose policy Clauseo couldn't check, by the last read's state: waiting while one runs;
 * null once a read found it clear (the row then says the browser is ready), or with no app feed
 * to hear the read on. */
function recheckPart(lang: Lang, policy: BrowserPolicy | null | undefined): RecheckPart | null {
  if (policy == null || policy.state === "clear") return null;
  return { button: tw(lang, "Try again"), waiting: policy.state === "checking" };
}

/** One row, for a tool the line speaks for. */
export function toolRow(lang: Lang, tool: KnownTool, room: ToolRowRoom): ToolRowWords {
  const on = tool.state === "available";
  const reason = reasonOf(tool, room.platform);
  const unread = tool.tool === "browser" && tool.state === "browser_policy_unreadable";
  // a read since found nothing blocking: the next conversation gets the browser
  const ready = unread && room.policy?.state === "clear";
  return {
    tool: tool.tool,
    glyph: glyphState(tool),
    name: toolsLine(lang, toolName(tool)),
    on,
    chip: tw(lang, on ? "On" : "Off"),
    // once nothing blocks it, the reason says what it is ready for
    reason:
      reason === null
        ? null
        : ready
          ? tw(lang, "Ready for new conversations")
          : toolsLine(lang, reason),
    beside: tool.state === "off",
    reasonTip: ready ? tw(lang, "The browser comes with your next conversation.") : null,
    note: tool.tool === "browser" && !ready ? browserNote(lang, room.platform, tool.state) : null,
    settings: tool.state === "off" && room.settings,
    recheck: unread ? recheckPart(lang, room.policy) : null,
  };
}
