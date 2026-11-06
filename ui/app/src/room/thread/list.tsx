// The open thread, in its own chunk with the Markdown renderer it shows answers through (chunk.ts): Thread's log and
// scroller, the older pages above, and the exchanges, grouped by turn (exchanges.ts, rebuilt only as the id list or a
// user message's place changes), each row reading its own entry (rows.tsx) and each run of tool calls and thoughts in
// an answer folded into a step group (step-groups.tsx), so while an answer streams one frame commits once and renders
// only the row whose text grew.
// Nothing here is needed for the room's first paint: Work, the session and the composer are in the room's own chunk.
import { memo, useCallback, useMemo, useState, useSyncExternalStore, type ReactNode } from "react";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Thread } from "@clauseo/components/clauseo/thread.tsx";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { frd } from "@clauseo/components/lib/words-first-read.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, StreamId, Timestamp, TurnId, ViewEntry } from "../../transport/protocol.ts";
import { firstReadMarks, isFirstReadTurn, subscribeMarks } from "../first-read/marks.ts";
import { AnswerMeta } from "./answer-meta.tsx";
import { CaseLawHintSlot } from "./case-law-hint.tsx";
import { stamp } from "./clock.ts";
import { exchangesPick, latestAsked } from "./exchanges.ts";
import { sameIds } from "./ids.ts";
import { useHasOlder, useQuestionWords } from "./live.ts";
import { useJumpLanding, useJumpOrder } from "./jump-entry.ts";
import { useOlder } from "./older.ts";
import { RowOf } from "./row-of.tsx";
import { SourcesTransport, webPick } from "./sources.ts";
import { WebSources } from "./sources.tsx";
import { SteerNotesOf } from "./steer-rows.tsx";
import { LiveAnswerRows } from "./step-groups.tsx";
import { pickQueued, useTimeOf } from "./time-of.ts";

/** The composer's limit warning, carried in this chunk with the stop's note (limits/limit-warning.tsx loads it). */
export { WarningNote } from "./limit-warning-note.tsx";
/** U15's out-of-credit note, carried here as U12's warning is (room/connectors/out-of-credit.tsx loads it). */
export { OutOfCreditNote } from "./out-of-credit-note.tsx";
/** UD2's notes in the composer's notice line: the expired sign-in card and the first read's line (work.tsx loads it). */
export { AfterSignInNote } from "./after-signin-note.tsx";

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
  since,
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
  /** The question's id for when the answer started, none where its turn waited (LiveExchange). */
  since: EntryId | undefined;
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
              asked={since}
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

/** The web sources of an exchange's turn (sources.tsx): what its tool calls found, each entry's `web` in order, with the
 * thread's agent from its header. Its tool call ids are taken once per id list, and an exchange with none draws and
 * subscribes to nothing. */
function ExchangeSources({
  ids,
  stream,
  paged,
}: {
  ids: readonly EntryId[];
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
}) {
  const tools = useMemo(() => ids.filter((id) => id.startsWith(TOOL)), [ids]);
  return tools.length === 0 ? null : <ToolSources tools={tools} stream={stream} paged={paged} />;
}

function ToolSources({
  tools,
  stream,
  paged,
}: {
  tools: readonly EntryId[];
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
}) {
  // the calls that carry web, and only their web: the same array while none of them changes, so a flush that leaves
  // them as they were renders nothing
  const pick = useMemo(() => webPick(tools, paged), [tools, paged]);
  const webs = useStreamPart(useViewStore(), stream, pick);
  const agent = useStreamPart(useViewStore(), stream, pickAgent);
  return <WebSources webs={webs} agent={agent} />;
}

const pickAgent = (v: StreamView | undefined) => v?.header?.header?.agent;

/** One exchange, its rows live or from an older page: a turn can start on a page and go on in the window. */
const LiveExchange = memo(
  function LiveExchange({
    ids,
    asked,
    sentNow,
    turn,
    stream,
    paged,
    latest,
    beforeAsked,
    beforeStep,
  }: {
    ids: readonly EntryId[];
    asked: boolean;
    sentNow: EntryId | null;
    /** Its turn, for the case-law hint the host may send for it (case-law-hint.tsx). */
    turn: TurnId | null;
    stream: StreamId;
    paged: ReadonlyMap<EntryId, ViewEntry>;
    latest: boolean;
    /** The exchange before's question and step, for the names it has taken (Sections). */
    beforeAsked: EntryId | undefined;
    beforeStep: EntryId | undefined;
  }) {
    const rest = asked ? ids.slice(1) : ids;
    const step = rest.find((id) => id.startsWith(STEP));
    const question = asked ? ids[0] : undefined;
    // a turn seen waiting behind another conversation's writer stays one that waited once it runs: the view keeps the
    // wait only while it lasts (the header's `queued`), and the meta line remounts as the answer's first row comes, so
    // the exchange remembers it, by its question, and its meta line takes no start time from the question
    const queued = useStreamPart(useViewStore(), stream, pickQueued);
    const [waitedFor, setWaitedFor] = useState<EntryId | null>(null);
    if (queued && latest && step === undefined && question !== undefined && waitedFor !== question)
      setWaitedFor(question);
    const since = question !== undefined && waitedFor === question ? undefined : question;
    const answer =
      rest.length === 0 ? null : (
        <LiveAnswerRows
          ids={rest}
          step={step}
          asked={since}
          before={beforeStep}
          stream={stream}
          paged={paged}
          sentNow={sentNow}
          latest={latest}
          sources={<ExchangeSources ids={ids} stream={stream} paged={paged} />}
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
          answer === null ? null : (
            <>
              {answer}
              {notes && <SteerNotesOf ids={rest} stream={stream} paged={paged} />}
              <CaseLawHintSlot stream={stream} turn={turn} />
            </>
          )
        }
        asked={question}
        since={since}
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
    a.turn === b.turn &&
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
  // a Find result opens the thread at one of its entries: paged back to it, then scrolled to it
  const reach = useJumpOrder(stream);
  const older = useOlder(transport, store, stream, hasOlder, scroller, reach);
  useJumpLanding(store, stream, reach, older.entries, scroller, older.start);
  // one grouping over the older pages and the window, built again as a page goes in
  const pickShape = useMemo(() => exchangesPick(older.entries), [older.entries]);
  const { exchanges, paged } = useStreamPart(store, stream, pickShape);
  const asked = latestAsked(exchanges);
  const words = useQuestionWords(stream, asked, asked === null ? undefined : paged.get(asked));
  // UD2: the first read's prompt never shows, pinned either, by its turn (first-read/marks.ts)
  const pickTurn = useCallback(
    (v: StreamView | undefined) =>
      asked === null
        ? null
        : ((paged.get(asked) ?? v?.entries.get(asked))?.user_message?.turn ?? null),
    [asked, paged],
  );
  const askedTurn = useStreamPart(store, stream, pickTurn);
  useSyncExternalStore(subscribeMarks, firstReadMarks, firstReadMarks);
  const question =
    askedTurn !== null && isFirstReadTurn(askedTurn) ? frd(w.lang, "Clauseo's first read") : words;
  const landed = useStreamPart(store, stream, pickLanded);
  return (
    // it rests once there is something to rest on: a reopened thread's view arrives after the mount, and the thread
    // settles on its latest answer then. The log is busy until the first view lands and while an older page goes in,
    // so a screen reader doesn't read a whole reopened history, or a page put in above, as news
    <SourcesTransport value={transport}>
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
                turn={g.turn}
                stream={stream}
                paged={paged}
                latest={i === exchanges.length - 1}
                beforeAsked={b?.asked === true ? b.ids[0] : undefined}
                beforeStep={b?.ids.find((id) => id.startsWith(STEP))}
              />
            );
          })}
      </Thread>
    </SourcesTransport>
  );
});
