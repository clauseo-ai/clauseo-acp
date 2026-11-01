// The open thread of the room's work column and what the lawyer does in it: which thread is open (the newest that is
// not archived, store/reopen.ts), and the commands Ask, Stop and Undo send. With no thread yet, the first Ask makes
// one, so a visit leaves no empty thread behind. A refused command shows by its code only, never its message.
import { createContext, use, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { pickReopenThread } from "../../store/reopen.ts";
import { streamOf } from "../../store/view.ts";
import { internalError, toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type {
  ErrorCode,
  HostCommand,
  MatterId,
  StepEntry,
  StreamId,
  ThreadId,
  TurnId,
} from "../../transport/protocol.ts";

/** The agent every thread the room starts runs: `agents.claude` in the user settings, until U6 lets the lawyer choose
 * (as R12's internal window, internal/session.ts). */
export const AGENT = "claude";

/** What the rows can do and need to know: Undo, and the turn sent last (busy until its end is in the view). */
export type ThreadActions = {
  readonly stream: StreamId | null;
  readonly sentTurn: TurnId | null;
  /** Undoes every file of a step; rejects with the HostError when the host refuses. */
  undo: (step: StepEntry) => Promise<void>;
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

export function useThreadSession(transport: HostTransport, matter: MatterId | null): ThreadSession {
  const [thread, setThread] = useState<ThreadId | null>(null);
  const [sentTurn, setSentTurn] = useState<TurnId | null>(null);
  const [refused, setRefused] = useState<ErrorCode | null>(null);
  // the refs hold what an Ask in flight reads: two Enters in one frame must make one thread
  const open = useRef<ThreadId | null>(null);
  const lookup = useRef<Promise<ThreadId | null> | null>(null);
  const sending = useRef(false);

  const adopt = useCallback((id: ThreadId) => {
    open.current = id;
    setThread(id);
  }, []);

  const command = useCallback(
    (body: HostCommand) => transport.dispatch({ command_id: crypto.randomUUID(), command: body }),
    [transport],
  );

  /** The thread to reopen, once: a failed lookup is asked again by the next Ask. */
  const find = useCallback((): Promise<ThreadId | null> => {
    if (matter === null) return Promise.resolve(null);
    if (lookup.current !== null) return lookup.current;
    const asked = transport.query({ list_threads: { matter } }).then((result) => {
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
    [matter, find, command, adopt],
  );

  const stop = useCallback(() => {
    const id = open.current;
    if (id === null) return;
    command({ cancel: { thread: id } }).catch((e: unknown) => setRefused(toHostError(e).code));
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

  const stream = thread === null ? null : streamOf(thread);
  const actions = useMemo(() => ({ stream, sentTurn, undo }), [stream, sentTurn, undo]);
  return { thread, sentTurn, refused, ask, stop, actions };
}
