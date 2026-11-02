// The agent's permission requests and elicitations in the thread. A live permission row reads its tool call from the
// view store too, so its folded line follows the call to completion; a paged one shows its entry alone. Answering
// sends answer_permission and holds the prompt busy until the entry folds; allow_always shows the OS's confirm line
// while the native dialog is up, and a confirm declined there (`transport.unauthorized`) puts the prompt back as it
// was. Only the prompt Work names (KeyedRequestContext: the oldest pending one) takes the keys. An elicitation, until
// it is designed, says the agent has a question, with its words and Stop; its URL is never shown or opened.
import { use, useCallback, useMemo, useRef, useState } from "react";
import { PermissionPrompt } from "@clauseo/components/clauseo/permission.tsx";
import type { PermissionAnswer } from "@clauseo/components/clauseo/permission-keys.ts";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useStreamPart, useViewStore } from "../store/hooks.ts";
import type { StreamView } from "../store/view.ts";
import { toHostError } from "../transport/host-transport.ts";
import type {
  ElicitationEntry,
  EntryId,
  PermissionEntry,
  StreamId,
  ToolCallEntry,
} from "../transport/protocol.ts";
import {
  elicitationMessage,
  elicitationState,
  KeyedRequestContext,
  PERMISSION_TEST_IDS,
  permissionModel,
  toolCallIdOf,
} from "./permission-model.ts";
import { useThreadActions } from "./thread/session.ts";

/** The space above a row under another in its section (rows.tsx's ROW). */
const ROW = "not-first:mt-(--s3)";

/** An answer on its way: sent once answer_permission resolved, after which the prompt waits for the entry to fold. */
type Asked = { answer: PermissionAnswer; sent: boolean };

function Permission({
  id,
  entry,
  call,
  keys,
}: {
  id: EntryId;
  entry: PermissionEntry;
  call: ToolCallEntry | undefined;
  keys: boolean;
}) {
  const w = useThreadWords();
  const { answer } = useThreadActions();
  const m = useMemo(() => permissionModel(entry, call), [entry, call]);
  const offered = useMemo(() => m.offers.map((o) => o.answer), [m]);
  const [asked, setAsked] = useState<Asked | null>(null);
  const [error, setError] = useState<string | null>(null);
  // one answer at a time, even for two keys in one frame
  const flying = useRef(false);
  const pending = m.state === "pending";
  const lang = w.lang;
  const onAnswer = useCallback(
    (a: PermissionAnswer) => {
      const offer = m.offers.find((o) => o.answer === a);
      if (m.state !== "pending" || offer === undefined || flying.current) return;
      flying.current = true;
      setError(null);
      setAsked({ answer: a, sent: false });
      answer(entry.request, offer.option).then(
        () => setAsked({ answer: a, sent: true }),
        (e: unknown) => {
          const code = toHostError(e).code;
          // answered elsewhere first: the next patch folds it
          if (code === "permission.not_pending") return setAsked({ answer: a, sent: true });
          flying.current = false;
          setAsked(null);
          // a confirm the lawyer declined (only Always for this matter opens one) says nothing: the prompt is simply
          // back. Any other refusal, unauthorized included, says why
          if (a !== "always" || code !== "transport.unauthorized") setError(errorWords(lang, code));
        },
      );
    },
    [m, answer, entry.request, lang],
  );
  return (
    <div data-entry-id={id} className={ROW}>
      <PermissionPrompt
        paper={m.paper}
        state={m.state}
        onAnswer={onAnswer}
        keys={keys}
        offered={offered}
        {...(m.asks ? { question: w.th("Clauseo asks before going on.") } : {})}
        title={m.title}
        kept={m.kept}
        done={m.done}
        busy={pending && asked !== null}
        confirming={pending && asked?.answer === "always" && !asked.sent}
        error={pending ? error : null}
        testIds={PERMISSION_TEST_IDS}
        dataState={m.dataState}
        {...(m.dataAnswer === undefined ? {} : { dataAnswer: m.dataAnswer })}
      />
    </div>
  );
}

/** A live permission row: its entry, its tool call as the view has it now, and whether it takes the keys. */
export function LivePermission({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: PermissionEntry;
}) {
  const callId = toolCallIdOf(entry);
  const pick = useCallback(
    (v: StreamView | undefined) =>
      callId === null ? undefined : v?.entries.get(callId)?.tool_call,
    [callId],
  );
  const call = useStreamPart(useViewStore(), stream, pick);
  const keys = use(KeyedRequestContext) === entry.request;
  return <Permission id={id} entry={entry} call={call} keys={keys} />;
}

/** A permission on an older page: its entry alone, and never the keys. */
export function PagedPermission({ id, entry }: { id: EntryId; entry: PermissionEntry }) {
  return <Permission id={id} entry={entry} call={undefined} keys={false} />;
}

/** An elicitation: while it waits, "A question for you", the agent's words and Stop the answer; then its words. */
export function Elicitation({ id, entry }: { id: EntryId; entry: ElicitationEntry }) {
  const w = useThreadWords();
  const { stop } = useThreadActions();
  const state = elicitationState(entry);
  const message = elicitationMessage(entry);
  const pending = state === "pending";
  return (
    <div data-testid="elicitation" data-entry-id={id} data-state={state} className={ROW}>
      {pending && (
        <p className="text-small">
          <Say>{frame(w.lang, "A question for you")}</Say>
        </p>
      )}
      {message !== null && (
        <p className="text-small text-quiet whitespace-pre-wrap">
          <bdi>{message}</bdi>
        </p>
      )}
      {pending && (
        <Button kind="quiet" size="small" data-testid="elicitation-stop" onClick={stop}>
          {w.th("Stop the answer")}
        </Button>
      )}
    </div>
  );
}
