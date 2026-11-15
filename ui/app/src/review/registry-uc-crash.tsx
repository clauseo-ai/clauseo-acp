// UC-326 to UC-335 and UC-365 to UC-367: the notice after Clauseo closed unexpectedly (room/crash-notice.tsx), in the
// real room over a host whose `last_exit` says the last run ended without its quit: with nothing unfinished, with an
// undo and a counted restore in a matter, with one file each and a whole-matter restore, with tracked changes, and
// with an undo in the Scratch matter, which is never named. Every entry is provisional, its Hindi too, until Rohan's
// review; "Got it" is the update notice's (R23).
import { uw, type UpdateKey } from "@clauseo/components/lib/words-update.ts";
import type { ReactNode } from "react";
import { detailed, fakeNext } from "../room/fake-next.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  InterruptedAction,
  LastExit,
  MatterSummary,
  QueryResult,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import type { WordsRef } from "./words.ts";

/** A string of the notice's words (words-update.ts). */
const words = (key: UpdateKey): WordsRef => ({
  file: "lib/words-update.ts",
  key,
  text: (lang) => uw(lang, key),
  later: uw.hi[key] === null,
});

const SCRATCH: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000099",
  name: "Scratch",
  kind: "scratch",
};

const at = (
  matter: MatterSummary,
  kind: InterruptedAction["kind"],
  files?: number,
): InterruptedAction => ({
  kind,
  matter: matter.matter,
  ...(files === undefined ? {} : { files }),
});

const STATES = {
  general: [],
  counted: [at(MATTER, "undo", 3), at(MATTER, "restore", 2)],
  single: [at(MATTER, "undo", 1), at(MATTER, "restore", 1), at(MATTER, "restore")],
  tracked: [at(MATTER, "accept_changes", 1), at(MATTER, "reject_changes", 1)],
  scratch: [at(SCRATCH, "undo", 2)],
} satisfies Record<string, InterruptedAction[]>;

type State = keyof typeof STATES;

/** The room over a host whose last run ended unclean with `state`'s actions unfinished. */
const roomOf = (state: State, started: number): ReactNode => (
  <RoomOver
    host={() => {
      const transport = new FakeTransport({
        next: fakeNext({ matters: () => [detailed(MATTER), detailed(SCRATCH)], frames: true }),
      });
      const answer: LastExit = { unclean: true, run_started: started, interrupted: STATES[state] };
      const query = transport.query.bind(transport);
      Object.assign(transport, { lastExitWire: true });
      transport.query = async (asked): Promise<QueryResult> =>
        asked === "last_exit" ? { last_exit: answer } : await query(asked);
      return transport;
    }}
  />
);

const ORDER: readonly State[] = ["general", "counted", "single", "tracked", "scratch"];

const notice = (id: string, key: UpdateKey, state: State): Entry => ({
  id,
  words: words(key),
  screen: "room/crash-notice",
  width: 1280,
  // a run start of its own per state, so Got it pressed on one leaves the others showing
  render: () => roomOf(state, Date.UTC(2026, 10, 15, 9, ORDER.indexOf(state))),
});

export const UC_CRASH: readonly Entry[] = [
  // the card's name for assistive technology
  notice("UC-326", "Last session", "general"),
  notice("UC-327", "Clauseo closed unexpectedly.", "general"),
  notice("UC-328", "Your conversations are kept.", "general"),
  notice("UC-329", "{action} in {matter} may not have finished.", "counted"),
  notice("UC-330", "{action} may not have finished.", "scratch"),
  notice("UC-331", "Undo of 1 file", "single"),
  notice("UC-332", "Undo of {count} files", "counted"),
  notice("UC-333", "Restore of 1 file", "single"),
  notice("UC-334", "Restore of {count} files", "counted"),
  // a whole-matter restore, whose count is not known before it starts
  notice("UC-335", "A restore", "single"),
  notice("UC-365", "Accepting tracked changes", "tracked"),
  notice("UC-366", "Rejecting tracked changes", "tracked"),
  notice("UC-367", "Versions has the earlier copies.", "counted"),
];
