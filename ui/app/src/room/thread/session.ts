// The open conversation's thread in the room's work column and what the lawyer does in it: the commands Ask, Stop and
// Undo send. Which thread is open is the conversations' (conversations.ts), which also make the thread on a draft's
// first Ask and title it once the question is accepted, so a visit leaves no empty thread behind. A refused command
// shows by its code only, never its message.
import { createContext, use, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ViewStore } from "../../store/store.ts";
import { streamOf } from "../../store/view.ts";
import { internalError, toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type {
  ErrorCode,
  HostCommand,
  MatterId,
  MessageId,
  PermissionRequestId,
  StepEntry,
  StreamHeader,
  StreamId,
  ThreadId,
  Timestamp,
  TurnId,
} from "../../transport/protocol.ts";
import { queueCountPick } from "../queue-entries.ts";
import { turnRunning } from "./live.ts";
import { loadThread } from "./chunk.ts";

/** The code of a refused command, and its setter, which makes no set to the code already held: React drops such a
 * set at once but keeps it queued at its lane (Ask's click: sync), and the next render of Work at another lane (a
 * reply's) skips it and renders Work once more straight after, a second commit in the reply's frame (#253). */
export function useRefused(): [ErrorCode | null, (code: ErrorCode | null) => void] {
  const [refused, setRefused] = useState<ErrorCode | null>(null);
  const now = useRef<ErrorCode | null>(null);
  const refuse = useCallback((code: ErrorCode | null) => {
    if (now.current === code) return;
    now.current = code;
    setRefused(code);
  }, []);
  return [refused, refuse];
}

/** Each conversation's words in the composer's field, by its key (conversations.ts), kept above the room's Work, so
 * leaving a conversation or its matter and coming back finds them where the lawyer left them (a refused send promises
 * "Your question is kept."). */
export type Drafts = Map<string, string>;

/** What the session needs of the open conversation (conversations.ts). */
export type ConversationLink = {
  /** Its thread: null for a draft, or while the matter's list loads. */
  thread: ThreadId | null;
  /** The thread a send goes to when `thread` is null, for the conversation with this key (the session's own): the one
   * the list opens, else a new one. Rejects with the host's error. */
  create: (key: string) => Promise<ThreadId>;
  /** A prompt was accepted in the thread. */
  sent: (thread: ThreadId, text: string) => void;
  /** The code of the matter's refused list, until a read succeeds. */
  refused: ErrorCode | null;
  /** H13, where the host has it: a new conversation from one of the lawyer's questions in `thread`, which opens. */
  retry?: (thread: ThreadId, message: MessageId, at: Timestamp) => Promise<void>;
};

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
  /** H13's Retry from here on one of the lawyer's questions, at its time; absent where the host can't. Rejects with
   * the HostError when the host refuses. */
  retry?: ((message: MessageId, at: Timestamp) => Promise<void>) | undefined;
};

export const ThreadActionsContext = createContext<ThreadActions | null>(null);

export function useThreadActions(): ThreadActions {
  const actions = use(ThreadActionsContext);
  if (actions === null) throw new Error("no ThreadActionsContext above this row");
  return actions;
}

/** How a question goes (U7, H9): `behind`, sent while Clauseo works, so the host queues it (send_prompt with no
 * `delivery`) or, with `delivery: "steer"`, gives it to the running answer or stops that answer for it. A question
 * that goes behind a turn, or into a held or waiting queue, is not the sent turn: busy follows the status for it. */
export type AskHow = { behind?: boolean; delivery?: "steer" };

export type ThreadSession = {
  /** The open thread, or null before the first Ask makes one, while list_threads runs, or before the thread's chunk is
   * in. */
  readonly thread: ThreadId | null;
  /** The turn of the last prompt accepted. */
  readonly sentTurn: TurnId | null;
  /** The code of the last refused send or Stop, until the next Ask; else the matter's refused list. */
  readonly refused: ErrorCode | null;
  /** Sends a question, making the thread first if there is none. True once send_prompt is accepted. Never rejects. */
  ask: (text: string, how?: AskHow) => Promise<boolean>;
  stop: () => void;
  actions: ThreadActions;
};

/** Whether the thread's chunk is in memory: its import is tagged fulfilled once it is (chunk.ts). */
const chunkIn = (): boolean => (loadThread() as { status?: string }).status === "fulfilled";

export function useThreadSession(
  transport: HostTransport,
  store: ViewStore,
  matter: MatterId | null,
  link: ConversationLink,
  /** The open conversation's key (conversations.ts), which a first send's thread is made for. */
  key: string | null,
): ThreadSession {
  // the thread opens only once the chunk is in, so its rows never render, settle or page before it (chunk.ts); once
  // it is, a conversation opens on its first render
  const [loaded, setLoaded] = useState(chunkIn);
  const [made, setMade] = useState<ThreadId | null>(null);
  const [sentTurn, setSentTurn] = useState<TurnId | null>(null);
  const [refused, refuse] = useRefused();
  const known = link.thread ?? made;
  const thread = loaded ? known : null;
  // the refs hold what an Ask in flight reads: two Enters in one frame must make one thread
  const open = useRef<ThreadId | null>(known);
  const sending = useRef(false);
  const stopping = useRef(false);
  /** The view's header as the last prompt went: its turn is over once a later header says nothing runs. */
  const headerAtSend = useRef<StreamHeader | null>(null);
  const { create, sent: onSent, retry: retryFrom } = link;

  // a thread the conversations open (the list's, or one made elsewhere) is the one the commands go to
  useEffect(() => {
    open.current = known;
  }, [known]);

  useEffect(() => {
    if (loaded) return;
    let live = true;
    void loadThread().then(() => {
      if (live) setLoaded(true);
    });
    return () => {
      live = false;
    };
  }, [loaded]);

  const command = useCallback(
    (body: HostCommand) => transport.dispatch({ command_id: crypto.randomUUID(), command: body }),
    [transport],
  );

  const ask = useCallback(
    async (text: string, how: AskHow = {}): Promise<boolean> => {
      if (matter === null || key === null || sending.current) return false;
      sending.current = true;
      refuse(null);
      try {
        let id = open.current;
        if (id === null) {
          id = await create(key);
          // the conversations' emit has opened it already (Room re-rendered, `known`'s effect set open.current): a
          // set here would be a second commit in the reply's frame (#253)
          if (open.current !== id) setMade(id);
          open.current = id;
        }
        const view = store.view(streamOf(id));
        // only a question sent while nothing runs or waits is the sent turn: one queued behind the running turn, or
        // behind a held queue's messages, would hold busy past the running turn's end (live.ts, useBusy)
        const track =
          how.behind !== true &&
          how.delivery === undefined &&
          view?.header?.header?.queue_held !== true &&
          queueCountPick(view) === 0;
        const header = view?.header ?? null;
        const content = [{ text: { text } }];
        const sent = await command({
          send_prompt: {
            thread: id,
            content,
            ...(how.delivery === undefined ? {} : { delivery: how.delivery }),
          },
        });
        if (track && typeof sent !== "string" && sent.prompt_accepted !== undefined) {
          headerAtSend.current = header;
          setSentTurn(sent.prompt_accepted.turn);
        }
        onSent(id, text);
        return true;
      } catch (e) {
        refuse(toHostError(e).code);
        return false;
      } finally {
        sending.current = false;
      }
    },
    [matter, key, create, onSent, command, store, refuse],
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
      .catch((e: unknown) => refuse(toHostError(e).code))
      .finally(() => {
        stopping.current = false;
      });
  }, [command, refuse]);

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

  const retry = useMemo(
    () =>
      retryFrom === undefined
        ? undefined
        : async (message: MessageId, at: Timestamp) => {
            const id = open.current;
            if (id === null) throw internalError("no thread is open");
            try {
              await retryFrom(id, message, at);
            } catch (e) {
              throw toHostError(e);
            }
          },
    [retryFrom],
  );

  const stream = thread === null ? null : streamOf(thread);
  const actions = useMemo(
    () => ({ stream, sentTurn, undo, answer, stop, retry }),
    [stream, sentTurn, undo, answer, stop, retry],
  );
  return { thread, sentTurn, refused: refused ?? link.refused, ask, stop, actions };
}
