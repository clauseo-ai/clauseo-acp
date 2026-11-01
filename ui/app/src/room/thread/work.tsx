// The room's work column: the open thread and the composer under it. The thread reopens the matter's newest thread,
// or the first Ask makes one (session.ts). Rows are grouped by exchange from the id list alone (ids.ts), each row
// reading its own entry (rows.tsx), so while an answer streams one frame commits once and renders only the row whose
// text grew.
import { memo, useCallback, useEffect, useMemo, type ReactNode } from "react";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Thread } from "@clauseo/components/clauseo/thread.tsx";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { MarkdownHostContext, type MarkdownHost } from "@clauseo/components/markdown/host.ts";
import { useEntryIds, useStreamError, useViewStore, useWatch } from "../../store/hooks.ts";
import { entryIdOf, orderOf } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type {
  EntryId,
  MatterSummary,
  StreamId,
  ThreadId,
  ViewEntry,
} from "../../transport/protocol.ts";
import { exchangesOf, isQuestion, sameIds } from "./ids.ts";
import { useBusy, useHasOlder, useLatestQuestion } from "./live.ts";
import { useOlder } from "./older.ts";
import { PagedRow, Row } from "./rows.tsx";
import { ThreadActionsContext, useThreadSession } from "./session.ts";
import { WorkComposer } from "./work-composer.tsx";

/** What a link or reference in an answer does when clicked: nothing yet, as #123's transcript has it. The app has
 * no open_external or open_reference command, and the renderer cancels every click, so the webview never navigates. */
const ANSWER_HOST: MarkdownHost = {
  openExternal() {},
  openReference() {},
};

/** One exchange: the question's section and the answer's. The room's named nodes for the question and the answer
 * (the thread's scroll and its pinned question measure them) go on the latest exchange only. */
function Sections({
  question,
  answer,
  latest,
}: {
  question: ReactNode | null;
  answer: ReactNode;
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

const ThreadList = memo(function ThreadList({
  transport,
  stream,
  busy,
}: {
  transport: HostTransport;
  stream: StreamId | null;
  busy: boolean;
}) {
  const w = useWords();
  const store = useViewStore();
  const { nodes } = useRoom();
  const ids = useEntryIds(store, stream);
  const question = useLatestQuestion(stream);
  const hasOlder = useHasOlder(stream);
  const scroller = useCallback(() => nodes.get("thread"), [nodes]);
  const older = useOlder(transport, store, stream, hasOlder, scroller);
  const groups = useMemo(() => exchangesOf(ids), [ids]);
  const first =
    stream === null || ids[0] === undefined ? undefined : store.view(stream)?.entries.get(ids[0]);
  return (
    <Thread question={question} who={w.t("You")} live={busy} resting={!busy}>
      {older.length > 0 && (
        <OlderPages entries={older} below={first === undefined ? null : orderOf(first)} />
      )}
      {stream !== null &&
        groups.map((g, i) => (
          <LiveExchange key={g[0]} ids={g} stream={stream} latest={i === groups.length - 1} />
        ))}
    </Thread>
  );
});

/** The work column of the room: the matter's thread, and the composer (disabled with no matter). */
export function Work({
  transport,
  matter,
  onThread,
}: {
  transport: HostTransport;
  matter: MatterSummary | null;
  /** Hears the open thread: null until there is one. */
  onThread?: (thread: ThreadId | null) => void;
}) {
  const store = useViewStore();
  const session = useThreadSession(transport, matter?.matter ?? null);
  const stream = session.actions.stream;
  useWatch(store, stream);
  const busy = useBusy(stream, session.sentTurn);
  const failed = useStreamError(store, stream);
  useEffect(() => onThread?.(session.thread), [onThread, session.thread]);
  return (
    <MarkdownHostContext value={ANSWER_HOST}>
      <ThreadActionsContext value={session.actions}>
        {/* the list's test id; `contents` keeps the room's column layout, whose scroller is Thread's .room-thread */}
        <div data-testid="transcript" className="contents">
          <ThreadList transport={transport} stream={stream} busy={busy} />
        </div>
      </ThreadActionsContext>
      <WorkComposer
        disabled={matter === null}
        busy={busy}
        refused={session.refused}
        failed={failed?.code ?? null}
        onAsk={session.ask}
        onStop={session.stop}
        onRetry={() => {
          if (stream !== null) store.retry(stream);
        }}
      />
    </MarkdownHostContext>
  );
}
