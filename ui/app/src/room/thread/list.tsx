// The open thread, in its own chunk with the Markdown renderer it shows answers through (chunk.ts): Thread's log and
// scroller, the older pages above, and the exchanges, grouped by turn (exchanges.ts, rebuilt only as the id list or a
// user message's place changes), each row reading its own entry (rows.tsx) and each run of tool calls and thoughts in
// an answer folded into a step group (step-groups.tsx), so while an answer streams one frame commits once and renders
// only the row whose text grew.
// Nothing here is needed for the room's first paint: Work, the session and the composer are in the room's own chunk.
import { memo, useCallback, useMemo, useState, type ReactNode } from "react";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Thread } from "@clauseo/components/clauseo/thread.tsx";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, StreamId, Timestamp, ViewEntry } from "../../transport/protocol.ts";
import { AnswerMeta } from "./answer-meta.tsx";
import { stamp } from "./clock.ts";
import { exchangesPick, latestAsked } from "./exchanges.ts";
import { sameIds } from "./ids.ts";
import { useHasOlder, useQuestionWords } from "./live.ts";
import { useOlder } from "./older.ts";
import { useTimeOf } from "./time-of.ts";
import { RowOf } from "./row-of.tsx";
import { SteerNotesOf } from "./steer-rows.tsx";
import { LiveAnswerRows } from "./step-groups.tsx";

/** The composer's limit warning, carried in this chunk with the stop's note (limits/limit-warning.tsx loads it). */
export { WarningNote } from "./limit-warning-note.tsx";

const STEP = "step:";

/** One exchange: the question's section and the answer's, each named with its time so two exchanges' sections never
 * share a name (axe's landmark-unique): the question's `ts`, and the answer's step's `ts`, the time it ended. A
 * running answer has no step yet and keeps the plain name ("Clauseo's answer"): only one answer runs at a time. The
 * answer's meta line shows the same time. The room's named nodes for the question and the answer (the thread's scroll
 * and its pinned question measure them) go on the latest exchange only. Its rows are its props: a time coming in
 * renders the sections, not the rows. Two exchanges in one minute would share the names: a section whose name is the
 * exchange before's (its question's and step's ids, `beforeAsked` and `beforeStep`; times only grow, so a repeat is always that one) goes
 * unnamed, and so is no landmark (an unnamed section is no region). */
function Sections({
  question,
  answer,
  asked,
  step,
  stream,
  paged,
  latest,
  beforeAsked,
  beforeStep,
}: {
  question: ReactNode | null;
  answer: ReactNode;
  /** The question's id and its step's, for their times. */
  asked: EntryId | undefined;
  step: EntryId | undefined;
  /** The exchange before's, the same; none for the first. */
  beforeAsked: EntryId | undefined;
  beforeStep: EntryId | undefined;
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
  latest: boolean;
}) {
  const w = useWords();
  const { nodes } = useRoom();
  // "today" as the sections first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const askedAt = useTimeOf(asked, stream, paged);
  const endedAt = useTimeOf(step, stream, paged);
  const at = (ts: Timestamp | null) => (ts === null ? null : stamp(w.lang, ts, now));
  const askedTime = at(askedAt);
  const endedTime = at(endedAt);
  const askedName =
    askedTime === null
      ? th(w.lang, "Your question")
      : th(w.lang, "Your question, {time}", { time: askedTime });
  const endedName =
    endedTime === null
      ? th(w.lang, "Clauseo's answer")
      : th(w.lang, "Clauseo's answer, {time}", { time: endedTime });
  // the exchange before's times: a name it has already is left off
  const askedBefore = at(useTimeOf(beforeAsked, stream, paged));
  const endedBefore = at(useTimeOf(beforeStep, stream, paged));
  const askedRepeats = askedTime !== null && askedBefore === askedTime;
  const endedRepeats = endedTime !== null && endedBefore === endedTime;
  return (
    <>
      {question !== null && (
        <section
          className="step step-question"
          ref={latest ? nodes.slot("questionStep") : undefined}
          aria-label={askedRepeats ? undefined : askedName}
        >
          {question}
        </section>
      )}
      {(answer !== null || (latest && question !== null)) && (
        <section
          className="step step-answer"
          ref={latest ? nodes.slot("answerStep") : undefined}
          aria-label={endedRepeats ? undefined : endedName}
        >
          {/* the design's meta line opens each answer: "Clauseo", and its turn's time once the step is in. The
              answer's rows put it in themselves, with its first step group on it as the chip (step-groups.tsx) */}
          {answer === null && (
            <AnswerMeta
              step={step}
              asked={asked}
              before={beforeStep}
              stream={stream}
              paged={paged}
              latest={latest}
            />
          )}
          {answer}
        </section>
      )}
    </>
  );
}

const USER = "user_message:";
const TOOL = "tool_call:";

/** One exchange, its rows live or from an older page: a turn can start on a page and go on in the window. */
const LiveExchange = memo(
  function LiveExchange({
    ids,
    asked,
    sentNow,
    stream,
    paged,
    latest,
    beforeAsked,
    beforeStep,
  }: {
    ids: readonly EntryId[];
    asked: boolean;
    sentNow: EntryId | null;
    stream: StreamId;
    paged: ReadonlyMap<EntryId, ViewEntry>;
    latest: boolean;
    /** The exchange before's question and step, for the names it has taken (Sections). */
    beforeAsked: EntryId | undefined;
    beforeStep: EntryId | undefined;
  }) {
    const rest = asked ? ids.slice(1) : ids;
    const step = rest.find((id) => id.startsWith(STEP));
    const answer =
      rest.length === 0 ? null : (
        <LiveAnswerRows
          ids={rest}
          step={step}
          asked={asked ? ids[0] : undefined}
          before={beforeStep}
          stream={stream}
          paged={paged}
          sentNow={sentNow}
          latest={latest}
        />
      );
    // a steer's mark in the answer (a user message there) is where a tool call can carry on in the background
    const notes = rest.some((id) => id.startsWith(USER)) && rest.some((id) => id.startsWith(TOOL));
    return (
      <Sections
        question={
          asked ? <RowOf id={ids[0]!} stream={stream} paged={paged} sentNow={false} /> : null
        }
        answer={
          answer !== null && notes ? (
            <>
              {answer}
              <SteerNotesOf ids={rest} stream={stream} paged={paged} />
            </>
          ) : (
            answer
          )
        }
        asked={asked ? ids[0] : undefined}
        step={step}
        stream={stream}
        paged={paged}
        latest={latest}
        beforeAsked={beforeAsked}
        beforeStep={beforeStep}
      />
    );
  },
  (a, b) =>
    a.stream === b.stream &&
    a.latest === b.latest &&
    a.asked === b.asked &&
    a.sentNow === b.sentNow &&
    a.paged === b.paged &&
    a.beforeAsked === b.beforeAsked &&
    a.beforeStep === b.beforeStep &&
    sameIds(a.ids, b.ids),
);

/** Whether the view's first Reset is in. */
const pickLanded = (v: StreamView | undefined) => v !== undefined && v.epoch !== null;

export type ThreadListProps = {
  transport: HostTransport;
  stream: StreamId | null;
  busy: boolean;
};

/** The open thread: Thread's log and scroller, the older pages above, the live exchanges. */
export const ThreadList = memo(function ThreadList({ transport, stream, busy }: ThreadListProps) {
  const w = useWords();
  const store = useViewStore();
  const { nodes } = useRoom();
  const hasOlder = useHasOlder(stream);
  const scroller = useCallback(() => nodes.get("thread"), [nodes]);
  const older = useOlder(transport, store, stream, hasOlder, scroller);
  // one grouping over the older pages and the window, built again as a page goes in
  const pickShape = useMemo(() => exchangesPick(older.entries), [older.entries]);
  const { exchanges, paged } = useStreamPart(store, stream, pickShape);
  const asked = latestAsked(exchanges);
  const question = useQuestionWords(stream, asked, asked === null ? undefined : paged.get(asked));
  const landed = useStreamPart(store, stream, pickLanded);
  return (
    // it rests once there is something to rest on: a reopened thread's view arrives after the mount, and the thread
    // settles on its latest answer then. The log is busy until the first view lands and while an older page goes in,
    // so a screen reader doesn't read a whole reopened history, or a page put in above, as news
    <Thread
      question={question}
      who={w.t("You")}
      live={busy}
      resting={!busy && exchanges.length > 0}
      busy={(stream !== null && !landed) || older.loading}
    >
      {stream !== null &&
        exchanges.map((g, i) => {
          const b = i === 0 ? undefined : exchanges[i - 1];
          return (
            <LiveExchange
              key={g.key}
              ids={g.ids}
              asked={g.asked}
              sentNow={g.sentNow}
              stream={stream}
              paged={paged}
              latest={i === exchanges.length - 1}
              beforeAsked={b?.asked === true ? b.ids[0] : undefined}
              beforeStep={b?.ids.find((id) => id.startsWith(STEP))}
            />
          );
        })}
    </Thread>
  );
});
