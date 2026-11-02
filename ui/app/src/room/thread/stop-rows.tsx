// How a turn ended, in the thread: the step (its stop line, and the change note with Undo when it changed papers) and
// the stop at the usage limit with its reset. An Undo refused because a program holds a file shows the lock's lines
// under the note, with Try again in Undo's place (word/word-lock.tsx); a note naming one Word document the wall lists
// offers "Open in Word" while its changes stand (word/open-in-word.ts), and says why in its error line when the open
// fails. Focus follows the lock: to Try again when the lines come, back to Undo (or the note) when they go.
import { lazy, Suspense, useLayoutEffect, useRef, useState } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { ChangeNote } from "@clauseo/components/clauseo/change-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { toHostError } from "../../transport/host-transport.ts";
import { StoppedMark } from "../stopped-mark.tsx";
import { takesMark } from "../thread-break.ts";
import type { EntryId, LimitStopEntry, StepEntry } from "../../transport/protocol.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { isoOf, whenOf } from "./clock.ts";
import { useBusy } from "./live.ts";
import { useThreadActions } from "./session.ts";
import { resetPassed, stopLine } from "./stops.ts";
import { wordLock } from "../word/lock.ts";
import { useWordOpen, type WordOpener } from "../word/open-in-word.ts";

/** The lock's lines, from Versions' chunk, which holds their words: loaded on the first lock. */
const WordLock = lazy(() =>
  late(import("../versions/versions-chunk.ts")).then((m) => ({ default: m.WordLock })),
);

const LOCKED = "matter.file_locked";

const QUIET = "text-small text-quiet";
/** The space above a row under another in its section, on the spacing tokens; first in its section, the thread's own
 * gap between sections stands alone. */
const ROW = "not-first:mt-(--s3)";

function Changed({ entry }: { entry: StepEntry }) {
  const w = useWords();
  const { undo, sentTurn, stream } = useThreadActions();
  const busy = useBusy(stream, sentTurn);
  const plan = useAgentPlan();
  const [refused, setRefused] = useState<string | null>(null);
  // one undo at a time: Undo waits while the host answers the first
  const [pending, setPending] = useState(false);
  const { openerFor } = useWordOpen();
  // a failed Open in Word, by its code, for the note's error line
  const [openFailed, setOpenFailed] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const papers = entry.files.map((f) => f.path);
  const edited = entry.files.find((f) => f.may_include_lawyer_edits)?.path ?? null;
  const undoable = entry.checkpoint_after !== undefined;
  // a program holds a file: the lock's lines stand under the note, and Try again takes Undo's place
  const locked = refused === LOCKED && !entry.undone;
  // the one Word document the note names that the wall lists, while the changes stand
  const openers = entry.undone
    ? []
    : papers.map(openerFor).filter((o): o is WordOpener => o !== null);
  const opener = openers.length === 1 ? openers[0] : undefined;
  const open =
    opener === undefined
      ? undefined
      : {
          label: opener.label,
          onOpen: () => {
            setOpenFailed(null);
            void opener.onOpen().then(setOpenFailed);
          },
        };
  // a refusal again keeps what it said until the host answers: a lock stays a lock, another code replaces it
  const send = () => {
    if (pending) return;
    setPending(true);
    undo(entry)
      .then(
        () => setRefused(null),
        (e: unknown) => setRefused(toHostError(e).code),
      )
      .finally(() => setPending(false));
  };
  const onUndo = () => {
    if (pending) return;
    setRefused(null);
    setOpenFailed(null);
    send();
  };
  // the lines going take Try again with them: focus that was on it, or fell to the page, goes back to Undo, or to
  // the note when Undo isn't back (WordLock takes focus to Try again as the lines come)
  const wasLocked = useRef(locked);
  useLayoutEffect(() => {
    const was = wasLocked.current;
    wasLocked.current = locked;
    const note = box.current;
    if (!was || locked || note === null) return;
    const at = document.activeElement;
    if (at !== null && at !== document.body && !note.contains(at)) return;
    const back =
      note.querySelector<HTMLElement>('[data-testid="undo"]') ??
      note.querySelector<HTMLElement>(".change-note");
    back?.focus();
  }, [locked]);
  const undoRefusal = refused === null || locked ? null : refused;
  const shown = undoRefusal ?? openFailed;
  return (
    <div data-testid="files-changed" ref={box}>
      <ChangeNote
        papers={papers}
        editedSince={edited}
        state={entry.undone ? "undone" : "changed"}
        {...(undoable && !locked ? { onUndo } : {})}
        {...(open === undefined ? {} : { open })}
        undoDisabled={busy || pending}
        error={shown === null ? null : { words: errorWords(w.lang, shown, plan), code: shown }}
        testIds={{ paper: "file-changed", undo: "undo", error: "error", open: "open-in-word" }}
      />
      {locked && (
        <Suspense fallback={null}>
          <WordLock
            file={wordLock(LOCKED, papers)}
            code={LOCKED}
            pending={busy || pending}
            onRetry={send}
            focusOnMount
            testIds={{ block: "word-lock", retry: "word-lock-retry" }}
          />
        </Suspense>
      )}
      {!undoable && (
        <p className={QUIET} data-code="matter.checkpoint_failed">
          <Say>{errorWords(w.lang, "matter.checkpoint_failed")}</Say>
        </p>
      )}
    </div>
  );
}

export function StepRow({ id, entry }: { id: EntryId; entry: StepEntry }) {
  const w = useWords();
  const line = stopLine(entry);
  const plan = useAgentPlan();
  return (
    <div
      data-testid="turn-stopped"
      data-entry-id={id}
      data-stop-reason={entry.stop}
      data-failure={entry.failure}
      className={ROW}
    >
      {takesMark(entry) && <StoppedMark ts={entry.ts} />}
      {line !== null && !takesMark(entry) && (
        <p className={QUIET} data-code={line.code}>
          <Say>
            {line.kind === "error" ? errorWords(w.lang, line.code, plan) : th(w.lang, line.key)}
          </Say>
        </p>
      )}
      {entry.files.length > 0 && <Changed entry={entry} />}
    </div>
  );
}

export function LimitStopRow({ id, entry }: { id: EntryId; entry: LimitStopEntry }) {
  const w = useWords();
  // only a used-up plan window is the plan's limit: a temporary rate limit, or one this version doesn't know, leaves
  // the plan as it was
  const plan = entry.reason === "plan_window";
  // "now" as the row first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const resets = entry.resets_at;
  const when = resets === undefined ? null : whenOf(w.lang, resets, now);
  const passed = resets !== undefined && resetPassed(resets, now);
  return (
    <div
      data-testid="limit-stop"
      data-entry-id={id}
      data-reason={entry.reason}
      role="status"
      className={ROW}
    >
      <p>
        <Say>{plan ? w.t("Claude plan limit reached") : lim(w.lang, "Usage limit reached")}</Say>
      </p>
      {entry.resets_at === undefined && (
        <p className={QUIET}>
          <Say>{lim(w.lang, "Reset not given.")}</Say>
        </p>
      )}
      {resets !== undefined && when !== null && (
        <p className={QUIET}>
          <time data-testid="limit-resets-at" dateTime={isoOf(resets)}>
            <Say>
              {when.day === null
                ? lim(w.lang, passed ? "The limit reset at {time}." : "Resets {time}", {
                    time: when.time,
                  })
                : lim(
                    w.lang,
                    passed ? "The limit reset on {day} at {time}." : "Resets {day} {time}",
                    {
                      day: when.day,
                      time: when.time,
                    },
                  )}
            </Say>
          </time>
        </p>
      )}
    </div>
  );
}
