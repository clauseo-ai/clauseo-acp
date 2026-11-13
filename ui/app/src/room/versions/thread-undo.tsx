// The thread's Undo for a step with a file that may hold the lawyer's own edits (a file saved during the answer that
// no tool call of Clauseo's wrote, FileChange.may_include_lawyer_edits): Versions' undo dialog, its words composed by
// words.ts's undoAskOf as Versions composes them, each such file and each the host says changed since kept until the
// lawyer says otherwise (state.ts's defaultChoices). In Versions' chunk, which holds those words: the thread
// (thread/stop-rows.tsx) loads it only for such a step.
import { useMemo, useState } from "react";
import { RestoreDialog, type EditedChoice } from "@clauseo/components/clauseo/restore-dialog.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import type { FileChoice, StepEntry } from "../../transport/protocol.ts";
import { useNow } from "../../store/limits.ts";
import { clockOf, localZone } from "../when.ts";
import { defaultChoices } from "./state.ts";
import { calendarOf, isoDayOf } from "./timeline.ts";
import { undoAskOf } from "./words.ts";

export function ThreadUndo({
  step,
  edited,
  onClose,
  onUndo,
}: {
  /** The step the change note stands for. */
  step: StepEntry;
  /** The files the host's PreviewUndo says changed since the step, as it answered the press. */
  edited: ReadonlySet<string>;
  /** Cancel, or the act gone ahead. */
  onClose: () => void;
  /** Undo, with each file's choice. */
  onUndo: (files: { path: string; choice: FileChoice }[]) => void;
}) {
  const { lang } = useDisplay();
  const [choices, setChoices] = useState(() => defaultChoices(step, edited));
  const now = useNow();
  const { cal, v } = useMemo(() => {
    const zone = localZone();
    const moment = {
      day: isoDayOf(step.ts, zone),
      at: clockOf(step.ts, now, zone, "en").time,
      who: { kind: "clauseo", conversation: "" } as const,
    };
    return { cal: calendarOf(now, zone), v: moment };
  }, [step.ts, now]);
  const since = useMemo(() => new Map(), []);
  const ask = undoAskOf(lang, cal, { v, files: step.files, choices, since, edited, failed: null });
  const files = step.files.map((f) => ({ path: f.path, choice: choices.get(f.path) ?? "keep" }));
  // undo_step needs one file to put back: with every file kept, Undo waits, as in Versions
  const held = !files.some((f) => f.choice === "undo");
  return (
    <RestoreDialog
      ask={ask}
      waiting={null}
      held={held}
      onClose={onClose}
      onChoice={(path: string, choice: EditedChoice) =>
        setChoices((now) => new Map(now).set(path, choice))
      }
      onRestore={() => {
        if (held) return;
        onUndo(files);
        onClose();
      }}
      onTryAgain={() => undefined}
      testIds={{ confirm: "dialog-confirm" }}
    />
  );
}
