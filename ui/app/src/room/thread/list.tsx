// The open thread, in its own chunk with the Markdown renderer it shows answers through (chunk.ts): Thread's log and
// scroller, the older pages above, and the exchanges, grouped by turn (exchanges.ts, rebuilt only as the id list or a
// user message's place changes), each row reading its own entry (rows.tsx), so while an answer streams one frame
// commits once and renders only the row whose text grew.
// Nothing here is needed for the room's first paint: Work, the session and the composer are in the room's own chunk.
import { memo, useCallback, useMemo, useState, type ReactNode } from "react";
import { Who } from "@clauseo/components/clauseo/answer.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Thread } from "@clauseo/components/clauseo/thread.tsx";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useEntry, useStreamPart, useViewStore } from "../../store/hooks.ts";
import { entryIdOf, orderOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, StreamId, Timestamp, ViewEntry } from "../../transport/protocol.ts";
import { stamp } from "./clock.ts";
import { exchangesPick, groupExchanges, type Exchange } from "./exchanges.ts";
import { sameIds } from "./ids.ts";
import { useHasOlder, useLatestQuestion } from "./live.ts";
import { useOlder } from "./older.ts";
import { PagedRow, Row } from "./rows.tsx";
import { LiveSteerNotes, PagedSteerNotes } from "./steer-rows.tsx";

/** The composer's limit warning, carried in this chunk with the stop's note (limits/limit-warning.tsx loads it). */
export { WarningNote } from "./limit-warning-note.tsx";

/** The answer's meta line: "Clauseo", with its turn's time once the turn's step is in (the step's `ts`), alone
 * before it. */
function AnswerWho({ time }: { time: Timestamp | null }) {
  const w = useWords();
  // "today" as the line first showed: no clock runs to move it
  const [now] = useState(Date.now);
  return <Who name={w.t("Clauseo")} time={time === null ? undefined : stamp(w.lang, time, now)} />;
}

const STEP = "step:";

/** A live exchange's meta line, reading only its turn's step, so a flush of the answer's text doesn't render it. */
function LiveAnswerWho({ ids, stream }: { ids: readonly EntryId[]; stream: StreamId }) {
  const step = ids.find((id) => id.startsWith(STEP));
  const entry = useEntry(useViewStore(), stream, step ?? STEP);
  return <AnswerWho time={entry?.step?.ts ?? null} />;
}

/** One exchange: the question's section and the answer's. The room's named nodes for the question and the answer
 * (the thread's scroll and its pinned question measure them) go on the latest exchange only. */
function Sections({
  question,
  answer,
  who,
  latest,
}: {
  question: ReactNode | null;
  answer: ReactNode;
  /** The answer's meta line. */
  who: ReactNode;
  latest: boolean;
}) {
  const w = useWords();
  const { nodes } = useRoom();
  return (
    <>
      {question !== null && (
        <section
          className="step step-question"
          ref={latest ? nodes.slot("questionStep") : undefined}
          aria-label={th(w.lang, "Your question")}
        >
          {question}
        </section>
      )}
      {(answer !== null || (latest && question !== null)) && (
        <section
          className="step step-answer"
          ref={latest ? nodes.slot("answerStep") : undefined}
          aria-label={th(w.lang, "Clauseo's answer")}
        >
          {/* the design's meta line opens each answer: "Clauseo", and its turn's time once the step is in */}
          <div className="meta meta-live" ref={latest ? nodes.slot("meta") : undefined}>
            {who}
          </div>
          {answer}
        </section>
      )}
    </>
  );
}

const TOOL = "tool_call:";

const LiveExchange = memo(
  function LiveExchange({
    ids,
    asked,
    sentNow,
    stream,
    latest,
  }: {
    ids: readonly EntryId[];
    asked: boolean;
    sentNow: EntryId | null;
    stream: StreamId;
    latest: boolean;
  }) {
    const rest = asked ? ids.slice(1) : ids;
    const answer =
      rest.length === 0
        ? null
        : rest.map((id) =>
            id === sentNow ? (
              <Row key={id} id={id} stream={stream} sentNow />
            ) : (
              <Row key={id} id={id} stream={stream} />
            ),
          );
    return (
      <Sections
        question={asked ? <Row id={ids[0]!} stream={stream} /> : null}
        answer={
          answer !== null && rest.some((id) => id.startsWith(TOOL)) ? (
            <>
              {answer}
              <LiveSteerNotes ids={rest} stream={stream} />
            </>
          ) : (
            answer
          )
        }
        who={<LiveAnswerWho ids={rest} stream={stream} />}
        latest={latest}
      />
    );
  },
  (a, b) =>
    a.stream === b.stream &&
    a.latest === b.latest &&
    a.asked === b.asked &&
    a.sentNow === b.sentNow &&
    sameIds(a.ids, b.ids),
);

function PagedExchange({
  entries,
  asked,
  sentNow,
}: {
  entries: readonly ViewEntry[];
  asked: boolean;
  sentNow: EntryId | null;
}) {
  const first = entries[0]!;
  const rest = asked ? entries.slice(1) : entries;
  return (
    <Sections
      question={asked ? <PagedRow entry={first} /> : null}
      answer={
        rest.length === 0 ? null : (
          <>
            {rest.map((e) => {
              const id = entryIdOf(e);
              return id === sentNow ? (
                <PagedRow key={id} entry={e} sentNow />
              ) : (
                <PagedRow key={id} entry={e} />
              );
            })}
            <PagedSteerNotes entries={rest} />
          </>
        )
      }
      who={<AnswerWho time={rest.find((e) => e.step !== undefined)?.step?.ts ?? null} />}
      latest={false}
    />
  );
}

/** The older pages, by exchange (by turn, as the live window's), less anything the live window now holds. */
function OlderPages({ entries, below }: { entries: readonly ViewEntry[]; below: number | null }) {
  const groups = useMemo(() => {
    const shown = below === null ? entries : entries.filter((e) => orderOf(e) < below);
    const byId = new Map(shown.map((e) => [entryIdOf(e), e] as const));
    return groupExchanges([...byId], false).map((ex: Exchange) => ({
      ex,
      entries: ex.ids.map((id) => byId.get(id)!),
    }));
  }, [entries, below]);
  return groups.map(({ ex, entries: own }) => (
    <PagedExchange key={ex.key} entries={own} asked={ex.asked} sentNow={ex.sentNow} />
  ));
}

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
  const [pickShape] = useState(exchangesPick);
  const { exchanges, first: firstId } = useStreamPart(store, stream, pickShape);
  const question = useLatestQuestion(stream);
  const hasOlder = useHasOlder(stream);
  const scroller = useCallback(() => nodes.get("thread"), [nodes]);
  const older = useOlder(transport, store, stream, hasOlder, scroller);
  const landed = useStreamPart(store, stream, pickLanded);
  const first =
    stream === null || firstId === null ? undefined : store.view(stream)?.entries.get(firstId);
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
      {older.entries.length > 0 && (
        <OlderPages entries={older.entries} below={first === undefined ? null : orderOf(first)} />
      )}
      {stream !== null &&
        exchanges.map((g, i) => (
          <LiveExchange
            key={g.key}
            ids={g.ids}
            asked={g.asked}
            sentNow={g.sentNow}
            stream={stream}
            latest={i === exchanges.length - 1}
          />
        ))}
    </Thread>
  );
});
