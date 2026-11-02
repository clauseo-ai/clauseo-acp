// The open thread of the room's work column and what the lawyer does in it: which thread is open (the newest that is
// not archived, store/reopen.ts), and the commands Ask, Stop and Undo send. With no thread yet, the first Ask makes
// one, so a visit leaves no empty thread behind. A refused command shows by its code only, never its message.
import { createContext, use, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { pickReopenThread } from "../../store/reopen.ts";
import type { ViewStore } from "../../store/store.ts";
import { streamOf } from "../../store/view.ts";
import { internalError, toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type {
  ErrorCode,
  HostCommand,
  MatterId,
  PermissionRequestId,
  StepEntry,
  StreamHeader,
  StreamId,
  ThreadId,
  TurnId,
} from "../../transport/protocol.ts";
import { turnRunning } from "./live.ts";
import { loadThread } from "./chunk.ts";

/** The agent every thread the room starts runs: `agents.claude-acp` in the user settings, until U6 lets the lawyer choose
 * (as R12's internal window did). */
export const AGENT = "claude-acp";

/** Each matter's words in the composer's field, kept above the matter's Work, so leaving a matter and coming back
 * finds them where the lawyer left them (a refused send promises "Your question is kept."). */
export type Drafts = Map<MatterId, string>;

/** What the rows can do and need to know: Undo, and the turn sent last (busy until its end is in the view). */
export type ThreadActions = {
  readonly stream: StreamId | null;
  readonly sentTurn: TurnId | null;
  /** Undoes every file of a step; rejects with the HostError when the host refuses. */
  undo: (step: StepEntry) => Promise<void>;
  /** Answers a permission request with an option it offered; rejects with the HostError when the host refuses (an
   * allow_always declined at the native confirm: `transport.unauthorized`). */
  answer: (request: PermissionRequestId, option: string) => Promise<void>;
  /** Stop: cancels the open thread's turn. */
  stop: () => void;
};

export const ThreadActionsContext = createContext<ThreadActions | null>(null);

export function useThreadActions(): ThreadActions {
  const actions = use(ThreadActionsContext);
  if (actions === null) throw new Error("no ThreadActionsContext above this row");
  return actions;
}

export type ThreadSession = {
  /** The open thread, or null before the first Ask makes one (or while list_threads runs). */
  readonly thread: ThreadId | null;
  /** The turn of the last prompt accepted. */
  readonly sentTurn: TurnId | null;
  /** The code of the last refused list, send or Stop, until the next Ask. */
  readonly refused: ErrorCode | null;
  /** Sends a question, making the thread first if there is none. True once send_prompt is accepted. Never rejects. */
  ask: (text: string) => Promise<boolean>;
  stop: () => void;
  actions: ThreadActions;
};

export function useThreadSession(
  transport: HostTransport,
  store: ViewStore,
  matter: MatterId | null,
): ThreadSession {
  const [thread, setThread] = useState<ThreadId | null>(null);
  const [sentTurn, setSentTurn] = useState<TurnId | null>(null);
  const [refused, setRefused] = useState<ErrorCode | null>(null);
  // the refs hold what an Ask in flight reads: two Enters in one frame must make one thread
  const open = useRef<ThreadId | null>(null);
  const lookup = useRef<Promise<ThreadId | null> | null>(null);
  const sending = useRef(false);
  const stopping = useRef(false);
  /** The view's header as the last prompt went: its turn is over once a later header says nothing runs. */
  const headerAtSend = useRef<StreamHeader | null>(null);

  const adopt = useCallback((id: ThreadId) => {
    open.current = id;
    setThread(id);
  }, []);

  const command = useCallback(
    (body: HostCommand) => transport.dispatch({ command_id: crypto.randomUUID(), command: body }),
    [transport],
  );

  /** The thread to reopen, once: a failed lookup is asked again by the next Ask. The thread's chunk (the list, its rows and the
   * Markdown renderer, chunk.ts) loads alongside: the thread opens only once it is in, so no row renders, and the
   * thread never settles or pages, before it. */
  const find = useCallback((): Promise<ThreadId | null> => {
    if (matter === null) return Promise.resolve(null);
    if (lookup.current !== null) return lookup.current;
    const listed = transport.query({ list_threads: { matter } });
    const asked = Promise.all([listed, loadThread()]).then(([result]) => {
      const pick = pickReopenThread(result.threads ?? []);
      if (pick !== null && open.current === null) adopt(pick.thread);
      return open.current;
    });
    lookup.current = asked;
    asked.catch(() => {
      if (lookup.current === asked) lookup.current = null;
    });
    return asked;
  }, [transport, matter, adopt]);

  useEffect(() => {
    // Strict Mode runs this twice; find() asks once
    find().catch((e: unknown) => setRefused(toHostError(e).code));
  }, [find]);

  const ask = useCallback(
    async (text: string): Promise<boolean> => {
      if (matter === null || sending.current) return false;
      sending.current = true;
      setRefused(null);
      try {
        let id = open.current ?? (await find());
        if (id === null) {
          const made = await command({ create_thread: { matter, agent: AGENT } });
          if (typeof made === "string" || made.thread_created === undefined)
            throw internalError("create_thread answered without a thread");
          id = made.thread_created.thread;
          adopt(id);
        }
        headerAtSend.current = store.view(streamOf(id))?.header ?? null;
        const sent = await command({ send_prompt: { thread: id, content: [{ text: { text } }] } });
        if (typeof sent !== "string" && sent.prompt_accepted !== undefined)
          setSentTurn(sent.prompt_accepted.turn);
        return true;
      } catch (e) {
        setRefused(toHostError(e).code);
        return false;
      } finally {
        sending.current = false;
      }
    },
    [matter, find, command, adopt, store],
  );

  // The sent turn holds busy until its step or limit stop is in the view (live.ts, useBusy). A turn can end without
  // either reaching the view: so it is also over once a header after the send (an update, or a Reset's) says nothing
  // runs, and when the stream fails.
  useEffect(() => {
    if (sentTurn === null || thread === null) return;
    const stream = streamOf(thread);
    const check = () => {
      const header = store.view(stream)?.header ?? null;
      const ended =
        store.error(stream) !== undefined ||
        (header !== headerAtSend.current && !turnRunning(header?.header?.status));
      if (ended) setSentTurn(null);
    };
    check();
    return store.subscribe(check);
  }, [store, sentTurn, thread]);

  const stop = useCallback(() => {
    const id = open.current;
    // one cancel at a time: a second press while the first is on its way sends nothing
    if (id === null || stopping.current) return;
    stopping.current = true;
    command({ cancel: { thread: id } })
      .catch((e: unknown) => setRefused(toHostError(e).code))
      .finally(() => {
        stopping.current = false;
      });
  }, [command]);

  const undo = useCallback(
    async (step: StepEntry) => {
      const id = open.current;
      if (id === null) throw internalError("no thread is open");
      const files = step.files.map((f) => ({ path: f.path, choice: "undo" as const }));
      try {
        await command({ undo_step: { thread: id, turn: step.turn, files } });
      } catch (e) {
        throw toHostError(e);
      }
    },
    [command],
  );

  const answer = useCallback(
    async (request: PermissionRequestId, option: string) => {
      const id = open.current;
      if (id === null) throw internalError("no thread is open");
      try {
        await transport.answerPermission(id, request, option);
      } catch (e) {
        throw toHostError(e);
      }
    },
    [transport],
  );

  const stream = thread === null ? null : streamOf(thread);
  const actions = useMemo(
    () => ({ stream, sentTurn, undo, answer, stop }),
    [stream, sentTurn, undo, answer, stop],
  );
  return { thread, sentTurn, refused, ask, stop, actions };
}
