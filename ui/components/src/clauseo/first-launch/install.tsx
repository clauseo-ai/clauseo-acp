// The agent getting ready (design/03-system/src/pages/first-launch/download.tsx), on the working line: at the first
// screen's foot and, once the room is open, at the top of the composer's well. The bar moves only when the host says
// bytes arrived, and says no number and no time; once every file is in, npm's part has no bytes to count and its
// length varies by computer, so it is a calm wait whose pen keeps writing, with words that say what is happening and
// no seconds promised. Offline it waits and resumes by itself; a blocked network, a full disk and any other failure
// each say what happened and give the one or two things to do, in concrete, never in red.
import type { ReactNode } from "react";
import { useDisplay } from "../../lib/display.ts";
import { errorWords } from "../../lib/words-errors.ts";
import { fl } from "../../lib/words-first-launch.ts";
import { Button } from "../../ui/button.tsx";
import type { WorkingState } from "../working-line.tsx";
import { InstallBar as Bar } from "./parts.tsx";

/** Where the install is, as the line shows it. The app reads it from the host's `InstallProgress`. */
export type InstallView =
  /** Not here yet, about to download or paused: the size it will download, in MB. */
  | { kind: "missing"; mb: number }
  | { kind: "downloading"; mb: number; received: number; total: number }
  | { kind: "installing" }
  | { kind: "offline" }
  | { kind: "blocked"; hosts: readonly string[] }
  | { kind: "disk"; need: number; free: number }
  | { kind: "failed"; log: readonly string[] };

/** What the line's buttons do. */
export type InstallActs = {
  retry: () => void;
  proxy: () => void;
  /** Copies the text: the message for IT, or the log. */
  copy: (text: string) => void;
  /** What was copied for the failure on screen now: the line reads it back. */
  copied: "message" | "log" | null;
  setCopied: (what: "message" | "log") => void;
};

export type InstallTestIds = { line?: string; retry?: string; proxy?: string; askIt?: string };

/** The working line for the install, or null when there is nothing to say (no install known, ready, managed). */
export function useInstallLine(
  view: InstallView | null,
  name: string,
  acts: InstallActs,
  ids: InstallTestIds = {},
): WorkingState | null {
  const { lang } = useDisplay();
  if (view === null) return null;
  const w = (key: Parameters<typeof fl>[1], slots?: Record<string, string | number>) =>
    fl(lang, key, slots);
  const wrap = (children: ReactNode) => (
    <span className="fl-install" data-testid={ids.line} data-state={view.kind}>
      {children}
    </span>
  );
  const retry = (
    <Button
      kind="tone"
      size="small"
      className="working-act"
      onClick={acts.retry}
      data-testid={ids.retry}
    >
      {w("Try again")}
    </Button>
  );
  switch (view.kind) {
    case "missing": {
      const words = w("Getting {name} ready. About {mb} MB.", { name, mb: view.mb });
      return { kind: "wait", words, pill: null, actions: wrap(null) };
    }
    case "downloading": {
      const words = w("Getting {name} ready. About {mb} MB.", { name, mb: view.mb });
      return {
        kind: "wait",
        words,
        pill: null,
        actions: wrap(<Bar received={view.received} total={view.total} words={words} />),
      };
    }
    case "installing":
      return {
        kind: "wait",
        words: w("Setting up {name} on this computer.", { name }),
        pill: null,
        working: true,
        actions: wrap(null),
      };
    case "offline":
      return {
        kind: "wait",
        words: errorWords(lang, "agent.install_offline"),
        pill: null,
        code: "agent.install_offline",
        actions: wrap(null),
      };
    case "blocked": {
      const message = w(
        "Clauseo needs to download {name} to work. Please allow these addresses on the firm's network: {list}.",
        { name, list: view.hosts.join(", ") },
      );
      return {
        kind: "wait",
        words: errorWords(lang, "agent.install_blocked"),
        pill: null,
        code: "agent.install_blocked",
        actions: wrap(
          <>
            <Button
              kind="tone"
              size="small"
              className="working-act"
              onClick={acts.proxy}
              data-testid={ids.proxy}
            >
              {w("Use the system proxy")}
            </Button>
            <Button
              kind="quiet"
              size="small"
              className="working-act"
              data-testid={ids.askIt}
              onClick={() => {
                acts.copy(message);
                acts.setCopied("message");
              }}
            >
              {acts.copied === "message" ? w("Message copied") : w("Ask your IT team")}
            </Button>
          </>,
        ),
      };
    }
    case "disk":
      return {
        kind: "wait",
        words: w("{name} needs {need} MB free on this computer, and {free} MB is free.", {
          name,
          need: view.need,
          free: view.free,
        }),
        pill: null,
        code: "agent.install_disk_full",
        actions: wrap(retry),
      };
    case "failed":
      return {
        kind: "wait",
        words:
          acts.copied === "log"
            ? w(
                "{name} couldn't be downloaded. The log is copied, with nothing from your matters in it.",
                { name },
              )
            : w("{name} couldn't be downloaded.", { name }),
        pill: null,
        code: "agent.install_failed",
        actions: wrap(
          <>
            {retry}
            <Button
              kind="quiet"
              size="small"
              className="working-act"
              onClick={() => {
                acts.copy(view.log.join("\n"));
                acts.setCopied("log");
              }}
            >
              {w("Details")}
            </Button>
          </>,
        ),
      };
  }
}
