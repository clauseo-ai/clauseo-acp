// How a turn ended, in the thread: the step (its stop line, and the change note with Undo when it changed papers) and
// the stop at the usage limit with its reset.
import { useState } from "react";
import { ChangeNote } from "@clauseo/components/clauseo/change-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { lim } from "@clauseo/components/lib/words-limits.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { toHostError } from "../../transport/host-transport.ts";
import type { EntryId, LimitStopEntry, StepEntry } from "../../transport/protocol.ts";
import { isoOf, whenOf } from "./clock.ts";
import { useBusy } from "./live.ts";
import { useThreadActions } from "./session.ts";
import { stopLine } from "./stops.ts";

const QUIET = "text-small text-quiet";
/** The space above a row under the answer's words, on the spacing tokens. */
const ROW = "mt-(--s3)";

function Changed({ entry }: { entry: StepEntry }) {
  const w = useWords();
  const { undo, sentTurn, stream } = useThreadActions();
  const busy = useBusy(stream, sentTurn);
  const [refused, setRefused] = useState<string | null>(null);
  // one undo at a time: Undo waits while the host answers the first
  const [pending, setPending] = useState(false);
  const edited = entry.files.find((f) => f.may_include_lawyer_edits)?.path ?? null;
  const undoable = entry.checkpoint_after !== undefined;
  const onUndo = () => {
    if (pending) return;
    setPending(true);
    setRefused(null);
    undo(entry)
      .catch((e: unknown) => setRefused(toHostError(e).code))
      .finally(() => setPending(false));
  };
  return (
    <div data-testid="files-changed">
      <ChangeNote
        papers={entry.files.map((f) => f.path)}
        editedSince={edited}
        state={entry.undone ? "undone" : "changed"}
        {...(undoable ? { onUndo } : {})}
        undoDisabled={busy || pending}
        error={refused === null ? null : { words: errorWords(w.lang, refused), code: refused }}
        testIds={{ paper: "file-changed", undo: "undo", error: "error" }}
      />
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
  return (
    <div
      data-testid="turn-stopped"
      data-entry-id={id}
      data-stop-reason={entry.stop}
      className={ROW}
    >
      {line !== null && (
        <p className={QUIET} data-code={line.code}>
          <Say>{line.kind === "error" ? errorWords(w.lang, line.code) : th(w.lang, line.key)}</Say>
        </p>
      )}
      {entry.files.length > 0 && <Changed entry={entry} />}
    </div>
  );
}

export function LimitStopRow({ id, entry }: { id: EntryId; entry: LimitStopEntry }) {
  const w = useWords();
  const usage = entry.reason === "workspace_credits" || entry.reason === "workspace_spend_cap";
  const resets = entry.resets_at;
  // "today" as the row first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const when = resets === undefined ? null : whenOf(w.lang, resets, now);
  return (
    <div
      data-testid="limit-stop"
      data-entry-id={id}
      data-reason={entry.reason}
      role="status"
      className={ROW}
    >
      <p>
        <Say>{usage ? lim(w.lang, "Usage limit reached") : w.t("Claude plan limit reached")}</Say>
      </p>
      <p className={QUIET}>
        {resets === undefined || when === null ? (
          <Say>{lim(w.lang, "Limit reached. Reset not given.")}</Say>
        ) : (
          <time data-testid="limit-resets-at" dateTime={isoOf(resets)}>
            <Say>
              {when.day === null
                ? lim(w.lang, "Resets {time}", { time: when.time })
                : lim(w.lang, "Resets {day} {time}", { day: when.day, time: when.time })}
            </Say>
          </time>
        )}
      </p>
    </div>
  );
}
