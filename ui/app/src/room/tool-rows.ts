// Each row of the tool line's popover, in the fewest words (DESIGN.md §Icons): the tool's glyph and name, an On or
// Off chip, the reason in a few words only when off; under the browser with no Chrome (or Edge, on Windows), what to
// install and that a new conversation then finds it, and on a Mac (or Linux) Clauseo's own browser: its offer, with a
// line above it when the last download failed in a way the lawyer can act on (TLS, a refused address, the server's
// error, a full disk, no network after the retries); how far a download has got, with Cancel; or
// why there is none (browser-install.ts); under the browser the firm's policy blocks, whose policy Clauseo couldn't
// check, or whose safe connection or unsafe-site list didn't start, the reason in full, and for the policy not checked,
// Try again (browser-policy.ts). Pure, so the tests can read every state; loaded with the popover only.
import type { Lang, Platform } from "@clauseo/components/lib/display.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { toolsLine } from "@clauseo/components/lib/words-tools-line.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import type { ToolGlyphState } from "@clauseo/components/icons/tool-glyph.tsx";
import type { BrowserInstall, InstallProgress } from "./browser-install.ts";
import type { BrowserPolicy } from "./browser-policy.ts";
import { glyphState, reasonOf, toolName, type BrowserState, type KnownTool } from "./tools.ts";

/** Try again under the browser whose policy Clauseo couldn't check: its words, and whether the read it asked for runs. */
export type RecheckPart = { button: string; waiting: boolean };

/** Clauseo's own browser under the browser row with none on the computer, its words resolved. */
export type InstallPart =
  /** Get a browser for Clauseo, with `line` above it (why it is offered again, in words) or null for the plain offer;
   * `waiting` (disabled, busy) from the press until the host says where it stands. */
  | { kind: "offer"; line: string | null; button: string; waiting: boolean }
  /** The download under way or unpacking: `behind` (its update's words) or null, how far in words (empty until the
   * size is known), its bar, and Cancel. */
  | {
      kind: "getting";
      behind: string | null;
      text: string;
      value: number;
      max: number;
      percent: number;
      cancel: string;
    }
  /** No network: the wait, in words, and Cancel. */
  | { kind: "offline"; line: string; cancel: string }
  /** The firm's policy forbids it: the line, and nothing to press. */
  | { kind: "forbidden"; line: string }
  /** In place: the line saying a new conversation gets it. */
  | { kind: "done"; line: string };

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
  /** A muted line under the reason saying it in full: what to install and that a new conversation then finds it, the
   * firm's policy blocks the browser, Clauseo couldn't check it, or its safe connection or unsafe-site list didn't
   * start; else null. */
  note: string | null;
  /** Off in Settings, with the settings window to turn it on: the popover's foot offers Open Settings. */
  settings: boolean;
  /** Under the browser whose policy Clauseo couldn't check, Try again; else null. */
  recheck: RecheckPart | null;
  /** Under the browser with none on a Mac (or Linux), Clauseo's own browser; else null (Windows, or no app feed). */
  install: InstallPart | null;
};

export type ToolRowRoom = {
  /** Whether the host has the settings window, for Open Settings. */
  settings: boolean;
  /** The computer's OS: what to install for the browser (Chrome, or Chrome or Edge on Windows). */
  platform: Platform;
  /** The re-read of the firm's browser policy; null when the window has no app feed to hear it on. */
  policy?: BrowserPolicy | null;
  /** Clauseo's own browser download; null (or left out) when the window has no app feed to hear it on. */
  install?: BrowserInstall | null;
};

const POLICY: ToolsKey =
  "A Chrome or Edge policy on this computer doesn't let apps control the browser, so Clauseo can't use one for you.";
/** On Windows the host passes over a browser its own policy blocks, so these show only when every browser installed
 * is, naming those: the firm's settings don't let apps control them, which only its IT can change (Clauseo's own
 * setting can't: such a browser refuses Clauseo anyway). */
const POLICY_BOTH: ToolsKey =
  "Your firm's settings don't let apps control Microsoft Edge or Google Chrome. Ask your firm's IT to allow this for Clauseo, then start a new conversation.";
const POLICY_EDGE: ToolsKey =
  "Your firm's settings don't let apps control Microsoft Edge. Ask your firm's IT to allow this for Clauseo, then start a new conversation.";
const POLICY_CHROME: ToolsKey =
  "Your firm's settings don't let apps control Google Chrome. Ask your firm's IT to allow this for Clauseo, then start a new conversation.";
/** Windows: the only Chrome is installed for this user alone, which Clauseo's browser can't start. */
const PER_USER: ToolsKey =
  "Clauseo can't use a copy of Google Chrome installed just for you. Install Microsoft Edge, or ask your firm's IT to install Google Chrome for everyone on this computer. Then start a new conversation.";
const UNREADABLE: ToolsKey =
  "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.";
const NO_CONNECTION: ToolsKey =
  "Clauseo couldn't start its safe connection for the browser, so it won't use one for now. It tries again when a new conversation starts.";
const NO_LIST: ToolsKey =
  "Clauseo's unsafe-site list didn't load, so it won't use a browser for now. It tries again when a new conversation starts.";

/** What to install, in full: the browser's skill loads as each conversation starts, so a browser installed later
 * is there in the next new conversation. On a Mac, the browsers Clauseo looks for (host's browser/chrome.rs);
 * Windows takes Edge too. */
const INSTALL_MAC: ToolsKey =
  "Clauseo didn't find Google Chrome, Microsoft Edge, Brave or Chromium on this computer. Install one, then start a new conversation.";
const INSTALL_WINDOWS: ToolsKey =
  "Install Google Chrome or Microsoft Edge, then start a new conversation.";

/** Above the offer, why it is offered again: the last download failed in a way the lawyer can act on, in full (UC-586 to
 * UC-590). */
const TLS: ToolsKey =
  "Clauseo couldn't connect securely to get the browser. Ask your firm's IT team for help.";
const HOST_REFUSED: ToolsKey =
  "The download address was refused, so Clauseo couldn't get the browser. Try again later.";
const SERVER_REFUSED: ToolsKey =
  "Google's download server answered with an error, so Clauseo couldn't get the browser. Try again later.";
const DISK_SPACE: ToolsKey =
  "There isn't enough free space on this computer to get the browser. Free up about 1\u00a0GB and try again.";
const NETWORK: ToolsKey =
  "Clauseo couldn't reach the internet to get the browser. Check your connection and try again.";

/** The browser's reason in full: what to install with none on the computer, its policy keeping it off, or its safe
 * connection or unsafe-site list not started; else null. */
function browserNote(lang: Lang, platform: Platform, state: BrowserState): string | null {
  if (state === "browser_not_installed")
    return tw(lang, platform === "windows" ? INSTALL_WINDOWS : INSTALL_MAC);
  if (state === "browser_blocked_by_policy")
    return tw(lang, platform === "windows" ? POLICY_BOTH : POLICY);
  if (state === "browser_blocked_edge") return tw(lang, POLICY_EDGE);
  if (state === "browser_blocked_chrome") return tw(lang, POLICY_CHROME);
  if (state === "browser_per_user_chrome_only") return tw(lang, PER_USER);
  if (state === "browser_policy_unreadable") return tw(lang, UNREADABLE);
  if (state === "no_proxy") return tw(lang, NO_CONNECTION);
  if (state === "block_list_not_loaded") return tw(lang, NO_LIST);
  return null;
}

/** Try again under the browser whose policy Clauseo couldn't check, by the last read's state: waiting while one runs;
 * null once a read found it clear (the row then says the browser is ready), or with no app feed
 * to hear the read on. */
function recheckPart(lang: Lang, policy: BrowserPolicy | null | undefined): RecheckPart | null {
  if (policy == null || policy.state === "clear") return null;
  return { button: tw(lang, "Try again"), waiting: policy.state === "checking" };
}

const mb = (bytes: number) => Math.round(bytes / 1e6);

/** The line above the offer for a progress that offers again with words of its own; null for the plain offer. */
function offerLine(lang: Lang, progress: InstallProgress | null): string | null {
  switch (progress) {
    case "tls":
      return tw(lang, TLS);
    case "host_refused":
      return tw(lang, HOST_REFUSED);
    case "server_refused":
      return tw(lang, SERVER_REFUSED);
    case "disk_space":
      return tw(lang, DISK_SPACE);
    case "network":
      return tw(lang, NETWORK);
    default:
      return null;
  }
}

/** Clauseo's own browser under the browser row with none, by the download's state: the offer until one starts, and
 * again after a cancel or a failure (with a line above it when the failure has words of its own, else none); never on
 * Windows, whose browser is Edge. */
export function installPart(
  lang: Lang,
  platform: Platform,
  install: BrowserInstall | null | undefined,
): InstallPart | null {
  if (install == null || platform === "windows") return null;
  const p = install.progress;
  const cancel = ui(lang, "Cancel");
  if (
    p === null ||
    p === "tls" ||
    p === "host_refused" ||
    p === "server_refused" ||
    p === "disk_space" ||
    p === "network"
  )
    return {
      kind: "offer",
      line: offerLine(lang, p),
      button: tw(lang, "Get a browser for Clauseo"),
      waiting: install.pressed,
    };
  if (p === "offline")
    return {
      kind: "offline",
      line: tw(lang, "Waiting for an internet connection to get the browser."),
      cancel,
    };
  if (p === "forbidden")
    return {
      kind: "forbidden",
      line: tw(lang, "Your firm doesn't allow Clauseo to download a browser."),
    };
  if (p === "done")
    return {
      kind: "done",
      line: tw(lang, "Clauseo's browser is ready. Start a new conversation to use it."),
    };
  // unpacking: the whole size, at 100%
  const total = p === "unpacking" ? install.total : p.downloading.total;
  const bytes = p === "unpacking" ? total : Math.min(p.downloading.bytes, total);
  const behind = p === "unpacking" ? install.behind : p.downloading.behind;
  // rounded, but never 100% before the last byte
  const percent =
    total === 0
      ? p === "unpacking"
        ? 100
        : 0
      : bytes >= total
        ? 100
        : Math.min(99, Math.round((bytes / total) * 100));
  return {
    kind: "getting",
    behind: behind
      ? tw(lang, "Clauseo's browser is behind on security updates. Updating it now.")
      : null,
    // the first Downloading brings the size: until then the bar has no words
    text:
      total > 0
        ? tw(lang, "Getting Clauseo's browser: {done}\u00a0MB of {total}\u00a0MB ({percent}%)", {
            done: mb(bytes),
            total: mb(total),
            percent,
          })
        : "",
    value: p === "unpacking" ? 1 : bytes,
    max: p === "unpacking" ? 1 : Math.max(total, 1),
    percent,
    cancel,
  };
}

/** One row, for a tool the line speaks for. */
export function toolRow(lang: Lang, tool: KnownTool, room: ToolRowRoom): ToolRowWords {
  const on = tool.state === "available";
  const reason = reasonOf(tool, room.platform);
  const unread = tool.tool === "browser" && tool.state === "browser_policy_unreadable";
  const install =
    tool.tool === "browser" && tool.state === "browser_not_installed"
      ? installPart(lang, room.platform, room.install)
      : null;
  // a read since found nothing blocking, or Clauseo's own browser is in: the next conversation gets the browser
  const ready = (unread && room.policy?.state === "clear") || install?.kind === "done";
  // while Clauseo's own browser downloads, waits or is in, installing Chrome is no longer the step to say
  const busy = install !== null && install.kind !== "offer" && install.kind !== "forbidden";
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
    note:
      tool.tool === "browser" && !ready && !busy
        ? browserNote(lang, room.platform, tool.state)
        : null,
    settings: tool.state === "off" && room.settings,
    recheck: unread ? recheckPart(lang, room.policy) : null,
    install,
  };
}
