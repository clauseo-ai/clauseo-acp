// The change note. After Clauseo changes papers, the thread says so in one line, with the papers named and two ways
// on: Compare, and Undo. When a paper was edited after Clauseo's change, Undo asks first, in the alert dialog, with
// Keep current as the safe choice.
import { useRef, useState } from "react";
import { AlertDialog, AlertDialogClose, AlertDialogPopup } from "../ui/alert-dialog.tsx";
import { Button } from "../ui/button.tsx";
import { IconOpen, IconPaperBookPage, IconUndo } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { Say } from "./say.tsx";

/** Clauseo's changes standing, or undone by the lawyer. */
export type ChangeState = "changed" | "undone";

export function ChangeNote({
  papers,
  editedSince = null,
  state: held,
  onUndo,
  onCompare,
}: {
  /** The papers Clauseo changed, by their names on the wall. */
  papers: readonly string[];
  /** A paper edited after Clauseo's change, if one was: Undo asks first. */
  editedSince?: string | null;
  state?: ChangeState;
  onUndo?: () => void;
  onCompare?: () => void;
}) {
  const w = useThreadWords();
  const [own, setOwn] = useState<ChangeState>("changed");
  const state = held ?? own;
  const [asking, setAsking] = useState(false);
  const keep = useRef<HTMLButtonElement>(null);
  const n = papers.length;
  const undo = () => {
    setOwn("undone");
    onUndo?.();
  };

  if (state === "undone") {
    return (
      <p className="change-note change-undone">
        <IconUndo size={16} className="change-icon" />
        <span>
          <Say>{w.th("You undid Clauseo's changes to {n} papers.", { n })}</Say>
        </span>
      </p>
    );
  }
  return (
    <div
      className="change-note"
      role="group"
      aria-label={w.th("Clauseo changed {n} papers.", { n })}
    >
      <IconPaperBookPage size={16} className="change-icon" />
      <p className="change-text">
        <span className="change-what">
          <Say>{w.th("Clauseo changed {n} papers.", { n })}</Say>
        </span>
        <span className="change-papers">
          <Say>{papers.join(", ")}</Say>
        </span>
      </p>
      <div className="change-actions">
        <Button kind="quiet" size="small" onClick={onCompare}>
          <IconOpen size={16} />
          <span className="deva-nudge">{w.th("Compare")}</span>
        </Button>
        <Button
          kind="quiet"
          size="small"
          onClick={() => (editedSince === null ? undo() : setAsking(true))}
        >
          <IconUndo size={16} />
          <span className="deva-nudge">{w.t("Undo")}</span>
        </Button>
      </div>
      {editedSince !== null && (
        <AlertDialog open={asking} onOpenChange={setAsking}>
          <AlertDialogPopup
            initialFocus={keep}
            title={w.th("{name} changed after Clauseo's edit.", { name: editedSince })}
            body={w.ui(
              "Undo anyway puts back the version from before Clauseo's edit. The changes made since stay in History.",
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
