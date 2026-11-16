// The first read's start (UD2, its own lazy chunk, loaded by the first run): once, for a matter added during the first
// run, with at least one paper, once its plan is signed in (and, after UC, once the Safe workspace step has an outcome;
// first-run.tsx holds that). A matter that already has a "First read" conversation starts nothing. Then
// `create_thread`, `rename_thread` to "First read", and `send_prompt` with the fixed prompt; the accepted turn is
// marked (marks.ts). Restart is the same prompt sent again in the same thread, until H10's RestartAgent lands.
import { frd } from "@clauseo/components/lib/words-first-read.ts";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, HostCommand, MatterId, ThreadId } from "../../transport/protocol.ts";
import type { Matters } from "../matters.ts";
import { firstReadMarks, markThread, markTurn } from "./marks.ts";
import { FIRST_READ_PROMPT } from "./prompt.ts";

/** The thread's title, in English as the host keeps it. */
export const FIRST_READ_TITLE = frd("en", "First read");

/** Matters whose start runs now: a second signed_in meanwhile starts nothing more. */
const starting = new Set<MatterId>();

const send = (transport: HostTransport, command: HostCommand) =>
  transport.dispatch({ command_id: crypto.randomUUID(), command });

/** Sends the prompt in `thread` and marks its turn. True once accepted. */
export async function askFirstRead(
  transport: HostTransport,
  matter: MatterId,
  thread: ThreadId,
): Promise<boolean> {
  const sent = await send(transport, {
    send_prompt: { thread, content: [{ text: { text: FIRST_READ_PROMPT } }] },
  });
  if (typeof sent === "string" || sent.prompt_accepted === undefined) return false;
  markTurn(matter, sent.prompt_accepted.turn);
  return true;
}

/** The read's Restart: the same prompt again in its thread (RestartAgent is refused on main). */
export const restartFirstRead = askFirstRead;

/** Starts the read of `matter` with `agent`, if it should start; never twice. Gives the read's thread once made, else
 * null. Refusals go to the console. */
export async function startFirstRead(
  transport: HostTransport,
  matters: Matters,
  matter: MatterId,
  agent: AgentId,
): Promise<ThreadId | null> {
  const mark = firstReadMarks()[matter];
  if (mark?.added !== true || mark.thread !== undefined || starting.has(matter)) return null;
  const { papers, matters: rows } = matters.next;
  if (papers === null) return null;
  starting.add(matter);
  try {
    const listed = (await papers.listPapers(matter)).papers.length;
    if (listed === 0) return null;
    const threads =
      rows !== null
        ? await rows.listThreads(matter)
        : ((await transport.query({ list_threads: { matter } })).threads ?? []);
    const had = threads.find((t) => t.title === FIRST_READ_TITLE);
    if (had !== undefined) {
      markThread(matter, had.thread);
      return null;
    }
    const made = await send(transport, { create_thread: { matter, agent } });
    if (typeof made === "string" || made.thread_created === undefined) return null;
    const thread = made.thread_created.thread;
    markThread(matter, thread, listed);
    await send(transport, { rename_thread: { thread, title: FIRST_READ_TITLE } });
    await askFirstRead(transport, matter, thread);
    return thread;
  } catch (error) {
    console.warn(`the first read didn't start: ${JSON.stringify(toHostError(error).code)}`);
    return null;
  } finally {
    starting.delete(matter);
  }
}
