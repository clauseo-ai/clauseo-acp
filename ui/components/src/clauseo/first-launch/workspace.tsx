// UC: the first run's Safe workspace step (HANDOFF item 5), after the first sign-in and before the first read, in the
// gate's place in the composer's well. On a computer whose protection needs its one-time set-up with an
// administrator's permission, it says Windows asks once, and the "Unknown publisher" line before Windows' prompt: this
// screen is the confirm (Ruled, 1). "Set up protection" sends the set-up, and the line waits in the act's place for
// the lawyer's Yes; "Not now" goes on without it. Where the set-up asks no administrator, it is only the wait. Refused,
// failed or Not now, the room's line says Not protected in the chosen plan's words, with Try again and Settings, and
// the first read starts: any of these is the step's outcome. The words come from the caller (the first run's, and
// protection's), so the room's protection chunk, which carries these parts, holds no first-run words of its own.
import type { ReactNode } from "react";
import { useDisplay } from "../../lib/display.ts";
import { tbt } from "../../lib/words-toolbar.ts";
import { Button } from "../../ui/button.tsx";
import { NotProtectedMark } from "../not-protected.tsx";
import { Say } from "../say.tsx";
import type { Plan } from "./gate.tsx";

/** Where the step stands: asking (the Windows screen), waiting for the lawyer's Yes, or setting up with no prompt. */
export type WorkspaceStepState = "ask" | "waiting" | "wait";

/** The step's words, in the page's language. */
export type WorkspaceWords = {
  /** "Windows asks once for permission to set up the Safe workspace." */
  ask: string;
  /** The "Unknown publisher" line. */
  unknown: string;
  /** "Set up protection" */
  setUp: string;
  /** "Not now" */
  notNow: string;
  /** "Choose Yes in the window Windows shows." */
  waiting: string;
  /** "Setting up the Safe workspace." */
  wait: string;
  /** "Try again" */
  tryAgain: string;
  /** "Settings" */
  settings: string;
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

/** The step, in the gate's place: it holds the composer's field until it has an outcome. */
export function WorkspaceStep({
  state,
  words,
  onSetUp,
  onNotNow,
}: {
  state: WorkspaceStepState;
  words: WorkspaceWords;
  /** Sends the set-up, once. */
  onSetUp: () => void;
  onNotNow: () => void;
}) {
  const body =
    state === "ask" ? (
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

/** The room's line once the step ended without protection: the mark in the plan's words, the refusal's words when the
 * set-up was refused or failed, Try again (back to the Windows screen) and Settings. */
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
  onRetry: () => void;
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
        <Button kind="quiet" size="small" onClick={onRetry} data-testid="workspace-retry">
          <span className="deva-nudge">{words.tryAgain}</span>
        </Button>
        {onSettings !== undefined && (
          <Button kind="quiet" size="small" onClick={onSettings} data-testid="workspace-settings">
            <span className="deva-nudge">{words.settings}</span>
          </Button>
        )}
      </span>
    </span>
  );
}
