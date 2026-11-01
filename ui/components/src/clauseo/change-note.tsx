// The change note. After Clauseo changes papers, the thread says so in one line, with the papers named and two ways
// on: Compare, and Undo, each when the caller gives it. A refused undo says why in a quiet line beside Undo. When a
// paper was edited after Clauseo's change, Undo asks first, in the alert dialog, with Keep current as the safe choice.
import { Fragment, useRef, useState } from "react";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";
import { IconOpen, IconPaperBookPage, IconUndo } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { Say } from "./say.tsx";

/** Clauseo's changes standing, or undone by the lawyer. */
export type ChangeState = "changed" | "undone";

/** What the note says, with one paper read in the singular. */
function useCounts(n: number) {
  const w = useThreadWords();
  return {
    changed:
      n === 1 ? w.th("Clauseo changed 1 paper.") : w.th("Clauseo changed {n} papers.", { n }),
    undone:
      n === 1
        ? w.th("You undid Clauseo's changes to 1 paper.")
        : w.th("You undid Clauseo's changes to {n} papers.", { n }),
  };
}

export function ChangeNote({
  papers,
  editedSince = null,
  state: held,
  onUndo,
  onCompare,
  undoDisabled = false,
  error = null,
  testIds,
}: {
  /** The papers Clauseo changed, by their names on the wall. */
  papers: readonly string[];
  /** A paper edited after Clauseo's change, if one was: Undo asks first. */
  editedSince?: string | null;
  /** Given, the host says when the changes are undone; without it, Undo marks them undone itself. */
  state?: ChangeState;
  /** Undo shows only when given. */
  onUndo?: () => void;
  /** Compare shows only when given. */
  onCompare?: () => void;
  /** Undo can't be pressed for now, such as while Clauseo answers. */
  undoDisabled?: boolean;
  /** A refused undo: its words, in the language, and its code, as a quiet line beside Undo. */
  error?: { words: string; code: string } | null;
  testIds?: { paper?: string; undo?: string; error?: string };
}) {
  const w = useThreadWords();
  const counts = useCounts(papers.length);
  const [own, setOwn] = useState<ChangeState>("changed");
  const state = held ?? own;
  const [asking, setAsking] = useState(false);
  const keep = useRef<HTMLButtonElement>(null);
  const undo = () => {
    if (held === undefined) setOwn("undone");
    onUndo?.();
  };

  if (state === "undone") {
    return (
      <p className="change-note change-undone">
        <IconUndo size={16} className="change-icon" />
        <span>
          <Say>{counts.undone}</Say>
        </span>
      </p>
    );
  }
  return (
    <div className="change-note" role="group" aria-label={counts.changed}>
      <IconPaperBookPage size={16} className="change-icon" />
      <p className="change-text">
        <span className="change-what">
          <Say>{counts.changed}</Say>
        </span>
        <span className="change-papers">
          {papers.map((p, i) => (
            <Fragment key={`${i}:${p}`}>
              {i > 0 && ", "}
              <span data-testid={testIds?.paper}>
                <bdi>
                  <Say>{p}</Say>
                </bdi>
              </span>
            </Fragment>
          ))}
        </span>
      </p>
      <div className="change-actions">
        {error !== null && (
          <span
            className="change-error"
            role="alert"
            data-code={error.code}
            data-testid={testIds?.error}
          >
            <Say>{error.words}</Say>
          </span>
        )}
        {onCompare !== undefined && (
          <Button kind="quiet" size="small" onClick={onCompare}>
            <IconOpen size={16} />
            <span className="deva-nudge">{w.th("Compare")}</span>
          </Button>
        )}
        {onUndo !== undefined && (
          <Button
            kind="quiet"
            size="small"
            disabled={undoDisabled}
            data-testid={testIds?.undo}
            onClick={() => (editedSince === null ? undo() : setAsking(true))}
          >
            <IconUndo size={16} />
            <span className="deva-nudge">{w.t("Undo")}</span>
          </Button>
        )}
      </div>
      {editedSince !== null && onUndo !== undefined && (
        <AlertDialog open={asking} onOpenChange={setAsking}>
          <AlertDialogPopup
            initialFocus={keep}
            title={w.th("{name} changed after Clauseo's edit.", { name: editedSince })}
            body={w.ui(
              "Undo anyway puts back the version from before Clauseo's edit. The changes made since stay in Versions.",
            )}
            actions={
              <>
                <AlertDialogClose render={<Button kind="quiet" />} onClick={undo}>
                  {w.ui("Undo anyway")}
                </AlertDialogClose>
                <AlertDialogClose render={<Button kind="tone" ref={keep} />}>
                  {w.ui("Keep current")}
                </AlertDialogClose>
              </>
            }
          />
        </AlertDialog>
      )}
    </div>
  );
}
