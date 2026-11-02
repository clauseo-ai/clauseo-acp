// Scripted hosts for the review's room states: a FakeTransport that answers list_matters, and, where a state needs the
// wall, H8's list_papers (room/host-next.ts), as room.test.tsx and the probe script them; the rail in every state;
// hosts that refuse a new matter with a code, for the form's lines, with the steps a state takes once rendered
// (states.tsx's RoomAfter): New matter pressed, a name typed, Change and Create pressed; and for the thread's states,
// one thread whose view the host sends as the room subscribes (testing/stream-script.ts's entries), and whose answers to
// a permission request may never come back.
import { batch, event, header, reset, THREAD, TURN } from "../testing/stream-script.ts";
import type {
  CommandOutcome,
  ErrorCode,
  MatterSummary,
  ThreadStatus,
  ViewEntry,
} from "../transport/protocol.ts";
import { FakeTransport, type FakeHost } from "../transport/fake-transport.ts";
import type { NextErrorCode, NextHostError, PaperSummary } from "../room/host-next.ts";
import { detailed, fakeNext, railStates } from "../room/fake-next.ts";

/** A matter as filed, for states that show one. */
export const MATTER: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000001",
  name: "Acme Traders v. State",
  kind: "folder",
};

const SCRATCH: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000099",
  name: "Scratch",
  kind: "scratch",
};

/** A host with these matters; with papers, also H8's list of the matter's papers (and the matters' details). */
export function roomHost(matters: MatterSummary[], papers?: PaperSummary[]): FakeTransport {
  const host: FakeHost = {
    query: (query) => {
      if (query === "list_matters") return { matters };
      throw new Error(`the review's host answers list_matters only, not ${JSON.stringify(query)}`);
    },
  };
  if (papers !== undefined)
    host.next = fakeNext({ matters: () => matters.map((m) => detailed(m)), papers });
  return new FakeTransport(host);
}

/** A thread's state for the review: its entries and status, and what the host refuses or loses. */
export type ThreadState = {
  entries: ViewEntry[];
  status?: ThreadStatus;
  /** send_prompt is refused with this code. */
  refuse?: ErrorCode;
  /** The stream is lost with this code once its view has arrived. */
  lose?: ErrorCode;
  /** An answer to a permission request never comes back, as while the OS asks the lawyer to confirm it. */
  hold?: boolean;
};

const refusal = (code: ErrorCode) => ({
  code,
  message: "the review's scripted refusal",
  retryable: false,
});

/** A host with one matter and its one thread, whose view arrives as the room subscribes. */
export function threadHost(state: ThreadState): FakeTransport {
  const head = header();
  head.header!.status = state.status ?? "idle";
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined)
        return {
          threads: [{ thread: THREAD, matter: MATTER.matter, agent: "claude", status: "idle" }],
        };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    dispatch: (envelope): CommandOutcome => {
      const command = envelope.command;
      if (command.send_prompt !== undefined) {
        if (state.refuse !== undefined) throw refusal(state.refuse);
        return { prompt_accepted: { turn: TURN, message: THREAD } };
      }
      return "done";
    },
    subscribe: (stream) =>
      queueMicrotask(() => {
        transport.deliver(event(batch(1, [reset(state.entries, head)], undefined, stream)));
        // lost after the view has applied (a frame, or the store's 100 ms catch-up), which would clear it
        const lose = state.lose;
        if (lose !== undefined) setTimeout(() => transport.failStream(stream, refusal(lose)), 250);
      }),
  });
  if (state.hold === true) transport.answerPermission = () => new Promise<void>(() => {});
  return transport;
}

/** A NextHost whose matters show the scratch matter open (opened last), with a folder matter beside it. */
export const scratchHost = (): FakeTransport =>
  new FakeTransport({
    next: fakeNext({ matters: () => [detailed(MATTER), detailed(SCRATCH, { last_opened: 1 })] }),
  });

/** A NextHost whose matters show every rail state, in every date group, one pinned (room/fake-next.ts). */
export const railHost = (): FakeTransport =>
  new FakeTransport({ next: fakeNext({ matters: () => railStates(MATTER, SCRATCH, Date.now()) }) });

/** A host that refuses create_matter with `code`, its picker giving `display`. With `next`, a NextHost with the
 * default location "Documents/Clauseo", so Create sends default_location and the line names no folder. */
export function refusingHost(code: NextErrorCode, display = "", next = false): FakeTransport {
  const refusal: NextHostError = { code, message: "a detail for logs", retryable: false };
  const refuse = () => {
    throw refusal;
  };
  if (next)
    return new FakeTransport({
      next: fakeNext({
        matters: () => [detailed(MATTER)],
        location: "Documents/Clauseo",
        command: (command) => ("create_matter" in command ? refuse() : undefined),
      }),
    });
  return new FakeTransport({
    query: () => ({ matters: [MATTER] }),
    dispatch: refuse,
    pickFolder: () => ({ folder: "review-folder", display }),
  });
}

/** One thing a state does once rendered: press the control with this test id, type into a field of the dialog (by its
 * place among the dialog's fields: 0 the name, 1 the client, 2 the next date) and move on from it, or type into the
 * rail's Find. */
export type Step = { press: string } | { type: { field: number; text: string } } | { find: string };

/** New matter pressed and the name typed. */
export const NEW_MATTER: readonly Step[] = [
  { press: "new-matter" },
  { type: { field: 0, text: "Mehta v. Mehta" } },
];
