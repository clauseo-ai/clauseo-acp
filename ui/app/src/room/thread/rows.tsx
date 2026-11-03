// The thread's rows, one per entry. A live row reads only its own entry and its own text from the view store, and is
// memoized on its id, so a flush renders only the rows whose part changed. A paged row (older entries, page_entries)
// shows the entry it was given. Permissions and elicitations are permission-row.tsx's; markers, versions and compaction
// show nothing yet. A queued or removed message shows nothing (the queue's strip holds it), and a steer taken into the
// answer shows as its mark (steer.ts, steer-rows.tsx).
import { memo, useState, type ReactNode } from "react";
import { QuestionLine, Who } from "@clauseo/components/clauseo/answer.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Image } from "@clauseo/components/markdown/image.tsx";
import { Markdown } from "@clauseo/components/markdown/markdown.tsx";
import { useEntry, useStreamPart, useText, useViewStore } from "../../store/hooks.ts";
import { entryIdOf, textKey } from "../../store/view.ts";
import type {
  AgentContentEntry,
  ContentBlock,
  EntryId,
  NoticeEntry,
  StreamId,
  ToolCallEntry,
  UserMessageEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import { Elicitation, LivePermission, PagedPermission } from "../permission-row.tsx";
import { stamp } from "./clock.ts";
import { isolated, shownOf } from "./content.ts";
import { questionText, useQuestionText, useStreaming } from "./live.ts";
import { askedAs, backgroundOf, outcomeOf, runningIn } from "./steer.ts";
import { LiveSteerMark, RequeuedLine, SteerMarkRow } from "./steer-rows.tsx";
import { LimitStopRow, StepRow } from "./stop-rows.tsx";

const QUIET = "text-small text-quiet";
/** The space above a row under another in its section, on the spacing tokens; first in its section, the thread's own
 * gap between sections stands alone. */
const ROW = "not-first:mt-(--s3)";
/** The answer's words: the design's reply type (thread.css .reply-text, 17 px at Clauseo's weight), and its .reply
 * rhythm above them under the meta line, a tool call, a thought or any row before them in its section. */
const REPLY = "reply-text not-first:mt-(--s4)";

/** A block that isn't text, as one line or an image. */
function ContentLine({ block }: { block: ContentBlock }) {
  const w = useThreadWords();
  const shown = shownOf(block);
  if (shown.kind === "image") return <Image src={shown.src} alt={shown.alt} />;
  return (
    <p className={QUIET}>
      <Say>
        {shown.kind === "attached"
          ? w.th("Attached: {name}", { name: isolated(shown.name) })
          : w.th("Something here can't be shown in the thread.")}
      </Say>
    </p>
  );
}

/** The lawyer's question: "You", the time it was sent (the entry's `ts`), and its words. `slots`: the room's named
 * nodes (the pinned question's measure) are the live thread's; a paged question takes none. */
function Question({
  id,
  entry,
  text,
  slots,
}: {
  id: EntryId;
  entry: UserMessageEntry;
  text: string;
  slots: boolean;
}) {
  const w = useThreadWords();
  // "today" as the row first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const at = stamp(w.lang, entry.ts, now);
  return (
    <div data-testid="message-user" data-entry-id={id}>
      {slots ? (
        <QuestionLine who={w.t("You")} time={at} text={text} />
      ) : (
        <>
          <Who name={w.t("You")} time={at} />
          <p className="asked">
            <Say>{text}</Say>
          </p>
        </>
      )}
      {entry.content.map((block, i) =>
        block.text === undefined ? <ContentLine key={i} block={block} /> : null,
      )}
    </div>
  );
}

function LiveQuestion({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: UserMessageEntry;
}) {
  const text = useQuestionText(stream, id, entry);
  return (
    <>
      <Question id={id} entry={entry} text={text} slots />
      {outcomeOf(entry) === "requeued" && <RequeuedLine />}
    </>
  );
}

/** A steer not yet resolved, live or on an older page: a mark in the running answer, a question when no turn runs
 * (steer.ts askedAs over runningIn, the rule the thread's exchanges place it by). */
function UnresolvedSteer({
  stream,
  entry,
  mark,
  question,
}: {
  stream: StreamId;
  entry: UserMessageEntry;
  mark: ReactNode;
  question: ReactNode;
}) {
  const running = useStreamPart(useViewStore(), stream, runningIn);
  return askedAs(entry, running) === "mark" ? mark : question;
}

function LiveUserMessage({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: UserMessageEntry;
}) {
  const as = askedAs(entry, true);
  if (as === "hidden") return null;
  if (outcomeOf(entry) === "injected")
    return <LiveSteerMark id={id} stream={stream} entry={entry} />;
  if (as === "mark")
    return (
      <UnresolvedSteer
        stream={stream}
        entry={entry}
        mark={<LiveSteerMark id={id} stream={stream} entry={entry} />}
        question={<LiveQuestion id={id} stream={stream} entry={entry} />}
      />
    );
  return <LiveQuestion id={id} stream={stream} entry={entry} />;
}

function Answer({ id, text, streaming }: { id: EntryId; text: string; streaming: boolean }) {
  return (
    // busy while it streams, so a screen reader reads the answer once it ends, not each flush's words
    <div
      data-testid="message-agent"
      data-entry-id={id}
      className={REPLY}
      aria-busy={streaming ? true : undefined}
    >
      <Markdown text={text} streaming={streaming} />
    </div>
  );
}

function Thought({ id, text }: { id: EntryId; text: string }) {
  return (
    <p
      data-testid="thought"
      data-entry-id={id}
      className={`${ROW} ${QUIET} whitespace-pre-wrap`}
      dir="auto"
    >
      {text}
    </p>
  );
}

function LiveAnswer({ id, stream, text }: { id: EntryId; stream: StreamId; text: string }) {
  return <Answer id={id} text={text} streaming={useStreaming(stream, id)} />;
}

function LiveAgentText({
  id,
  stream,
  thought,
}: {
  id: EntryId;
  stream: StreamId;
  thought: boolean;
}) {
  const text = useText(useViewStore(), stream, textKey(id));
  return thought ? (
    <Thought id={id} text={text} />
  ) : (
    <LiveAnswer id={id} stream={stream} text={text} />
  );
}

function ToolCall({ id, entry }: { id: EntryId; entry: ToolCallEntry }) {
  const w = useThreadWords();
  const mark =
    entry.status === "failed"
      ? w.th("Failed")
      : entry.status === "cancelled"
        ? w.th("Stopped")
        : backgroundOf(entry) !== undefined
          ? mt(w.lang, "In the background")
          : null;
  return (
    <p
      data-testid="tool-call"
      data-entry-id={id}
      data-tool-call-id={entry.tool_call}
      data-status={entry.status}
      data-background={backgroundOf(entry) === undefined ? undefined : true}
      className={`${ROW} ${QUIET}`}
    >
      <bdi>{entry.title}</bdi>
      {mark !== null && (
        <>
          {" · "}
          <Say>{mark}</Say>
        </>
      )}
    </p>
  );
}

function AgentContent({ id, entry }: { id: EntryId; entry: AgentContentEntry }) {
  return (
    <div data-testid="agent-content" data-entry-id={id} className={ROW}>
      <ContentLine block={entry.block} />
    </div>
  );
}

function Notice({ id, entry }: { id: EntryId; entry: NoticeEntry }) {
  return (
    <p
      data-testid="notice"
      data-entry-id={id}
      data-level={entry.level}
      className={`${ROW} ${QUIET}`}
      dir="auto"
    >
      {entry.text}
    </p>
  );
}

/** The kinds that read nothing but their entry. */
function EntryRow({
  id,
  entry,
  sentNow,
}: {
  id: EntryId;
  entry: ViewEntry;
  sentNow?: boolean | undefined;
}) {
  if (entry.tool_call !== undefined) return <ToolCall id={id} entry={entry.tool_call} />;
  if (entry.agent_content !== undefined)
    return <AgentContent id={id} entry={entry.agent_content} />;
  if (entry.step !== undefined) return <StepRow id={id} entry={entry.step} sentNow={sentNow} />;
  if (entry.limit_stop !== undefined) return <LimitStopRow id={id} entry={entry.limit_stop} />;
  if (entry.notice !== undefined) return <Notice id={id} entry={entry.notice} />;
  if (entry.elicitation !== undefined) return <Elicitation id={id} entry={entry.elicitation} />;
  return null;
}

/** One live row, by entry id. Memoized: the thread renders its rows again when an entry is added, and a row whose id
 * and stream are the same must not render with it. `sentNow`: a step whose turn a steer sent now stopped. */
export const Row = memo(function Row({
  id,
  stream,
  sentNow,
}: {
  id: EntryId;
  stream: StreamId;
  sentNow?: boolean;
}) {
  const entry = useEntry(useViewStore(), stream, id);
  if (entry === undefined) return null;
  if (entry.user_message !== undefined)
    return <LiveUserMessage id={id} stream={stream} entry={entry.user_message} />;
  if (entry.agent_text !== undefined)
    return <LiveAgentText id={id} stream={stream} thought={entry.agent_text.kind === "thought"} />;
  if (entry.permission !== undefined)
    return <LivePermission id={id} stream={stream} entry={entry.permission} />;
  return <EntryRow id={id} entry={entry} sentNow={sentNow} />;
});

/** One older row, from a page of page_entries: nothing in it streams. A turn can start on a page and go on in the
 * window, so a steer still unresolved there is placed as a live one is (UnresolvedSteer). */
export const PagedRow = memo(function PagedRow({
  entry,
  stream,
  sentNow,
}: {
  entry: ViewEntry;
  stream: StreamId;
  sentNow?: boolean;
}) {
  const id = entryIdOf(entry);
  if (entry.user_message !== undefined) {
    const m: UserMessageEntry = entry.user_message;
    const as = askedAs(m, true);
    if (as === "hidden") return null;
    const text = questionText(id, m, undefined);
    const question = (
      <>
        <Question id={id} entry={m} text={text} slots={false} />
        {outcomeOf(m) === "requeued" && <RequeuedLine />}
      </>
    );
    if (as === "question") return question;
    const mark = <SteerMarkRow id={id} entry={m} text={text} />;
    if (outcomeOf(m) === "injected") return mark;
    return <UnresolvedSteer stream={stream} entry={m} mark={mark} question={question} />;
  }
  if (entry.agent_text !== undefined) {
    const a = entry.agent_text;
    return a.kind === "thought" ? (
      <Thought id={id} text={a.text} />
    ) : (
      <Answer id={id} text={a.text} streaming={false} />
    );
  }
  if (entry.permission !== undefined) return <PagedPermission id={id} entry={entry.permission} />;
  return <EntryRow id={id} entry={entry} sentNow={sentNow} />;
});
