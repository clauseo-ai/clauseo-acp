// R18's copy review entries: the denial cards in the real room over FakeTransport, the thread's view holding the
// denials as the reducer gives them (transport/fake-denials.ts): a folder, a file in another matter, a site and a
// shared-hosting site waiting; the dialog up (Allow pressed, the dialog never answering); allowed, allowed while the
// agent restarts, refused, dismissed and ended; what's never offered, for each kind; the network's sign-in; the paused
// answer; and each refusal said on the card, after the press that leads there (PressInOrder). The refusals' words are
// U3-err-<code>'s, entered again here for the card that shows them.
import { dn, type DenialKey } from "@clauseo/components/lib/words-denials.ts";
import { ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import type { ReactNode } from "react";
import { agentText, TURN, userMessage } from "../testing/stream-script.ts";
import {
  DENIAL_KINDS,
  denialEntry,
  pausedEntry,
  toolCallEntry,
  type FakeDenialsScript,
} from "../transport/fake-denials.ts";
import type { DenialState, ErrorCode, ThreadStatus, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { PressInOrder } from "./press-words.tsx";
import { RoomOver } from "./states.tsx";
import { errorWords, type WordsRef } from "./words.ts";

/** A string of the denial cards (words-denials.ts). */
const words = (key: DenialKey): WordsRef => ({
  file: "lib/words-denials.ts",
  key,
  text: (lang) => dn(lang, key),
  later: dn.hi[key] === null,
});

/** The open conversation: a question whose answer reached beyond the matter. */
const ENTRIES: ViewEntry[] = [
  userMessage(5, 1, "Compare the loan agreement with our precedents on prepayment."),
  agentText(6, 1, "I'll look at the precedents folder first."),
];

type Scene = {
  entries: ViewEntry[];
  denials?: FakeDenialsScript;
  restarting?: boolean;
  status?: ThreadStatus;
  /** The thread's AI tool; "" for one with no name at all. */
  agent?: string;
  press?: string[];
};

const ALLOW = '[data-testid="denial-allow"]';
const NOT_NOW = '[data-testid="denial-not-now"]';
const card = (kind: keyof typeof DENIAL_KINDS, state: DenialState = "pending"): ViewEntry =>
  denialEntry(1, DENIAL_KINDS[kind], state);
const refused = (wire: "request_escalation" | "dismiss_denial", code: ErrorCode): Scene => ({
  entries: [card("folder")],
  denials: { refuse: { [wire]: code } },
  press: [wire === "request_escalation" ? ALLOW : NOT_NOW],
});

const SCENES = {
  folder: { entries: [card("folder")], status: "running" },
  file: { entries: [card("file")], status: "running" },
  site: { entries: [card("site")], status: "running" },
  shared: { entries: [card("shared")], status: "running" },
  asking: { entries: [card("folder")], denials: { dialog: "never" }, press: [ALLOW] },
  allowed: { entries: [card("site", "allowed")] },
  restarting: { entries: [card("folder", "allowed")], restarting: true, status: "running" },
  "restarting-file": { entries: [card("file", "allowed")], restarting: true, status: "running" },
  refused: { entries: [card("folder", "refused")] },
  dismissed: { entries: [card("folder", "dismissed")] },
  expired: { entries: [card("folder", "expired")] },
  "kept-folder": { entries: [card("folder", "not_offered")] },
  "kept-file": { entries: [card("file", "not_offered")] },
  "kept-site": { entries: [card("site", "not_offered")] },
  upstream: { entries: [card("upstream", "not_offered")] },
  "cross-matter": { entries: [toolCallEntry(1)] },
  "cross-matter-nameless": { entries: [toolCallEntry(1)], agent: "" },
  paused: { entries: [card("folder", "refused"), pausedEntry(1, TURN, 3)], status: "running" },
  "not-pending": refused("request_escalation", "escalation.not_pending"),
  "not-offerable": refused("request_escalation", "escalation.not_offerable"),
  forbidden: refused("request_escalation", "sandbox.unprotected_auto_forbidden"),
  "dismiss-not-pending": refused("dismiss_denial", "escalation.not_pending"),
} satisfies Record<string, Scene>;

const roomOf = (scene: Scene): ReactNode => {
  const over = (
    <RoomOver
      host={() =>
        threadHost({
          entries: [...ENTRIES, ...scene.entries],
          ...(scene.status === undefined ? {} : { status: scene.status }),
          ...(scene.denials === undefined ? {} : { denials: scene.denials }),
          ...(scene.restarting === true ? { restarting: true } : {}),
          ...(scene.agent === undefined ? {} : { agent: scene.agent }),
        })
      }
    />
  );
  return scene.press === undefined ? over : <PressInOrder press={scene.press}>{over}</PressInOrder>;
};

const room = (id: string, ref: DenialKey | WordsRef, scene: keyof typeof SCENES): Entry => ({
  id,
  words: typeof ref === "string" ? words(ref) : ref,
  screen: scene === "paused" ? "room/denials-paused" : "room/denial",
  render: () => roomOf(SCENES[scene]),
});

type ErrorKey = Parameters<typeof errorWords>[0];
const said = (code: ErrorCode): WordsRef => errorWords(ERROR_WORDS[code] as ErrorKey);

/** R18's entries, each id written as it is. */
export const R18: Entry[] = [
  room("R18-01", "Allow access to this folder?", "folder"),
  room("R18-02", "Allow access to this file?", "file"),
  room("R18-03", "Clauseo will be able to read it for this matter only.", "folder"),
  room("R18-04", "This is in another matter: {matter}.", "file"),
  room("R18-05", "Allow access", "folder"),
  room("R18-06", "Not now", "folder"),
  room("R18-07", "Allow this site?", "site"),
  room("R18-08", "This address is shared by many sites. Allowing it allows all of them.", "shared"),
  room("R18-09", "Allow this site", "site"),
  room(
    "R18-10",
    "Clauseo kept this folder closed. It can't be allowed for this matter.",
    "kept-folder",
  ),
  room(
    "R18-11",
    "Clauseo kept this file closed. It can't be allowed for this matter.",
    "kept-file",
  ),
  room(
    "R18-12",
    "Clauseo kept this site closed. It can't be allowed for this matter.",
    "kept-site",
  ),
  room("R18-13", "Your network needs a sign-in Clauseo can't give.", "upstream"),
  room("R18-14", "Waiting for your answer in the window Clauseo opened…", "asking"),
  room("R18-15", "Allowed.", "allowed"),
  room("R18-16", "Going on with the folder…", "restarting"),
  room("R18-17", "Going on with the file…", "restarting-file"),
  room("R18-18", "Not allowed.", "refused"),
  room("R18-19", "Not now.", "dismissed"),
  room("R18-20", "This request has ended.", "expired"),
  room("R18-21", "Clauseo stopped after {n} blocked attempts outside this matter.", "paused"),
  room("R18-22", said("escalation.not_pending"), "not-pending"),
  room("R18-23", said("escalation.not_offerable"), "not-offerable"),
  room("R18-24", said("sandbox.unprotected_auto_forbidden"), "forbidden"),
  room("R18-25", said("escalation.not_pending"), "dismiss-not-pending"),
  room(
    "R18-26",
    "{name} tried to open a file from another matter, {matter}. Clauseo kept it closed. To share that matter's files, choose Use files from another matter.",
    "cross-matter",
  ),
  room("R18-27", "The AI tool", "cross-matter-nameless"),
];
