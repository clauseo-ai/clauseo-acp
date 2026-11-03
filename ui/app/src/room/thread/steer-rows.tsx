// The transcript's steer rows (U7, matters/steer-claude, steer-chatgpt, steer-aside): the lawyer's words taken into the
// running answer, as a mark where they arrived; the quiet line under a steer that came as the answer ended
// (requeued); the line in a turn stopped to send the words first (sent now), in its step's place; and the note under
// an answer whose tool calls a steer moved to the background.
import { useMemo, useState } from "react";
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
import { steerEntry, steerNotes, type SteerNoteData } from "./steer.ts";

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
          "Your words at {time} came in before this had finished: {title}. It carried on in the background, finished at {done}, and the answer uses what it found.",
          {
            time: stamp(w.lang, n.at, now),
            title: n.title.replace(TRAILING, ""),
            done: stamp(w.lang, n.done, now),
          },
        )}
        found={n.found.map((f) => ({ at: stamp(w.lang, f.at, now), what: f.what }))}
      />
    </div>
  ));
}

/** A title's own closing stop: the line puts its own after it. */
const TRAILING = /[\s.…।]+$/u;

const NO_NOTES: readonly SteerNoteData[] = [];
const TOOL = "tool_call:";
const USER = "user_message:";

/** A pick of an answer's notes, kept by its entries' identity: one per id list (SteerNotesOf). */
function notesPick(
  ids: readonly EntryId[],
  paged: ReadonlyMap<EntryId, ViewEntry>,
): (v: StreamView | undefined) => readonly SteerNoteData[] {
  const own = ids.filter((id) => id.startsWith(TOOL) || id.startsWith(USER));
  let seen: (ViewEntry | undefined)[] = [];
  let last = NO_NOTES;
  let key = "";
  const entryOf = (v: StreamView | undefined, id: EntryId) => paged.get(id) ?? v?.entries.get(id);
  return (v: StreamView | undefined) => {
    let same = seen.length === own.length;
    for (let i = 0; same && i < own.length; i += 1) same = entryOf(v, own[i]!) === seen[i];
    if (same) return last;
    seen = own.map((id) => entryOf(v, id));
    const byId = new Map<EntryId, ViewEntry>();
    const calls: ToolCallEntry[] = [];
    seen.forEach((e, i) => {
      if (e?.tool_call !== undefined) calls.push(e.tool_call);
      else if (e !== undefined) byId.set(own[i]!, e);
    });
    const next = steerNotes(calls, (steer) => byId.get(steerEntry(steer))?.user_message?.ts);
    const nextKey = JSON.stringify(next);
    if (nextKey === key) return last;
    key = nextKey;
    return (last = next.length === 0 ? NO_NOTES : next);
  };
}

/** The notes of an answer, from its entries in the window or on an older page (`paged`). The list mounts it only in an
 * exchange that holds a steer's mark, where a background mark can come (a steer moves a call to the background only
 * when the agent takes it in). Each flush compares the answer's tool call and message entries by identity, and reads
 * them again only when one is new. */
export function SteerNotesOf({
  ids,
  stream,
  paged,
}: {
  ids: readonly EntryId[];
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
}) {
  const pick = useMemo(() => notesPick(ids, paged), [ids, paged]);
  const notes = useStreamPart(useViewStore(), stream, pick);
  return notes.length === 0 ? null : <SteerNotes notes={notes} />;
}
