// The agent getting ready (design/03-system/src/pages/first-launch/download.tsx), on the working line: at the first
// screen's foot and, once the room is open, at the top of the composer's well. The bar moves only when the host says
// bytes arrived, and says no number and no time; once every file is in, npm's part has no bytes to count and its
// length varies by computer, so it is a calm wait whose pen keeps writing, with words that say what is happening and
// no seconds promised. Offline it waits and resumes by itself; a blocked network, a full disk and any other failure
// each say what happened and give the one or two things to do, in concrete, never in red.
import type { ReactNode } from "react";
import type { Lang } from "../../lib/display.ts";
import { type AgentName, errorWords } from "../../lib/words-errors.ts";
import { fl } from "../../lib/words-first-launch.ts";
import { Button } from "../../ui/button.tsx";
import { CopyForIt } from "../copy-for-it.tsx";
import type { WorkingState } from "../working-line.tsx";
import { InstallBar as Bar } from "./parts.tsx";

/** Where the install is, as the line shows it. The app reads it from the host's `InstallProgress`. */
export type InstallView =
  /** Not here yet, about to download or paused: the size it will download, about, in MB. 0 when
   * no size is known, and then the line says nothing. `reinstall`: Clauseo installed it here before
   * and its folder went (#859); with `InstallActs.install` the line says so and offers it again. */
  | { kind: "missing"; mb: number; reinstall?: boolean }
  /** Downloading: the bytes for the bar, and the whole MB the words say (done never above
   * total). */
  | {
      kind: "downloading";
      received: number;
      total: number;
      doneMb: number;
      totalMb: number;
    }
  | { kind: "installing" }
  | { kind: "offline" }
  | { kind: "blocked"; hosts: readonly string[] }
  | { kind: "disk"; need: number; free: number }
  /** `setup`: every file downloaded, and setting them up failed (agent.install_setup_failed). */
  | { kind: "failed"; log: readonly string[]; setup?: boolean };

/** What the line's buttons do. */
export type InstallActs = {
  retry: () => void;
  proxy: () => void;
  /** Copies the log ("Copy details for IT" copies its message itself, copy-for-it.tsx). */
  copy: (text: string) => void;
  /** Whether the log was copied for the failure on screen now: the line reads it back. */
  copied: "log" | null;
  setCopied: (what: "log") => void;
  /** Where no gate stands beside the line (the first screen, Settings' Plans): the room gate's Sign in, offered while
   * the plan isn't here yet. Desktop starts the download on the press and keeps it, so the sign-in goes on once the
   * plan is ready (0033, "When an install starts"). Left out in a matter, whose gate has the press. */
  signIn?: (() => void) | undefined;
  /** The Sign in names its plan, "Sign in with {name}": where both plans stand side by side (the first screen). */
  named?: boolean | undefined;
  /** The plan's install, the normal one (`install_agent`, its size, progress and failures on this line): offered as
   * "Install {plan}" for a plan whose install went missing after Clauseo installed it (#859). Left out, that plan's
   * line is the plain missing one. */
  install?: (() => void) | undefined;
};

export type InstallTestIds = {
  line?: string;
  install?: string;
  retry?: string;
  proxy?: string;
  askIt?: string;
  signIn?: string;
};

/** The first screen's line while the sign-in status check runs and the install has no line of its own: the check's
 * words, and Sign in busy (disabled, `aria-busy`) until the check answers. The room gate shows the same in the composer. */
export function checkingLine(
  lang: Lang,
  name: string,
  ids: InstallTestIds = {},
  named = false,
): WorkingState {
  const w = (key: Parameters<typeof fl>[1], slots?: Record<string, string | number>) =>
    fl(lang, key, slots);
  return {
    kind: "wait",
    words: w("Checking whether you're signed in to {name}…", { name }),
    pill: null,
    actions: (
      <span className="fl-install" data-testid={ids.line} data-state="checking">
        <Button
          kind="tone"
          size="small"
          className="working-act"
          disabled
          aria-busy="true"
          data-testid={ids.signIn}
        >
          {named ? w("Sign in with {name}", { name }) : w("Sign in")}
        </Button>
      </span>
    ),
  };
}

/** The working line for the install, in the language, or null when there is nothing to say (no install known, ready,
 * managed). A plain function, no hook: the first run builds it outside render too, as the setup store changes. */
export function installLine(
  lang: Lang,
  view: InstallView | null,
  name: string,
  acts: InstallActs,
  ids: InstallTestIds = {},
): WorkingState | null {
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
  /** Goes through the OS's proxy this time: the words and the handler the Blocked view has. */
  const proxyButton = (kind: "tone" | "quiet") => (
    <Button
      kind={kind}
      size="small"
      className="working-act"
      onClick={acts.proxy}
      data-testid={ids.proxy}
    >
      {w("Use the system proxy")}
    </Button>
  );
  switch (view.kind) {
    case "missing":
      // installed here before and gone (#859): it says so whatever the size, and Install starts the normal install
      if (view.reinstall === true && acts.install !== undefined)
        return {
          kind: "wait",
          words: w("{plan} isn't installed on this computer any more.", { plan: name }),
          pill: null,
          actions: (
            <span className="fl-install" data-testid={ids.line} data-state="reinstall">
              <Button
                kind="tone"
                size="small"
                className="working-act"
                onClick={acts.install}
                data-testid={ids.install}
              >
                {w("Install {plan}", { plan: name })}
              </Button>
            </span>
          ),
        };
      // no size to say (a download of no bytes): no line at all, so the next state is a change
      // the reader hears
      if (view.mb === 0) return null;
      return {
        kind: "wait",
        words: w("Needs a download of about {mb} MB", { mb: view.mb }),
        pill: null,
        actions: wrap(
          acts.signIn === undefined ? null : (
            <Button
              kind="tone"
              size="small"
              className="working-act"
              onClick={acts.signIn}
              data-testid={ids.signIn}
            >
              {acts.named === true ? w("Sign in with {name}", { name }) : w("Sign in")}
            </Button>
          ),
        ),
      };
    case "downloading": {
      const words = w("Getting {name} ready… {done} MB of {total} MB", {
        name,
        done: view.doneMb,
        total: view.totalMb,
      });
      return {
        kind: "wait",
        words,
        // the bytes move every 250 ms: the live region hears only that the download goes on
        said: w("Getting {name} ready…", { name }),
        pill: null,
        actions: wrap(<Bar received={view.received} total={view.total} words={words} />),
      };
    }
    case "installing":
      return {
        kind: "wait",
        words: w("Setting up {name}…", { name }),
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
        actions: wrap(
          <>
            {retry}
            {proxyButton("quiet")}
          </>,
        ),
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
            {proxyButton("tone")}
            <CopyForIt
              lang={lang}
              text={message}
              className="working-act"
              {...(ids.askIt === undefined ? {} : { testId: ids.askIt })}
            />
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
        // the agent by its name: Claude's sentence for Claude, the named one (provisional) for another
        words: view.setup
          ? errorWords(lang, "agent.install_setup_failed", setupAgent(name))
          : acts.copied === "log"
            ? w(
                "{name} couldn't be downloaded. The log is copied, with nothing from your matters in it.",
                { name },
              )
            : w("{name} couldn't be downloaded.", { name }),
        pill: null,
        code: view.setup ? "agent.install_setup_failed" : "agent.install_failed",
        actions: wrap(
          <>
            {retry}
            {/* a set-up failure is not the network's: no proxy offered */}
            {view.setup ? null : proxyButton("quiet")}
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

/** The install line's agent for the set-up's sentence (plannedKey): Claude by its kind, any other by its name. */
function setupAgent(name: string): AgentName {
  return name === "Claude" ? { kind: "claude", name } : { kind: undefined, name };
}
