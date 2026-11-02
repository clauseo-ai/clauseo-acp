// The open thread, in its own chunk with the Markdown renderer it shows answers through (chunk.ts): Thread's log and
// scroller, the older pages above, and the exchanges, grouped from the id list alone (ids.ts), each row reading its
// own entry (rows.tsx), so while an answer streams one frame commits once and renders only the row whose text grew.
// Nothing here is needed for the room's first paint: Work, the session and the composer are in the room's own chunk.
import { memo, useCallback, useMemo, useState, type ReactNode } from "react";
import { Who } from "@clauseo/components/clauseo/answer.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Thread } from "@clauseo/components/clauseo/thread.tsx";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { MarkdownHostContext, type MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { useEntry, useEntryIds, useStreamPart, useViewStore } from "../../store/hooks.ts";
import { entryIdOf, orderOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, StreamId, Timestamp, ViewEntry } from "../../transport/protocol.ts";
import { stamp } from "./clock.ts";
import { exchangesOf, isQuestion, sameIds } from "./ids.ts";
import { useHasOlder, useLatestQuestion } from "./live.ts";
import { useOlder } from "./older.ts";
import { PagedRow, Row } from "./rows.tsx";

/** What a link or reference in an answer does when clicked: nothing yet, as #123's transcript has it. The app has
 * no open_external or open_reference command, and the renderer cancels every click, so the webview never navigates. */
const ANSWER_HOST: MarkdownHost = {
  openExternal() {},
  openReference() {},
};

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

const LiveExchange = memo(
  function LiveExchange({
    ids,
    stream,
    latest,
  }: {
    ids: readonly EntryId[];
    stream: StreamId;
    latest: boolean;
  }) {
    const asked = isQuestion(ids[0]!);
    const rest = asked ? ids.slice(1) : ids;
    return (
      <Sections
        question={asked ? <Row id={ids[0]!} stream={stream} /> : null}
        answer={
          rest.length === 0 ? null : rest.map((id) => <Row key={id} id={id} stream={stream} />)
        }
        who={<LiveAnswerWho ids={rest} stream={stream} />}
        latest={latest}
      />
    );
  },
  (a, b) => a.stream === b.stream && a.latest === b.latest && sameIds(a.ids, b.ids),
);

function PagedExchange({ entries }: { entries: readonly ViewEntry[] }) {
  const first = entries[0]!;
  const asked = first.user_message !== undefined;
  const rest = asked ? entries.slice(1) : entries;
  return (
    <Sections
      question={asked ? <PagedRow entry={first} /> : null}
      answer={rest.length === 0 ? null : rest.map((e) => <PagedRow key={entryIdOf(e)} entry={e} />)}
      who={<AnswerWho time={rest.find((e) => e.step !== undefined)?.step?.ts ?? null} />}
      latest={false}
    />
  );
}

/** The older pages, by exchange, less anything the live window now holds. */
function OlderPages({ entries, below }: { entries: readonly ViewEntry[]; below: number | null }) {
  const groups = useMemo(() => {
    const shown = below === null ? entries : entries.filter((e) => orderOf(e) < below);
    const out: ViewEntry[][] = [];
    for (const e of shown) {
      const last = out.at(-1);
      if (last === undefined || e.user_message !== undefined) out.push([e]);
      else last.push(e);
    }
    return out;
  }, [entries, below]);
  return groups.map((g) => <PagedExchange key={entryIdOf(g[0]!)} entries={g} />);
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
  const ids = useEntryIds(store, stream);
  const question = useLatestQuestion(stream);
  const hasOlder = useHasOlder(stream);
  const scroller = useCallback(() => nodes.get("thread"), [nodes]);
  const older = useOlder(transport, store, stream, hasOlder, scroller);
  const groups = useMemo(() => exchangesOf(ids), [ids]);
  const landed = useStreamPart(store, stream, pickLanded);
  const first =
    stream === null || ids[0] === undefined ? undefined : store.view(stream)?.entries.get(ids[0]);
  return (
    // it rests once there is something to rest on: a reopened thread's view arrives after the mount, and the thread
    // settles on its latest answer then. The log is busy until the first view lands and while an older page goes in,
    // so a screen reader doesn't read a whole reopened history, or a page put in above, as news
    <MarkdownHostContext value={ANSWER_HOST}>
      <Thread
        question={question}
        who={w.t("You")}
        live={busy}
        resting={!busy && ids.length > 0}
        busy={(stream !== null && !landed) || older.loading}
      >
        {older.entries.length > 0 && (
          <OlderPages entries={older.entries} below={first === undefined ? null : orderOf(first)} />
        )}
        {stream !== null &&
          groups.map((g, i) => (
            <LiveExchange key={g[0]} ids={g} stream={stream} latest={i === groups.length - 1} />
          ))}
      </Thread>
    </MarkdownHostContext>
  );
});
