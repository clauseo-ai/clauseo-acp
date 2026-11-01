// Scripted hosts for the review's room states: a FakeTransport that answers list_matters, and, where a state needs the
// wall, H8's list_papers (room/host-next.ts), as room.test.tsx and the probe script them; the rail in every state; and
// hosts that refuse a new matter with a code, for the form's lines. With the steps a state takes once rendered
// (states.tsx's RoomAfter): New matter pressed, a name typed, Change and Create pressed.
import type { MatterSummary } from "../transport/protocol.ts";
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
