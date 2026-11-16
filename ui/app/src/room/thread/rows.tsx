// The thread's rows, one per entry. A live row reads only its own entry and its own text from the view store, and is
// memoized on its id, so a flush renders only the rows whose part changed. A paged row (older entries, page_entries)
// shows the entry it was given. Permissions and elicitations are permission-row.tsx's; a restore made from Versions
// while the conversation was open shows as a quiet line; other markers, versions and compaction show nothing yet. A queued or removed message shows nothing (the queue's strip holds it), a steer taken into the
// answer shows as its mark, a steer not yet resolved as its words waiting, with Send now, and a steer never sent as its
// words and that they were not sent (steer.ts, steer-rows.tsx).
import { memo, useState, useSyncExternalStore } from "react";
import { QuestionLine, Who } from "@clauseo/components/clauseo/answer.tsx";
import { FoldedQuestion } from "@clauseo/components/clauseo/question-fold.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { frd } from "@clauseo/components/lib/words-first-read.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { Image } from "@clauseo/components/markdown/image.tsx";
import { Markdown } from "@clauseo/components/markdown/markdown.tsx";
import { useEntry, useText, useViewStore } from "../../store/hooks.ts";
import { entryIdOf, textKey } from "../../store/view.ts";
import type {
  AgentContentEntry,
  ContentBlock,
  EntryId,
  MarkerEntry,
  NoticeEntry,
  StreamId,
  ToolCallEntry,
  UserMessageEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import { Elicitation, LivePermission, PagedPermission } from "../permission-row.tsx";
import { firstReadMarks, isFirstReadTurn, subscribeMarks } from "../first-read/marks.ts";
import { stamp } from "./clock.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { isolated, shownOf } from "./content.ts";
import { noticeLine } from "./notice-words.ts";
import { questionText, useQuestionText, useStreaming } from "./live.ts";
import { askedAs, backgroundOf, outcomeOf } from "./steer.ts";
import {
  LiveSteerMark,
  LiveSteerNotSent,
  LiveSteerWaiting,
  RequeuedLine,
  SteerMarkRow,
  SteerNotSentRow,
  SteerWaitingRow,
} from "./steer-rows.tsx";
import { LimitStopRow, StepRow } from "./stop-rows.tsx";
import { doingOf, plainOf, thoughtText, titled, unsettled } from "./step-names.ts";
import { skillOf } from "./skill-words.ts";
import { toolNameOf } from "./tool-names.ts";
import { overTitle, skillDone } from "./tool-words.ts";

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
  text: asked,
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
  // UD2: the first read's prompt never shows, by its turn (first-read/marks.ts), never by its words
  useSyncExternalStore(subscribeMarks, firstReadMarks, firstReadMarks);
  const text = isFirstReadTurn(entry.turn) ? frd(w.lang, "Clauseo's first read") : asked;
  return (
    <div data-testid="message-user" data-entry-id={id}>
      {slots ? (
        <QuestionLine who={w.t("You")} time={at} text={text} />
      ) : (
        <>
          <Who name={w.t("You")} time={at} />
          <FoldedQuestion text={text} />
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

function LiveUserMessage({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: UserMessageEntry;
}) {
  const as = askedAs(entry);
  if (as === "hidden") return null;
  if (as === "mark") return <LiveSteerMark id={id} stream={stream} entry={entry} />;
  if (as === "waiting") return <LiveSteerWaiting id={id} stream={stream} entry={entry} />;
  if (as === "not_sent") return <LiveSteerNotSent id={id} stream={stream} entry={entry} />;
  return <LiveQuestion id={id} stream={stream} entry={entry} />;
}

function Answer({ id, text, streaming }: { id: EntryId; text: string; streaming: boolean }) {
  // no words yet, or only blank lines: no empty block standing in the answer's space (D-030)
  if (text.trim() === "") return null;
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

/** The space above a thought under another row: the reply's own (REPLY), so a thought that opens an answer stands
 * under the meta line as the answer's first words would (D-030). */
const THOUGHT = "not-first:mt-(--s4)";

function Thought({ id, text }: { id: EntryId; text: string }) {
  const shown = thoughtText(text);
  // a thought with no words yet (or only blank lines) draws nothing, never an empty row
  if (shown === "") return null;
  return (
    <p
      data-testid="thought"
      data-entry-id={id}
      className={`${THOUGHT} ${QUIET} whitespace-pre-wrap`}
      dir="auto"
    >
      {shown}
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
      <bdi>
        {skillOf(entry) !== null
          ? unsettled(entry)
            ? doingOf(w.lang, entry)
            : skillDone(w.lang, entry, entry.status !== "completed")
          : toolNameOf(entry.title) === undefined
            ? plainOf(entry.title)
            : unsettled(entry)
              ? titled(entry, stepLive(w.lang, "Working on it."), w.lang)
              : overTitle(w.lang, entry.title, entry.status !== "completed")}
      </bdi>
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

/** A host notice in the app's words for its kind, or its English fallback for a kind this version doesn't know. An
 * agent's notice (no kind) shows nothing: it is the agent's own free text, such as Codex's raw error with its request id
 * and the key's masked ending, and the lawyer reads the app's own line for what went wrong instead. */
function Notice({ id, entry }: { id: EntryId; entry: NoticeEntry }) {
  const w = useThreadWords();
  if (entry.kind === undefined || entry.kind === null) return null;
  const agent = useAgentPlan();
  return (
    <p
      data-testid="notice"
      data-entry-id={id}
      data-level={entry.level}
      className={`${ROW} ${QUIET}`}
      dir="auto"
    >
      {noticeLine(entry.kind, w, agent) ?? entry.text}
    </p>
  );
}

/** A restore of the matter made from Versions while this conversation was open (the host's version_restored marker on
 * its stream), as a quiet line; any other marker shows nothing. */
function Marker({ id, entry }: { id: EntryId; entry: MarkerEntry }) {
  const w = useThreadWords();
  const restored = entry.kind.version_restored;
  if (restored === undefined) return null;
  return (
    <p data-testid="marker-restored" data-entry-id={id} className={`${ROW} ${QUIET}`}>
      <Say>
        {restored.undoes === undefined
          ? w.th("You restored an earlier version of the matter.")
          : w.th("You undid the restore.")}
      </Say>
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
  if (entry.marker !== undefined) return <Marker id={id} entry={entry.marker} />;
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
 * window, so a steer still unresolved there waits, with Send now, as a live one does. */
export const PagedRow = memo(function PagedRow({
  entry,
  sentNow,
}: {
  entry: ViewEntry;
  sentNow?: boolean;
}) {
  const id = entryIdOf(entry);
  if (entry.user_message !== undefined) {
    const m: UserMessageEntry = entry.user_message;
    const as = askedAs(m);
    if (as === "hidden") return null;
    const text = questionText(id, m, undefined);
    if (as === "mark") return <SteerMarkRow id={id} entry={m} text={text} />;
    if (as === "waiting") return <SteerWaitingRow id={id} entry={m} text={text} />;
    if (as === "not_sent") return <SteerNotSentRow id={id} text={text} />;
    return (
      <>
        <Question id={id} entry={m} text={text} slots={false} />
        {outcomeOf(m) === "requeued" && <RequeuedLine />}
      </>
    );
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
