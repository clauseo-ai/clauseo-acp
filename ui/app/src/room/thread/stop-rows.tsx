// How a turn ended, in the thread: the step (its stop line, and the change note with Undo when it changed papers) and
// the stop at the usage limit, its mark and its note with the reset (U12). An Undo refused because a program holds a
// file shows the lock's lines under the note, with Try again in Undo's place (word/word-lock.tsx); a note naming one
// Word document the wall lists offers "Open in Word" while its changes stand (word/open-in-word.ts), and says why in
// its error line when the open fails, with Show in folder beside it for a path too long for Word, or in a quiet line
// under the note when Word opened the file in Protected View (until the next open or Undo). Focus follows the lock: to Try again when the lines come, back to Undo (or the
// note) when they go.
import { lazy, Suspense, useLayoutEffect, useRef, useState } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { ChangeNote } from "@clauseo/components/clauseo/change-note.tsx";
import { NoticeCard, StopMark } from "@clauseo/components/clauseo/limit-note.tsx";
import { IconChecking } from "@clauseo/components/icons/icons.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { limThread } from "@clauseo/components/lib/words-limits-thread.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { revealKey, ui } from "@clauseo/components/lib/words-ui.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useNow } from "../../store/limits.ts";
import { toHostError } from "../../transport/host-transport.ts";
import { StoppedMark } from "../stopped-mark.tsx";
import { takesMark } from "../thread-break.ts";
import type {
  EntryId,
  HostError,
  LimitStopEntry,
  StepEntry,
  Timestamp,
} from "../../transport/protocol.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { isoOf, whenOf } from "./clock.ts";
import { planNameOf, stopTimeOf } from "./limit-words.ts";
import { windowLine } from "./stop-words.ts";
import { useBusy } from "./live.ts";
import { SentNowLine } from "./steer-rows.tsx";
import { useThreadActions } from "./session.ts";
import { resetPassed, stopLine } from "./stops.ts";
import { lockedFiles, wordLocks } from "../word/lock.ts";
import { useWordOpen, type WordOpener } from "../word/open-in-word.ts";

/** The lock's lines, from Versions' chunk, which holds their words: loaded on the first lock. */
const WordLock = lazy(() =>
  late(import("../versions/versions-chunk.ts")).then((m) => ({ default: m.WordLock })),
);

const LOCKED = "matter.file_locked";

const QUIET = "text-small text-quiet";
/** A matter path's file name: its last part, as the wall shows the file. */
const fileNameOf = (path: string): string => path.split(/[\\/]/).pop() || path;

/** The space above a row under another in its section, on the spacing tokens; first in its section, the thread's own
 * gap between sections stands alone. */
const ROW = "not-first:mt-(--s3)";

function Changed({ entry }: { entry: StepEntry }) {
  const w = useWords();
  const { platform } = useDisplay();
  const { undo, previewUndo, sentTurn, stream } = useThreadActions();
  const busy = useBusy(stream, sentTurn);
  const plan = useAgentPlan();
  // the refusal as the host sent it: its code, and the files it names as held
  const [refused, setRefused] = useState<HostError | null>(null);
  // one undo at a time: Undo waits while the host answers the first
  const [pending, setPending] = useState(false);
  const { openerFor } = useWordOpen();
  // a failed Open in Word, by its code, for the note's error line
  const [openFailed, setOpenFailed] = useState<string | null>(null);
  // the last open went through in Protected View: a quiet line says so until the next open or Undo
  const [protectedView, setProtectedView] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const papers = entry.files.map((f) => f.path);
  const undoable = entry.checkpoint_after !== undefined;
  // a program holds a file: the lock's lines stand under the note, and Try again takes Undo's place
  const locked = refused?.code === LOCKED && !entry.undone;
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
            setProtectedView(false);
            void opener.onOpen().then((opened) => {
              setOpenFailed(opened.refused);
              setProtectedView(opened.protectedView);
            });
          },
        };
  // a refusal again keeps what it said until the host answers: a lock stays a lock, another code replaces it
  const send = () => {
    if (pending) return;
    setPending(true);
    undo(entry)
      .then(
        () => setRefused(null),
        (e: unknown) => setRefused(toHostError(e)),
      )
      .finally(() => setPending(false));
  };
  const onUndo = () => {
    if (pending) return;
    setRefused(null);
    setOpenFailed(null);
    setProtectedView(false);
    send();
  };
  // Undo asks about a paper the lawyer edited since Clauseo's change: PreviewUndo says which, on each press, and Undo
  // waits for it. Where the query fails (an older host, or any error) the flag a file carries decides, as before: it
  // may ask when nothing changed, and never loses a warning.
  const askUndo = (): Promise<string | null> => {
    const flagged = entry.files.find((f) => f.may_include_lawyer_edits)?.path ?? null;
    if (previewUndo === undefined) return Promise.resolve(flagged);
    return previewUndo(entry).then(
      (edited) => (edited[0] === undefined ? null : fileNameOf(edited[0])),
      () => flagged,
    );
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
  const undoRefusal = refused === null || locked ? null : refused.code;
  const shown = undoRefusal ?? openFailed;
  return (
    <div data-testid="files-changed" ref={box}>
      <ChangeNote
        papers={papers}
        askUndo={askUndo}
        state={entry.undone ? "undone" : "changed"}
        {...(undoable && !locked ? { onUndo } : {})}
        {...(open === undefined ? {} : { open })}
        undoDisabled={busy || pending}
        error={shown === null ? null : { words: errorWords(w.lang, shown, plan), code: shown }}
        errorAction={
          shown === "matter.document_path_too_long" && opener !== undefined
            ? { label: ui(w.lang, revealKey(platform)), onAction: opener.onReveal }
            : undefined
        }
        testIds={{
          paper: "file-changed",
          undo: "undo",
          error: "error",
          errorAction: "reveal",
          open: "open-in-word",
        }}
      />
      {protectedView && opener !== undefined && shown === null && (
        <p className={QUIET} role="status" data-testid="open-protected-view">
          <Say>{opener.protectedLine}</Say>
        </p>
      )}
      {locked && (
        <Suspense fallback={null}>
          <WordLock
            files={wordLocks(LOCKED, lockedFiles(refused ?? { code: LOCKED }, papers))}
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

/** `sentNow`: a steer sent now stopped this turn, so its line says that, in place of the stop's own line or mark. */
export function StepRow({
  id,
  entry,
  sentNow = false,
}: {
  id: EntryId;
  entry: StepEntry;
  sentNow?: boolean | undefined;
}) {
  const w = useWords();
  const line = sentNow ? null : stopLine(entry);
  const plan = useAgentPlan();
  return (
    <div
      data-testid="turn-stopped"
      data-entry-id={id}
      data-stop-reason={entry.stop}
      data-failure={entry.failure}
      className={ROW}
    >
      {sentNow && <SentNowLine ts={entry.ts} />}
      {!sentNow && takesMark(entry) && <StoppedMark ts={entry.ts} />}
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

/** The reset as the stop's line says it on its own: "Resets 6:10 pm", its day form, or, once gone by, when it reset. */
function resetWords(lang: Lang, resets: Timestamp, now: number): string {
  const { day, time } = whenOf(lang, resets, now);
  const passed = resetPassed(resets, now);
  return day === null
    ? lim(lang, passed ? "The limit reset at {time}." : "Resets {time}", { time })
    : lim(lang, passed ? "The limit reset on {day} at {time}." : "Resets {day} {time}", {
        day,
        time,
      });
}

/** The stop at the usage limit (U12, the design's limits/hit, weekly and unknown, without Finished, Left or the ways
 * on): a plan window's stop marks where the answer stopped, "Stopped here at 4:52 pm", and its note says the plan's
 * limit was reached at that time, with the window's line, "The 5-hour limit. It resets at 6:10 pm."; without the
 * stop's time the title stands alone and no mark shows. A workspace's credits or spend cap, a temporary rate limit and
 * a reason this version doesn't know leave the plan as it was: the note says the usage limit, and the reset. Concrete,
 * never red, and no clock runs: "now" is the clock at each render, so a later render names the day it renders on. */
export function LimitStopRow({ id, entry }: { id: EntryId; entry: LimitStopEntry }) {
  const w = useWords();
  const agent = useAgentPlan();
  const plan = entry.reason === "plan_window";
  const now = useNow();
  const resets = entry.resets_at;
  const stopped = plan ? stopTimeOf(entry) : null;
  const name = planNameOf(agent);
  const at = stopped === null ? null : whenOf(w.lang, stopped, now);
  const title = !plan
    ? lim(w.lang, "Usage limit reached")
    : at === null
      ? limThread(w.lang, "{name} plan limit reached", { name })
      : at.day === null
        ? limThread(w.lang, "{name} plan limit reached at {time}", { name, time: at.time })
        : limThread(w.lang, "{name} plan limit reached on {day} at {time}", {
            name,
            day: at.day,
            time: at.time,
          });
  const line =
    resets === undefined
      ? lim(w.lang, "Reset not given.")
      : ((plan ? windowLine(w.lang, entry.window, resets, now) : null) ??
        resetWords(w.lang, resets, now));
  return (
    <div
      data-testid="limit-stop"
      data-entry-id={id}
      data-reason={entry.reason}
      role="status"
      className={ROW}
    >
      {at !== null && (
        <p className="lim-stop-line" data-testid="limit-stopped-here">
          <StopMark>
            {at.day === null
              ? limThread(w.lang, "Stopped here at {time}", { time: at.time })
              : limThread(w.lang, "Stopped here on {day} at {time}", {
                  day: at.day,
                  time: at.time,
                })}
          </StopMark>
        </p>
      )}
      <NoticeCard
        icon={<IconChecking size={18} />}
        label={title}
        title={title}
        sub={
          resets === undefined ? (
            line
          ) : (
            <time data-testid="limit-resets-at" dateTime={isoOf(resets)}>
              <Say>{line}</Say>
            </time>
          )
        }
      />
    </div>
  );
}
