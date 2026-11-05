// Restoring the matter to how it was, or undoing one change. The system's dialog, naming the time and every document
// that will change, and saying it can be undone. Undoing a change asks about each document changed again since, Undo
// anyway or Keep current, the safe one chosen until the lawyer says otherwise. Both move forward and can be undone, so
// the act is the dark primary and focus starts on it. Where Word holds a file, the dialog says which and offers Try
// again. While an answer runs, Restore and Undo wait and say until when. restoreWords and undoWords in
// versions-model.ts compose what it says.
import { Dialog as Base } from "@base-ui/react/dialog";
import { useId, useState } from "react";
import { Button } from "../ui/button.tsx";
import { focusPlane } from "../ui/dialog-start.ts";
import { SegmentedSwitch } from "../ui/segmented.tsx";
import {
  dialogActions,
  dialogBody,
  dialogPlace,
  dialogPlane,
  dialogTitle,
  scrim,
} from "../ui/recipes-overlays.ts";
import { IconChecking } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import { Say } from "./say.tsx";

export type EditedChoice = "undo" | "keep";
export type EditedRow = { id: string; title: string; at: string; choice: EditedChoice };

/** What the dialog does when the lawyer goes ahead: restore the matter, or undo one change. */
export type RestoreAct = "restore" | "undo";

export type RestoreAsk =
  | {
      kind: "confirm";
      act?: RestoreAct;
      title: string;
      body: string;
      stays: string | null;
      edited: readonly EditedRow[];
      editedHead: string | null;
      /** Optional: why the act just failed, when no file can be named: said in the dialog, which offers Try again. */
      failed?: string | null;
    }
  | { kind: "word"; act?: RestoreAct; files: readonly string[] };

export function RestoreDialog({
  ask,
  waiting,
  onClose,
  onChoice,
  onRestore,
  onTryAgain,
  held = false,
  testIds,
}: {
  /** What the dialog asks, or null while it's closed. */
  ask: RestoreAsk | null;
  /** While an answer runs, the line that says Restore waits and until when. */
  waiting: string | null;
  onClose: () => void;
  onChoice: (id: string, choice: EditedChoice) => void;
  onRestore: () => void;
  onTryAgain: () => void;
  /** Optional: the act can't go ahead as chosen, such as an undo that would put nothing back: faint, aria-disabled,
   * still focusable, as while an answer runs. */
  held?: boolean;
  /** Optional: test ids for the e2e specs: on the act, and on Try again. */
  testIds?: { confirm?: string; tryAgain?: string };
}) {
  const w = useVersionWords();
  const [{ mount, start }] = useState(focusPlane);
  const waitId = useId();
  return (
    <Base.Root open={ask !== null} onOpenChange={(open) => !open && onClose()}>
      <Base.Portal>
        <Base.Backdrop className={scrim} />
        {/* restoring and undoing both move forward and can be undone: the act is the dark primary, and focus starts on it */}
        <Base.Popup
          ref={mount}
          data-dialog-plane=""
          className={cn(dialogPlace, dialogPlane, "ver-dialog")}
          initialFocus={start(true)}
        >
          {ask?.kind === "word" ? (
            <>
              <Base.Title className={dialogTitle}>
                <Say>
                  {ask.files.length === 1
                    ? w.v(
                        ask.act === "undo"
                          ? "Close '{file}' in Word to undo the change."
                          : "Close '{file}' in Word to restore it.",
                        { file: ask.files[0] ?? "" },
                      )
                    : w.v(
                        ask.act === "undo"
                          ? "Close {files} in Word to undo the change."
                          : "Close {files} in Word to restore it.",
                        { files: w.files(ask.files) },
                      )}
                </Say>
              </Base.Title>
              <p className={dialogBody}>
                <Say>
                  {ask.files.length === 1
                    ? w.v("If it isn't open in Word, close Word and try again.")
                    : w.v("If they aren't open in Word, close Word and try again.")}
                </Say>
              </p>
              <Base.Description className={dialogBody}>
                {w.v("Word holds the file while it is open. Nothing has changed yet.")}
              </Base.Description>
              <div className={dialogActions} data-dialog-acts="">
                <Base.Close render={<Button kind="quiet" />}>{w.ui("Cancel")}</Base.Close>
                <Button
                  kind="primary"
                  onClick={onTryAgain}
                  className="ver-try"
                  data-testid={testIds?.tryAgain}
                >
                  {w.v("Try again")}
                </Button>
              </div>
            </>
          ) : ask?.kind === "confirm" ? (
            <>
              <Base.Title className={dialogTitle}>
                <Say>{ask.title}</Say>
              </Base.Title>
              <Base.Description className={dialogBody}>
                <Say>{ask.body}</Say>
              </Base.Description>
              {ask.stays !== null && (
                <p className="ver-dialog-note">
                  <Say>{ask.stays}</Say>
                </p>
              )}
              {ask.edited.length > 0 && ask.editedHead !== null && (
                <div className="ver-edited" role="group" aria-label={ask.editedHead}>
                  <p className="ver-edited-head">
                    <Say>{ask.editedHead}</Say>
                  </p>
                  <ul className="ver-edited-list">
                    {ask.edited.map((r) => (
                      <li key={r.id} className="ver-edited-row">
                        <span className="ver-edited-what">
                          <span className="ver-edited-name">
                            <Say>{r.title}</Say>
                          </span>
                          <span className="ver-edited-at">
                            <Say>{r.at}</Say>
                          </span>
                        </span>
                        <SegmentedSwitch<EditedChoice>
                          label={w.v("For {title}", { title: r.title })}
                          value={r.choice}
                          onValueChange={(c) => onChoice(r.id, c)}
                          options={[
                            { value: "undo", label: w.ui("Undo anyway") },
                            { value: "keep", label: w.ui("Keep current") },
                          ]}
                        />
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              {ask.failed != null && (
                <p className="ver-dialog-note ver-dialog-failed" role="alert">
                  <Say>{ask.failed}</Say>
                </p>
              )}
              {waiting !== null && (
                <p className="ver-wait" id={waitId}>
                  <IconChecking size={14} className="ver-wait-icon" />
                  <span>
                    <Say>{waiting}</Say>
                  </span>
                </p>
              )}
              <div className={dialogActions} data-dialog-acts="">
                <Base.Close render={<Button kind="quiet" />}>{w.ui("Cancel")}</Base.Close>
                {ask.failed != null ? (
                  <Button
                    kind="primary"
                    className="ver-try"
                    data-testid={testIds?.tryAgain}
                    aria-disabled={waiting !== null || held ? true : undefined}
                    aria-describedby={waiting !== null ? waitId : undefined}
                    data-disabled={waiting !== null || held ? "" : undefined}
                    onClick={() => waiting === null && !held && onTryAgain()}
                  >
                    {w.v("Try again")}
                  </Button>
                ) : (
                  <Button
                    kind="primary"
                    className="ver-do-restore"
                    data-testid={testIds?.confirm}
                    aria-disabled={waiting !== null || held ? true : undefined}
                    aria-describedby={waiting !== null ? waitId : undefined}
                    data-disabled={waiting !== null || held ? "" : undefined}
                    onClick={() => waiting === null && !held && onRestore()}
                  >
                    {ask.act === "undo" ? w.t("Undo") : w.ui("Restore")}
                  </Button>
                )}
              </div>
            </>
          ) : null}
        </Base.Popup>
      </Base.Portal>
    </Base.Root>
  );
}
