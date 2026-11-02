// The transcript's steer rows (U7, matters/steer-claude, steer-chatgpt, steer-aside): the lawyer's words taken into the
// running answer, as a mark where they arrived; the quiet line under a steer that came as the answer ended
// (requeued); the line in a turn stopped to send the words first (sent now), in its step's place; and the note under
// an answer whose tool calls a steer moved to the background.
import { useCallback, useMemo, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { SteerMark, SteerNote } from "@clauseo/components/clauseo/steer-marks.tsx";
import { useMatterWords } from "@clauseo/components/lib/words-matters.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type {
  EntryId,
  StreamId,
  Timestamp,
  ToolCallEntry,
  UserMessageEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import { stamp } from "./clock.ts";
import { useQuestionText } from "./live.ts";
import { backgroundOf, steerEntry, steerNotes, type SteerNoteData } from "./steer.ts";

const QUIET = "text-small text-quiet";
const ROW = "not-first:mt-(--s3)";

/** "You, 4:31 pm, while Clauseo answered", and the words, inside the answer. */
export function SteerMarkRow({
  id,
  entry,
  text,
}: {
  id: EntryId;
  entry: UserMessageEntry;
  text: string;
}) {
  const w = useMatterWords();
  // "today" as the row first showed: no clock runs to move it
  const [now] = useState(Date.now);
  return (
    <div className={ROW}>
      <SteerMark
        who={w.m("You, {time}, while Clauseo answered", { time: stamp(w.lang, entry.ts, now) })}
        text={<Say>{text}</Say>}
        testId="steer-mark"
        entryId={id}
      />
    </div>
  );
}

export function LiveSteerMark({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: UserMessageEntry;
}) {
  return <SteerMarkRow id={id} entry={entry} text={useQuestionText(stream, id, entry)} />;
}

/** Under a steer that came as the answer ended: it goes next, as its own question. */
export function RequeuedLine() {
  const w = useMatterWords();
  return (
    <p className={`${ROW} ${QUIET}`} data-testid="steer-outcome" data-outcome="requeued">
      <Say>{w.m("Your words came as the answer ended, so they go next.")}</Say>
    </p>
  );
}

/** In the step of a turn stopped for a steer sent now: when it stopped, and that its answer so far stays. */
export function SentNowLine({ ts }: { ts: Timestamp }) {
  const w = useMatterWords();
  const [now] = useState(Date.now);
  return (
    <p className={QUIET} data-testid="steer-outcome" data-outcome="sent_now">
      <Say>
        {w.m("Stopped at {time} to send your words first. The answer so far is kept.", {
          time: stamp(w.lang, ts, now),
        })}
      </Say>
    </p>
  );
}

/** The notes under an answer, one per steer. */
export function SteerNotes({ notes }: { notes: readonly SteerNoteData[] }) {
  const w = useMatterWords();
  const [now] = useState(Date.now);
  return notes.map((n) => (
    <div key={n.steer} className={ROW}>
      <SteerNote
        testId="steer-note"
        text={w.m(
          "Your words at {time} came in during '{title}'. It carried on in the background, finished at {done}, and this answer uses what it found.",
          { time: stamp(w.lang, n.at, now), title: n.title, done: stamp(w.lang, n.done, now) },
        )}
        found={n.found.map((f) => ({ at: stamp(w.lang, f.at, now), what: f.what }))}
      />
    </div>
  ));
}

const NO_NOTES: readonly SteerNoteData[] = [];
const TOOL = "tool_call:";

const askedAtIn = (entries: ReadonlyMap<EntryId, ViewEntry>) => (steer: string) =>
  entries.get(steerEntry(steer))?.user_message?.ts;

/** The notes of a live answer: read from its tool calls on each flush, as one string, so a flush that leaves every
 * background mark the same renders nothing. */
export function LiveSteerNotes({ ids, stream }: { ids: readonly EntryId[]; stream: StreamId }) {
  const calls = useMemo(() => ids.filter((id) => id.startsWith(TOOL)), [ids]);
  const pick = useCallback(
    (v: StreamView | undefined) => {
      if (v === undefined) return "";
      let sig = "";
      for (const id of calls) {
        const call = v.entries.get(id)?.tool_call;
        const mark = call === undefined ? undefined : backgroundOf(call);
        if (mark === undefined) continue;
        const at = v.entries.get(steerEntry(mark.steer))?.user_message?.ts ?? "";
        sig += `${id}\u0000${mark.steer}\u0000${at}\u0000${mark.finished_at ?? ""}\u0000${call!.title}\u0001`;
      }
      return sig;
    },
    [calls],
  );
  const store = useViewStore();
  const sig = useStreamPart(store, stream, pick);
  const notes = useMemo(() => {
    const v = sig === "" ? undefined : store.view(stream);
    if (v === undefined) return NO_NOTES;
    const own = calls
      .map((id) => v.entries.get(id)?.tool_call)
      .filter((c): c is ToolCallEntry => c !== undefined);
    return steerNotes(own, askedAtIn(v.entries));
  }, [sig, calls, store, stream]);
  return notes.length === 0 ? null : <SteerNotes notes={notes} />;
}

/** The notes of an older answer, from its page's entries. */
export function PagedSteerNotes({ entries }: { entries: readonly ViewEntry[] }) {
  const notes = useMemo(() => {
    const byId = new Map<EntryId, ViewEntry>();
    const calls: ToolCallEntry[] = [];
    for (const e of entries) {
      if (e.user_message !== undefined) byId.set(steerEntry(e.user_message.message), e);
      if (e.tool_call !== undefined) calls.push(e.tool_call);
    }
    return steerNotes(calls, askedAtIn(byId));
  }, [entries]);
  return notes.length === 0 ? null : <SteerNotes notes={notes} />;
}
