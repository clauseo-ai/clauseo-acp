// The room's work column: the open conversation's thread and the composer under it. The conversations (conversations.ts)
// say which thread is open, and make one on a draft's first Ask (session.ts); the room keys this column by
// conversation, so each opens fresh, with its own words in the field. The thread's list, its rows and the Markdown renderer are one chunk of
// their own (chunk.ts, list.tsx), started as Work mounts; the session opens the thread only once it is in, so the
// rows never wait for it. The composer and the transcript's box are here, for the first paint.
import { Suspense, useEffect, useMemo, useRef, useState } from "react";
import type { ComposerHandle, PlanChoice } from "@clauseo/components/clauseo/composer.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamError, useStreamPart, useViewStore, useWatch } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AvailableCommand, MatterSummary, ThreadId } from "../../transport/protocol.ts";
import type { Conversations } from "../conversations.ts";
import { nextHostOf } from "../host-next.ts";
import { KeyedRequestContext, keyedRequest } from "../permission-keyed.ts";
import { refillOf, useBreakLines, useStoppedLine, useThreadBreak } from "../thread-break.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { LazyThreadList, loadThread } from "./chunk.ts";
import { pausedPick } from "./limit-words.ts";
import { useBusy } from "./live.ts";
import { doingPick } from "./step-names.ts";
import {
  ThreadActionsContext,
  useRefused,
  useThreadSession,
  type AskHow,
  type ConversationLink,
  type Drafts,
} from "./session.ts";
import { paperMentions } from "./paper-mentions.ts";
import { WorkComposer } from "./work-composer.tsx";

const NO_COMMANDS: readonly AvailableCommand[] = [];

const sameCommand = (a: AvailableCommand | undefined, b: AvailableCommand) =>
  a !== undefined &&
  a.name === b.name &&
  a.description === b.description &&
  a.input_hint === b.input_hint;

/** The header's commands, the same array for as long as they read the same: a header that changes for its status or
 * its usage, as it does through a turn, re-renders nothing here. */
function commandsPick(): (v: StreamView | undefined) => readonly AvailableCommand[] {
  let last = NO_COMMANDS;
  return (v) => {
    const next = v?.header?.header?.commands ?? NO_COMMANDS;
    if (next.length !== last.length || next.some((c, i) => !sameCommand(last[i], c))) last = next;
    return last;
  };
}

/** The work column of the room: the matter's thread, and the composer (disabled with no matter). */
export function Work({
  transport,
  matter,
  drafts,
  draftKey,
  conversation,
  choice,
  plan = "",
  focus = null,
  onThread,
  conversations,
}: {
  transport: HostTransport;
  matter: MatterSummary | null;
  /** The room's composer drafts (room.tsx keeps them above Work's key). */
  drafts: Drafts;
  /** The open conversation's key in the drafts; null with no matter open. */
  draftKey: string | null;
  /** The open conversation: its thread, or how its first Ask makes one. */
  conversation: ConversationLink;
  /** A new conversation's plan choice, before its first question. */
  choice?: PlanChoice | undefined;
  /** The status line's plan words ("Uses your Claude plan"), "" for none. */
  plan?: string;
  /** A request for the field's focus (New conversation, or a conversation pressed): each new number takes it. */
  focus?: number | null;
  /** Hears the open thread: null until there is one. */
  onThread?: (thread: ThreadId | null) => void;
  /** The matter's conversations, for the wait while another of them holds the matter (U7); none, no wait. */
  conversations?: Conversations | undefined;
}) {
  const store = useViewStore();
  const field = useRef<ComposerHandle>(null);
  useEffect(() => {
    if (focus !== null) field.current?.focus();
  }, [focus]);
  const session = useThreadSession(
    transport,
    store,
    matter?.matter ?? null,
    conversation,
    draftKey,
  );
  const stream = session.actions.stream;
  useWatch(store, stream);
  const busy = useBusy(stream, session.sentTurn);
  const failed = useStreamError(store, stream);
  // the agent's commands for the composer's `/` menu, as the thread's header has them
  const [pickCommands] = useState(commandsPick);
  const commands = useStreamPart(store, stream, pickCommands);
  // U12: the thread paused at the plan's limit, for the composer's status line
  const [pickPaused] = useState(pausedPick);
  const paused = useStreamPart(store, stream, pickPaused);
  // U10: an answer Claude stopped giving, and Restart once the host has it (H10)
  const { lang } = useWords();
  // what the running step is doing, for the working line in place of "Answering."
  const pickDoing = useMemo(() => doingPick(lang), [lang]);
  const doing = useStreamPart(store, stream, pickDoing);
  const { brk, live } = useThreadBreak(store, stream);
  const restart = useMemo(() => nextHostOf(transport).restart, [transport]);
  const lines = useBreakLines();
  const agentPlan = useAgentPlan();
  const stopped = useStoppedLine({
    lang,
    brk,
    live,
    busy,
    thread: session.thread,
    restart,
    lines,
    plan: agentPlan,
  });
  const retry = () => {
    if (stream !== null) store.retry(stream);
  };
  // the one prompt that takes the keys, read here once a change rather than by every row
  const keyed = useStreamPart(store, stream, keyedRequest);
  useEffect(() => onThread?.(session.thread), [onThread, session.thread]);
  // U7: a refused Stop the other, by its code, on the composer's line until the next press or Ask
  const [otherRefused, refuseOther] = useRefused();
  const ask = (text: string, how?: AskHow) => {
    refuseOther(null);
    return session.ask(text, how);
  };
  // the matter's papers for the composer's `@`, read from the host only once `@` first opens its menu
  const matterId = matter?.matter ?? null;
  const mentions = useMemo(() => paperMentions(transport, matterId), [transport, matterId]);
  // the thread's chunk, in memory before the first view or the first answer, with a matter or without
  useEffect(() => void loadThread(), []);
  return (
    <>
      <ThreadActionsContext value={session.actions}>
        {/* the list's box, holding Thread, whose .room-thread is the scroller inside it; below 1000 px it takes the
            thread's place in the one column (responsive.css orders the thread first) */}
        <div data-testid="transcript" className="flex min-h-0 flex-1 flex-col max-mid:order-1">
          {/* empty until the thread's chunk is in: its arrival is one commit */}
          <Suspense fallback={null}>
            <KeyedRequestContext value={keyed}>
              <LazyThreadList transport={transport} stream={stream} busy={busy} />
            </KeyedRequestContext>
          </Suspense>
        </div>
      </ThreadActionsContext>
      <WorkComposer
        ref={field}
        drafts={drafts}
        draftKey={draftKey}
        choice={choice}
        plan={plan}
        commands={commands}
        mentions={mentions}
        disabled={matter === null}
        busy={busy}
        doing={doing}
        refused={session.refused ?? otherRefused}
        failed={
          failed === null
            ? null
            : lines !== null
              ? lines.streamFailedLine(lang, failed, retry, agentPlan)
              : // the thread's chunk never came in (chunk.ts's NoThread): the code's words, never no line
                {
                  state: {
                    kind: "wait",
                    words: errorWords(lang, failed.code, agentPlan),
                    pill: null,
                    code: failed.code,
                  },
                  testId: "stream-failed",
                }
        }
        stopped={stopped}
        refill={refillOf(brk, live, restart)}
        onAsk={ask}
        onStop={session.stop}
        paused={paused}
        others={
          conversations === undefined ? null : { transport, conversations, onRefused: refuseOther }
        }
        transport={transport}
        stream={stream}
        thread={session.thread}
      />
    </>
  );
}
