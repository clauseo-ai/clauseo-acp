// UC: the first run's Safe workspace step (HANDOFF item 5), after the first sign-in and before the
// first read, in the gate's place in the composer's well. Design B's installer sets the Safe
// workspace up, so a normal install has no step. Where the installer's admin copy is missing, the
// step is the repair: the cause's line, the repair's line, "Get the installer" on a per-user
// install (it opens the installed version's release page; the caller's act), and "Not now". Where
// the app can fetch the installer itself, "Download and run the installer again" takes the act's
// place (disabled, with its waiting line, while it runs; a refusal's words under the lines), and Get
// the installer moves beside Not now as the fallback. Where
// the set-up is due with the copy there (after an in-app Remove) and asks an administrator, it says
// Windows asks, and the "Unknown publisher" line before Windows' prompt: this screen is the confirm
// (Ruled, 1). "Set up the Safe workspace" sends the set-up, and the line waits in the act's place
// for the lawyer's Yes; "Not now" goes on without it. Where the set-up asks no administrator, it is
// only the wait. Refused, failed or Not now, the room's line says Not protected in the chosen
// plan's words, with Try again (not for the repair) and Settings, and the first read starts. The
// words come from the caller (the first run's, and protection's), so the room's protection chunk,
// which carries these parts, holds no first-run words of its own.
import type { ReactNode } from "react";
import { useDisplay } from "../../lib/display.ts";
import { pw, INSTALLER_CLOSE } from "../../lib/words-protection.ts";
import { tbt } from "../../lib/words-toolbar.ts";
import { Button } from "../../ui/button.tsx";
import { NotProtectedMark } from "../not-protected.tsx";
import { Say } from "../say.tsx";
import type { Plan } from "./gate.tsx";

/** Where the step stands: the repair (the admin copy missing), asking (the Windows screen), waiting
 * for the lawyer's Yes, or setting up with no prompt. */
export type WorkspaceStepState = "repair" | "ask" | "waiting" | "wait";

/** The step's words, in the page's language. */
export type WorkspaceWords = {
  /** "Windows asks once for permission to set up the Safe workspace." */
  ask: string;
  /** The "Unknown publisher" line. */
  unknown: string;
  /** "Set up the Safe workspace" */
  setUp: string;
  /** "Choose Yes in the window Windows shows." */
  waiting: string;
  /** "Not now" */
  notNow: string;
  /** "Setting up the Safe workspace." */
  wait: string;
  /** "Try again" */
  tryAgain: string;
  /** "Settings" */
  settings: string;
  /** "Get the installer" */
  getInstaller: string;
  /** "Download and run the installer again" */
  repairAgain: string;
};

// the gate's rows, as parts.tsx's Act and Ways draw them (first-launch.css), written here so the step rides in the
// room's protection chunk without the first run's parts

/** The act's row: the words, and the button where Ask will stand. */
const Act = ({ children, act }: { children: ReactNode; act?: ReactNode }) => (
  <div className="fl-gate-act">
    <span className="fl-gate-words" role="status">
      <span className="deva-nudge">{children}</span>
    </span>
    {act}
  </div>
);

/** "Download and run the installer again": the act's button, disabled and busy while it runs. */
const RepairAgain = ({
  words,
  onRepair,
  busy,
}: {
  words: WorkspaceWords;
  onRepair: () => void;
  busy: boolean;
}) => (
  <Button
    kind="tone"
    className="room-field-ask"
    onClick={onRepair}
    disabled={busy}
    aria-busy={busy}
    data-testid="protection-repair-again"
  >
    <span className="deva-nudge">{words.repairAgain}</span>
  </Button>
);

/** The step, in the gate's place: it holds the composer's field until it has an outcome. */
export function WorkspaceStep({
  state,
  words,
  repair = [],
  onGetInstaller,
  repairRun = {},
  onSetUp,
  onNotNow,
}: {
  state: WorkspaceStepState;
  words: WorkspaceWords;
  /** The repair's lines, for "repair": the cause's, then what to do. */
  repair?: string[];
  /** Opens the installer's page; without it (a firm install, a test build), no button. */
  onGetInstaller?: (() => void) | undefined;
  /** The in-app repair, for "repair": its press (without it, no button), its waiting line while it
   * runs, and the lines after the repair's (a refusal's words, the installer that didn't finish). */
  repairRun?: { onRepair?: (() => void) | undefined; wait?: string | null; notes?: string[] };
  /** Sends the set-up, once. */
  onSetUp: () => void;
  onNotNow: () => void;
}) {
  const { lang } = useDisplay();
  const { onRepair, wait = null, notes = [] } = repairRun;
  const installer =
    onGetInstaller === undefined ? null : (
      <Button
        kind={onRepair === undefined ? "tone" : "quiet"}
        size={onRepair === undefined ? "default" : "small"}
        {...(onRepair === undefined ? { className: "room-field-ask" } : {})}
        onClick={onGetInstaller}
        data-testid="protection-installer"
      >
        <span className="deva-nudge">{words.getInstaller}</span>
      </Button>
    );
  const body =
    state === "repair" ? (
      <>
        <Act
          act={
            onRepair === undefined ? (
              (installer ?? undefined)
            ) : (
              <RepairAgain words={words} onRepair={onRepair} busy={wait !== null} />
            )
          }
        >
          <span data-testid="protection-repair">
            <Say>{repair.join(" ")}</Say>
          </span>
          {notes.map((line) => (
            <span key={line} data-testid="protection-repair-note">
              <Say>{line}</Say>
            </span>
          ))}
          {wait !== null && (
            <span data-testid="protection-repair-wait">
              <Say>{wait}</Say>
            </span>
          )}
          {onGetInstaller !== undefined && (
            <span data-testid="protection-close-line">
              <Say>{pw(lang, INSTALLER_CLOSE)}</Say>
            </span>
          )}
        </Act>
        <div className="fl-gate-ways">
          {onRepair !== undefined && installer}
          <Button kind="quiet" size="small" onClick={onNotNow} data-testid="workspace-not-now">
            <span className="deva-nudge">{words.notNow}</span>
          </Button>
        </div>
      </>
    ) : state === "ask" ? (
      <>
        <Act
          act={
            <Button
              kind="tone"
              className="room-field-ask"
              onClick={onSetUp}
              data-testid="protection-setup"
            >
              <span className="deva-nudge">{words.setUp}</span>
            </Button>
          }
        >
          <Say>{words.ask}</Say>
        </Act>
        <div className="fl-gate-ways">
          <span className="fl-gate-note">
            <Say>{words.unknown}</Say>
          </span>
          <Button kind="quiet" size="small" onClick={onNotNow} data-testid="workspace-not-now">
            <span className="deva-nudge">{words.notNow}</span>
          </Button>
        </div>
      </>
    ) : (
      <Act>
        <span data-testid="protection-wait">
          <Say>{state === "waiting" ? words.waiting : words.wait}</Say>
        </span>
      </Act>
    );
  return (
    <div className="fl-gate" data-testid="workspace-setup" data-state={state}>
      <div className="fl-gate-body">{body}</div>
    </div>
  );
}

/** The room's line once the step ended without the Safe workspace: the mark in the plan's words,
 * the refusal's words when the set-up was refused or failed, Try again (back to the
 * Windows screen; not for the repair) and Settings. */
export function WorkspaceOff({
  plan,
  words,
  failed,
  onRetry,
  onSettings,
}: {
  plan: Plan;
  words: WorkspaceWords;
  /** The refusal, in its code's words; null after Not now. */
  failed: string | null;
  /** Sends the set-up again; without it (the repair is the installer), no Try again. */
  onRetry?: (() => void) | undefined;
  /** Opens Settings; without it, no Settings button. */
  onSettings?: (() => void) | undefined;
}) {
  const { lang } = useDisplay();
  return (
    <span className="fl-ws-off" data-testid="workspace-setup" data-state="off">
      <NotProtectedMark
        meaning={
          plan === "chatgpt"
            ? tbt(
                lang,
                "Clauseo can change files in this matter without asking. It asks before going beyond it.",
              )
            : tbt(lang, "Clauseo asks before each change in this matter.")
        }
      />
      {failed !== null && (
        <span className="fl-ws-failed" role="status" data-testid="protection-failed">
          <Say>{failed}</Say>
        </span>
      )}
      <span className="fl-ws-acts">
        {onRetry !== undefined && (
          <Button kind="quiet" size="small" onClick={onRetry} data-testid="workspace-retry">
            <span className="deva-nudge">{words.tryAgain}</span>
          </Button>
        )}
        {onSettings !== undefined && (
          <Button kind="quiet" size="small" onClick={onSettings} data-testid="workspace-settings">
            <span className="deva-nudge">{words.settings}</span>
          </Button>
        )}
      </span>
    </span>
  );
}
