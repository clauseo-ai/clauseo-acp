// The transcript's steer rows (U7, matters/steer-claude, steer-chatgpt, steer-aside): the lawyer's words taken into the
// running answer, as a mark where they arrived, once the agent took them in (`injected`) and only then; a steer not
// yet resolved, in the same place, as its words and "Sends when Claude finishes" with Send now, which sends
// send_queued_now with the turn it was sent into (the host stops that turn, if it still runs, and sends the words next;
// `thread.turn_not_queued`, they had already moved, says so as the queue's strip does); the quiet line under a steer
// that waited for the answer to end (requeued); the line in a turn stopped to send the words first (sent now), in its
// step's place; and the note under an answer whose tool calls a steer moved to the background.
import { use, useMemo, useRef, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { SteerMark, SteerNote, SteerWaiting } from "@clauseo/components/clauseo/steer-marks.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { useMatterWords } from "@clauseo/components/lib/words-matters.ts";
import { toHostError } from "../../transport/host-transport.ts";
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
import { useAgentPlan } from "./agent-plan.ts";
import { stamp } from "./clock.ts";
import { planNameOf } from "./limit-words.ts";
import { useQuestionText } from "./live.ts";
import { ThreadActionsContext } from "./session.ts";
import { steerEntry, steerNotes, type SteerNoteData } from "./steer.ts";
import { overTitle } from "./tool-words.ts";

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

const GONE = "thread.turn_not_queued";

/** A steer the agent hasn't taken yet, live or on an older page: its words, that they send when the agent finishes,
 * and Send now (send_queued_now with the turn it was sent into), one press at a time. A refusal's words stand in the
 * row while it waits; the view moves it once the host has (into a question, or the mark). Without the session's
 * actions (a part mounted outside the room) no Send now. */
export function SteerWaitingRow({
  id,
  entry,
  text,
}: {
  id: EntryId;
  entry: UserMessageEntry;
  text: string;
}) {
  const w = useMatterWords();
  const plan = useAgentPlan();
  const store = useViewStore();
  const send = use(ThreadActionsContext)?.sendSteerNow;
  const going = useRef(false);
  const [refused, setRefused] = useState<string | null>(null);
  const onSendNow = () => {
    if (send === undefined || going.current) return;
    going.current = true;
    if (refused !== null) setRefused(null);
    send(entry.turn).then(
      () => {
        going.current = false;
      },
      (e: unknown) => {
        going.current = false;
        const code = toHostError(e).code;
        // with the next frame's flush, as the strip's refusals land (#253)
        store.inFrame(() => setRefused(code));
      },
    );
  };
  const note =
    refused === null
      ? null
      : {
          code: refused,
          words:
            refused === GONE
              ? w.m("That message had already gone.")
              : errorWords(w.lang, refused, plan),
        };
  return (
    <div className={ROW}>
      <SteerWaiting
        text={<Say>{text}</Say>}
        when={w.m("Sends when {name} finishes", { name: planNameOf(plan) })}
        onSendNow={send === undefined ? undefined : onSendNow}
        note={note}
        entryId={id}
        testIds={{
          row: "steer-waiting",
          sendNow: "steer-waiting-send-now",
          note: "steer-waiting-note",
        }}
      />
    </div>
  );
}

export function LiveSteerWaiting({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: UserMessageEntry;
}) {
  return <SteerWaitingRow id={id} entry={entry} text={useQuestionText(stream, id, entry)} />;
}

/** Under a steer that waited for the answer to end (the agent didn't take it in, or the answer it joined was stopped or
 * failed): it went next, as its own question. */
export function RequeuedLine() {
  const w = useMatterWords();
  return (
    <p className={`${ROW} ${QUIET}`} data-testid="steer-outcome" data-outcome="requeued">
      <Say>{w.m("Your words waited for the answer to end, so they went next.")}</Say>
    </p>
  );
}

/** In the step of a turn stopped for a steer sent now: when it stopped, and that its answer so far stays. The design's
 * stop line type (matters.css's `text-small-row text-muted`, as .steer-note-text). */
export function SentNowLine({ ts }: { ts: Timestamp }) {
  const w = useMatterWords();
  const [now] = useState(Date.now);
  return (
    <p className="text-small-row text-muted" data-testid="steer-outcome" data-outcome="sent_now">
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
          "Your words at {time} came in before some work had finished. It went on in the background and finished at {done}. The answer uses what it found.",
          { time: stamp(w.lang, n.at, now), done: stamp(w.lang, n.done, now) },
        )}
        found={n.found.map((f) => ({
          at: stamp(w.lang, f.at, now),
          // a call titled with a tool's name says what it did instead (tool-names.ts)
          what: overTitle(w.lang, f.what, false),
        }))}
      />
    </div>
  ));
}

const NO_NOTES: readonly SteerNoteData[] = [];
const TOOL = "tool_call:";
const USER = "user_message:";
const STEP = "step:";

/** A pick of an answer's notes, kept by its entries' identity: one per id list (SteerNotesOf). */
function notesPick(
  ids: readonly EntryId[],
  paged: ReadonlyMap<EntryId, ViewEntry>,
): (v: StreamView | undefined) => readonly SteerNoteData[] {
  const own = ids.filter((id) => id.startsWith(TOOL) || id.startsWith(USER) || id.startsWith(STEP));
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
    let endedAt: Timestamp | undefined;
    seen.forEach((e, i) => {
      if (e?.tool_call !== undefined) calls.push(e.tool_call);
      else if (e?.step !== undefined) endedAt = e.step.ts;
      else if (e !== undefined) byId.set(own[i]!, e);
    });
    const next = steerNotes(
      calls,
      (steer) => byId.get(steerEntry(steer))?.user_message?.ts,
      endedAt,
    );
    const nextKey = JSON.stringify(next);
    if (nextKey === key) return last;
    key = nextKey;
    return (last = next.length === 0 ? NO_NOTES : next);
  };
}

/** The notes of an answer, from its entries in the window or on an older page (`paged`), once its step is in. The list mounts it only in an
 * exchange that holds a steer's mark, where a background mark can come (a steer moves a call to the background only
 * when the agent takes it in). Each flush compares the answer's tool call, message and step entries by identity, and reads
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
