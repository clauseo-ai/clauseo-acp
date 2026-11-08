// The change note. After Clauseo changes papers, the thread says so in one line, with the papers named and two ways
// on: Compare, and Undo, each when the caller gives it, and a way to open a paper where the caller gives one ("Open in
// Word"). A refused undo, or a failed open, says why in a quiet line beside Undo, with the way on the caller gives for
// it (Show in folder, for a path too long for Word). The note itself takes focus (not by
// Tab) when the control under it that held focus goes. When a paper was edited after Clauseo's change, Undo asks
// first, in the alert dialog, with Keep current as the safe choice: at once when the paper is given (editedSince), or,
// given askUndo, once the caller has answered the press (Undo waits meanwhile).
import { Fragment, lazy, Suspense, useState } from "react";
import { late } from "../lib/late.ts";
import { Button } from "../ui/button.tsx";
import { IconCompare, IconOpen, IconPaperBookPage, IconUndo } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { Say } from "./say.tsx";

const loadAsk = () => late(import("./change-note-ask.tsx"));
/** The dialog Undo opens when a paper changed since: its own chunk, as most notes never ask. */
const UndoAsk = lazy(() => late(loadAsk().then((m) => ({ default: m.UndoAsk }))));

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
  askUndo,
  state: held,
  onUndo,
  onCompare,
  open,
  undoDisabled = false,
  error = null,
  errorAction,
  testIds,
}: {
  /** The papers Clauseo changed, by their names on the wall. */
  papers: readonly string[];
  /** A paper edited after Clauseo's change, if one was: Undo asks first. */
  editedSince?: string | null;
  /** Asked on each press of Undo, before anything is undone, and Undo waits while it answers: the paper edited since,
   * which the dialog asks about, or null to undo straight away. Given, it takes the place of editedSince. */
  askUndo?: () => Promise<string | null>;
  /** Given, the host says when the changes are undone; without it, Undo marks them undone itself. */
  state?: ChangeState;
  /** Undo shows only when given. */
  onUndo?: () => void;
  /** Compare shows only when given. */
  onCompare?: () => void;
  /** Opens a paper the note names, in the program the label says: shows only when given, while the changes stand. */
  open?: { label: string; onOpen: () => void };
  /** Undo can't be pressed for now, such as while Clauseo answers. */
  undoDisabled?: boolean;
  /** A refused undo: its words, in the language, and its code, as a quiet line beside Undo. */
  error?: { words: string; code: string } | null;
  /** A press beside the error line, in the words the caller gives: shows only with the error. */
  errorAction?: { label: string; onAction: () => void } | undefined;
  testIds?: {
    paper?: string;
    undo?: string;
    error?: string;
    errorAction?: string;
    open?: string;
  };
}) {
  const w = useThreadWords();
  const counts = useCounts(papers.length);
  const [own, setOwn] = useState<ChangeState>("changed");
  const state = held ?? own;
  const [asking, setAsking] = useState(false);
  // the dialog's chunk loads when Undo is first in reach, and the dialog mounts the first time it is asked for
  const [asked, setAsked] = useState(false);
  // the paper the dialog asks about, as askUndo answers it
  const [about, setAbout] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const askAbout = askUndo === undefined ? editedSince : about;
  const reach = askAbout === null && askUndo === undefined ? undefined : () => void loadAsk();
  const undo = () => {
    if (held === undefined) setOwn("undone");
    onUndo?.();
  };
  const press = () => {
    if (previewing) return;
    if (askUndo === undefined) {
      if (editedSince === null) return undo();
      setAsked(true);
      setAsking(true);
      return;
    }
    setPreviewing(true);
    void askUndo().then((paper) => {
      setPreviewing(false);
      if (paper === null) return undo();
      setAbout(paper);
      setAsked(true);
      setAsking(true);
    });
  };

  if (state === "undone") {
    return (
      <p className="change-note change-undone" tabIndex={-1}>
        <IconUndo size={16} className="change-icon" />
        <span>
          <Say>{counts.undone}</Say>
        </span>
      </p>
    );
  }
  return (
    // tabIndex -1: where focus goes back to when the control the lawyer pressed under the note goes away
    <div className="change-note" role="group" aria-label={counts.changed} tabIndex={-1}>
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
        {error !== null && errorAction !== undefined && (
          <Button
            kind="quiet"
            size="small"
            onClick={errorAction.onAction}
            data-testid={testIds?.errorAction}
          >
            <span className="deva-nudge">{errorAction.label}</span>
          </Button>
        )}
        {onCompare !== undefined && (
          <Button kind="quiet" size="small" onClick={onCompare}>
            <IconCompare size={16} />
            <span className="deva-nudge">{w.th("Compare")}</span>
          </Button>
        )}
        {open !== undefined && (
          <Button kind="quiet" size="small" onClick={open.onOpen} data-testid={testIds?.open}>
            <IconOpen size={16} />
            <span className="deva-nudge">{open.label}</span>
          </Button>
        )}
        {onUndo !== undefined && (
          <Button
            kind="quiet"
            size="small"
            disabled={undoDisabled || previewing}
            focusableWhenDisabled
            data-testid={testIds?.undo}
            onPointerEnter={reach}
            onFocus={reach}
            onClick={press}
          >
            <IconUndo size={16} />
            <span className="deva-nudge">{w.t("Undo")}</span>
          </Button>
        )}
      </div>
      {askAbout !== null && onUndo !== undefined && asked && (
        <Suspense fallback={null}>
          <UndoAsk name={askAbout} open={asking} onOpenChange={setAsking} onUndo={undo} />
        </Suspense>
      )}
    </div>
  );
}
