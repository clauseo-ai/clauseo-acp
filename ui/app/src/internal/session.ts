// The internal window's state and actions: which matter and thread are open, the last error, and the commands the
// window sends. On start it reopens the matter with the latest activity and its last thread (store/reopen.ts).
import { createContext, use, useCallback, useEffect, useRef, useState } from "react";
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type {
  AgentChoiceId,
  HostCommand,
  HostError,
  MatterId,
  PermissionRequestId,
  StepEntry,
  StreamId,
  ThreadId,
  ThreadSummary,
  TurnId,
} from "../transport/protocol.ts";
import { pickReopenMatter, pickReopenThread } from "../store/reopen.ts";

/** The agent every thread of the internal window starts with: `agents.claude` in the user settings. */
export const AGENT_ID = "claude";

export type OpenMatter = { readonly id: MatterId; readonly name: string };

/** What the rows of an open thread can do. */
export type ThreadActions = {
  readonly thread: ThreadId;
  readonly stream: StreamId;
  answer: (request: PermissionRequestId, option: AgentChoiceId) => void;
  undo: (step: StepEntry) => void;
};

export const ThreadContext = createContext<ThreadActions | null>(null);

export function useThreadActions(): ThreadActions {
  const actions = use(ThreadContext);
  if (actions === null) throw new Error("no ThreadContext above this row");
  return actions;
}

export type Session = {
  readonly matter: OpenMatter | null;
  readonly thread: ThreadId | null;
  readonly threads: readonly ThreadSummary[];
  readonly error: HostError | null;
  /** The turn of the last prompt sent, until its step or limit stop arrives. */
  readonly sentTurn: TurnId | null;
  /** Whether the start-up reopen is still running. */
  readonly opening: boolean;
  newMatter: (name: string) => void;
  newThread: () => void;
  openThread: (thread: ThreadId) => void;
  send: (text: string) => Promise<boolean>;
  cancel: () => void;
  answer: (request: PermissionRequestId, option: AgentChoiceId) => void;
  undo: (step: StepEntry) => void;
  clearSent: () => void;
};

export function useSession(transport: HostTransport): Session {
  const [matter, setMatter] = useState<OpenMatter | null>(null);
  const [thread, setThread] = useState<ThreadId | null>(null);
  const [threads, setThreads] = useState<readonly ThreadSummary[]>([]);
  const [error, setError] = useState<HostError | null>(null);
  const [sentTurn, setSentTurn] = useState<TurnId | null>(null);
  const [opening, setOpening] = useState(true);
  const started = useRef(false);

  const fail = useCallback((e: unknown) => setError(toHostError(e)), []);

  const command = useCallback(
    (body: HostCommand) => {
      setError(null);
      return transport.dispatch({ command_id: crypto.randomUUID(), command: body });
    },
    [transport],
  );

  const listThreads = useCallback(
    async (id: MatterId): Promise<ThreadSummary[]> => {
      const result = await transport.query({ list_threads: { matter: id } });
      const list = result.threads ?? [];
      setThreads(list);
      return list;
    },
    [transport],
  );

  const createThread = useCallback(
    async (id: MatterId) => {
      const outcome = await command({ create_thread: { matter: id, agent: AGENT_ID } });
      if (typeof outcome === "string" || outcome.thread_created === undefined) return;
      setSentTurn(null);
      setThread(outcome.thread_created.thread);
      await listThreads(id);
    },
    [command, listThreads],
  );

  useEffect(() => {
    // Strict Mode runs this effect twice on one instance; the ref keeps the reopen to one.
    if (started.current) return;
    started.current = true;
    (async () => {
      const result = await transport.query("list_matters");
      const pick = pickReopenMatter(result.matters ?? []);
      if (pick === null) return;
      setMatter({ id: pick.matter, name: pick.name });
      const last = pickReopenThread(await listThreads(pick.matter));
      if (last !== null) setThread(last.thread);
      else await createThread(pick.matter);
    })()
      .catch(fail)
      .finally(() => setOpening(false));
  }, [transport, listThreads, createThread, fail]);

  return {
    matter,
    thread,
    threads,
    error,
    sentTurn,
    opening,
    newMatter: (name) => {
      (async () => {
        setError(null);
        const folder = await transport.pickFolder();
        if (folder === null) return;
        const outcome = await command({
          create_matter: { name, source: { existing_folder: { folder } } },
        });
        if (typeof outcome === "string" || outcome.matter_created === undefined) return;
        const id = outcome.matter_created.matter;
        setMatter({ id, name });
        setThread(null);
        setThreads([]);
        await createThread(id);
      })().catch(fail);
    },
    newThread: () => {
      if (matter !== null) createThread(matter.id).catch(fail);
    },
    openThread: (id) => {
      setSentTurn(null);
      setThread(id);
    },
    send: async (text) => {
      if (thread === null) return false;
      try {
        const outcome = await command({ send_prompt: { thread, content: [{ text: { text } }] } });
        if (typeof outcome !== "string" && outcome.prompt_accepted !== undefined) {
          setSentTurn(outcome.prompt_accepted.turn);
        }
        return true;
      } catch (e) {
        fail(e);
        return false;
      }
    },
    cancel: () => {
      if (thread !== null) command({ cancel: { thread } }).catch(fail);
    },
    answer: (request, option) => {
      if (thread === null) return;
      setError(null);
      transport.answerPermission(thread, request, option).catch(fail);
    },
    undo: (step) => {
      if (thread === null) return;
      const files = step.files.map((f) => ({ path: f.path, choice: "undo" as const }));
      command({ undo_step: { thread, turn: step.turn, files } }).catch(fail);
    },
    clearSent: () => setSentTurn(null),
  };
}
