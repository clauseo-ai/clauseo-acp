// The words of each row in the tool line's popover: the tool's name, where it stands, and the act it offers, with the
// Chrome download under the browser's row when there's no Chrome. Pure, so the tests can read every state; loaded
// with the popover only.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { tw } from "@clauseo/components/lib/words-tools.ts";
import type { BrowserInstall } from "./browser-install.ts";
import type { KnownTool } from "./tools.ts";

/** What a row offers: Open Settings, Download Chrome, or Try again after a failed download. */
export type ToolAction = "settings" | "download" | "retry";

export type ToolRowWords = {
  name: string;
  /** Muted lines under the name, in order. */
  lines: string[];
  /** The download's bar: how far, and its words, which say it for screen readers too. */
  progress: { value: number; max: number; label: string } | null;
  action: ToolAction | null;
};

export type ToolRowRoom = {
  /** The plan's name, as the status line names it: "Claude". */
  plan: string;
  /** Whether the host has the settings window, for Open Settings. */
  settings: boolean;
  /** The app's Chrome download; null when the host reports none. */
  install: BrowserInstall | null;
};

const mb = (bytes: number) => Math.round(bytes / 1e6);

/** The download's part of the browser row, under "Off. There's no Chrome on this computer."; nothing without a state
 * from the host. */
function downloadPart(lang: Lang, install: BrowserInstall | null): Omit<ToolRowWords, "name"> {
  const none = { lines: [], progress: null, action: null };
  if (install === null) return none;
  const p = install.progress;
  if (p === "unpacking")
    return {
      lines: [],
      progress: { value: 1, max: 1, label: tw(lang, "Unpacking Chrome.") },
      action: null,
    };
  if (p === "done")
    return { ...none, lines: [tw(lang, "Chrome is ready. New conversations get the browser.")] };
  if (p !== null && "downloading" in p) {
    const { bytes, total } = p.downloading;
    return {
      lines: [],
      progress: {
        value: Math.min(bytes, total),
        max: Math.max(total, 1),
        label: tw(lang, "Downloading Chrome. {done} of {total} MB.", {
          done: mb(bytes),
          total: mb(total),
        }),
      },
      action: null,
    };
  }
  if (p !== null) {
    const code = p.failed.code;
    const line =
      code === "disk"
        ? tw(lang, "Chrome couldn't be downloaded. It needs about 1 GB free on this computer.")
        : code === "refused"
          ? tw(lang, "Your network is blocking the download.")
          : code === "check"
            ? tw(lang, "The download didn't match Google's, so Clauseo deleted it.")
            : tw(lang, "Chrome couldn't be downloaded.");
    return { lines: [line], progress: null, action: "retry" };
  }
  if (install.offer?.offer === "download")
    return {
      lines: [
        tw(
          lang,
          "Clauseo can download Chrome for its own use: about 200 MB from Google, once for every conversation. It needs about 1 GB free.",
        ),
      ],
      progress: null,
      action: "download",
    };
  if (install.offer?.offer === "none")
    return { ...none, lines: [tw(lang, "Install Google Chrome to use the browser.")] };
  return none;
}

/** One row's words, for a tool the line speaks for. */
export function toolRow(lang: Lang, tool: KnownTool, room: ToolRowRoom): ToolRowWords {
  const off = (): ToolRowWords => ({
    name,
    lines: [tw(lang, "Off. You turned it off in Settings.")],
    progress: null,
    action: room.settings ? "settings" : null,
  });
  const say = (line: string): ToolRowWords => ({
    name,
    lines: [line],
    progress: null,
    action: null,
  });
  const managed = () =>
    say(tw(lang, "Off. {managed}.", { managed: st(lang, "Managed by your organisation") }));
  const name = tw(lang, tool.tool === "web_search" ? "Web search" : "Browser");
  if (tool.tool === "web_search") {
    switch (tool.state) {
      case "available":
        return say(tw(lang, "On. Clauseo can search the web and read pages."));
      case "off":
        return off();
      case "locked_off":
        return managed();
      case "no_search_on_this_login":
        return say(
          tw(lang, "This {name} login can't search the web. Clauseo can still read web pages.", {
            name: room.plan,
          }),
        );
    }
  }
  switch (tool.state) {
    case "available":
      return say(tw(lang, "On. Clauseo can open web pages in its own browser."));
    case "off":
      return off();
    case "locked_off":
      return managed();
    case "managed_by_firm":
      return say(
        tw(
          lang,
          "Off. Your organisation chooses the tools {name} has on this computer, so Clauseo adds none.",
          { name: room.plan },
        ),
      );
    case "no_proxy":
      return say(tw(lang, "Off until Clauseo can check each website the browser opens."));
    case "too_many_browsers":
      return say(
        tw(
          lang,
          "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
        ),
      );
    case "browser_not_installed": {
      const part = downloadPart(lang, room.install);
      return {
        name,
        ...part,
        lines: [tw(lang, "Off. There's no Chrome on this computer."), ...part.lines],
      };
    }
  }
}
