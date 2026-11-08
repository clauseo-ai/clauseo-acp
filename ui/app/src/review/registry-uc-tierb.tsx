// R17 PR-E's tier-B refusals in the copy review (the app's follow-up to #712), apart from registry-uc.tsx and
// registry.tsx to keep each under the size limit: the full pool's note by the sandbox's why (none or unknown, no free
// place, two conversations already in this matter), its reset line (one, many) and "Free up places" (never before
// R17 PR-D, so only here, from a host faked to say needs_reset); the note of an item a place didn't hand back
// (pool_owned), whose IT line says "it" in the room, which knows no item; and in the thread, a refused turn naming
// where (TurnFailed's at), each cant_label kind, the full pool's Try again, and the stop for leaving the Safe
// workspace. The Notes kind is in TurnFailed's detail, which the view's step doesn't carry yet (a host follow-up), so
// its line shows through the same words function on a plain fixture line. R17 PR-E's M4: a broken set-up's refusal
// with its repair (per-user: the installer's line and Get the installer; a firm's: ask IT) and its last resort, the
// blocked note's refusal opening, and a cause this version doesn't know.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { failureWords } from "@clauseo/components/lib/words-errors-where.ts";
import type { ProtectionKey } from "@clauseo/components/lib/words-protection.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode, Protection, StepEntry, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import { note, room } from "./registry-uc.tsx";
import { RoomOver } from "./states.tsx";
import { errorWords, limitWords, protectionWords, threadWords } from "./words.ts";

const np = (cause: unknown): Protection => ({ not_protected: { cause } }) as Protection;
const pool = (over: object) =>
  np({ pool_full: { size: 8, open: 5, held: 0, owed: 2, set_aside: 1, ...over } });
const P = {
  free: pool({ why: "no_free_slot" }),
  matter: pool({ why: "matter_sessions_full" }),
  resetOne: pool({ why: "no_free_slot", needs_reset: 1 }),
  resetMany: pool({ why: "no_free_slot", needs_reset: 3 }),
  owned: np("pool_owned"),
  // R17 PR-E's M4: broken, blocked, and a cause from a newer Clauseo
  failed: np("set_up_failed"),
  policy: np({ blocked: { reason: "policy" } }),
  later: np("a_cause_from_a_newer_clauseo"),
  // #722: the check couldn't confirm the Safe workspace
  probe: np("probe_failed"),
};
/** A firm install, by its policy: the repair asks IT (blocked.ts's firmInstall), and no Get the installer. */
const FIRM = { unprotected_auto_allowed: false } as const;
const pw = (key: ProtectionKey) => protectionWords(key);

const thread = (state: ThreadState) => () => <RoomOver host={() => threadHost(state)} />;
const ASKED = userMessage(5, 1, "Summarise the notice to the bank.");
const SO_FAR = agentText(6, 1, "Clause nine requires **notice in writing**, served at");
const ended = (over: Partial<StepEntry>): ViewEntry => step(9, { ts: Date.now(), ...over });
/** A turn refused with `code`, naming `at` when given, as the thread holds it once reopened. */
const refused = (code: ErrorCode, at?: string) =>
  thread({
    entries: [
      ASKED,
      ended({ stop: "failed", failure: code, ...(at === undefined ? {} : { failure_at: at }) }),
    ],
  });
const DRAFT = "Pleadings/Draft reply.docx";

/** The Notes kind's line on a plain line, as the step row would say it (fixture: the view lacks the detail). */
const notesLine = (lang: Lang) => (
  <main lang={lang} className="grid min-h-dvh place-items-center bg-page p-(--s4)">
    <p className="text-small text-quiet max-w-[36rem]" data-code="sandbox.cant_label">
      {failureWords(lang, "sandbox.cant_label", { detail: "notes" })}
    </p>
  </main>
);

/** The entries, each id written as it is. */
export const UC_TIERB: Entry[] = [
  room(
    "UC-97",
    pw(
      "All {size} places in this computer's Safe workspace are in use by open conversations. End a conversation in another matter to free one.",
    ),
    "room/np-note-pool",
    note(P.free),
  ),
  room(
    "UC-98",
    pw("Two conversations are already working in this matter. Try again when one finishes."),
    "room/np-note-pool",
    note(P.matter),
  ),
  room(
    "UC-99",
    pw(
      "One of them needs resetting before it can be used again. Windows asks for permission first.",
    ),
    "room/np-note-pool",
    note(P.resetOne),
  ),
  room(
    "UC-100",
    pw(
      "{n} of them need resetting before they can be used again. Windows asks for permission first.",
    ),
    "room/np-note-pool",
    note(P.resetMany),
  ),
  room("UC-101", pw("Free up places"), "room/np-note-pool", note(P.resetMany)),
  room(
    "UC-102",
    pw(
      "Part of this matter's folder wasn't handed back to you after the Safe workspace last used it, so Clauseo can't answer in this matter until it is.",
    ),
    "room/np-note",
    note(P.owned),
  ),
  room(
    "UC-103",
    errorWords("Ask your IT team to give you back ownership of it."),
    "room/np-note",
    note(P.owned),
  ),
  room(
    "UC-104",
    pw(
      "The Safe workspace on this computer isn't working, so Clauseo can't answer here until it's set up again.",
    ),
    "room/np-note",
    note(P.failed),
  ),
  room("UC-105", pw("If that doesn't help, ask your IT team."), "room/np-note", note(P.failed)),
  room(
    "UC-106",
    pw("Something on this computer stops the Safe workspace, so Clauseo can't answer here."),
    "room/np-note",
    note(P.policy),
  ),
  room(
    "UC-107",
    pw(
      "This version of Clauseo doesn't recognise what's stopping the Safe workspace, so it can't answer here. Update Clauseo, or ask your IT team.",
    ),
    "room/np-note",
    note(P.later),
  ),
  room("UC-108", pw("Get the installer"), "room/np-note", note(P.failed)),
  room(
    "UC-109",
    pw("Run the Clauseo installer again to set up the Safe workspace."),
    "room/np-note",
    note(P.failed),
  ),
  room("UC-110", pw("Ask your IT team to install Clauseo again."), "room/np-note", {
    ...note(P.failed),
    over: FIRM,
  }),
  room(
    "UC-111",
    pw(
      "Clauseo couldn't check the Safe workspace on this computer, so it can't answer here yet. Try again. If it keeps happening, restart Clauseo.",
    ),
    "room/np-note",
    note(P.probe),
  ),
  {
    id: "U3-52",
    words: errorWords(
      "{where} in this matter still belongs to the Safe workspace, so Clauseo can't answer here.",
    ),
    screen: "room/thread",
    render: refused("sandbox.pool_owned", DRAFT),
  },
  {
    id: "U3-53",
    words: errorWords("Ask your IT team to give you back ownership of {where}."),
    screen: "room/thread",
    render: refused("sandbox.pool_owned", DRAFT),
  },
  {
    id: "U3-54",
    words: errorWords(
      "The Safe workspace couldn't be given access to {where}, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
    ),
    screen: "room/thread",
    render: refused("sandbox.cant_label", "Pleadings"),
  },
  {
    id: "U3-55",
    words: errorWords(
      "The Safe workspace couldn't be given access to this matter's notes, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
    ),
    screen: "room/thread",
    render: notesLine,
  },
  {
    id: "U3-56",
    words: errorWords(
      "The Safe workspace couldn't be prepared for this matter, so Clauseo can't answer here. If it keeps happening, ask your IT team.",
    ),
    screen: "room/thread",
    render: refused("sandbox.cant_label"),
  },
  {
    id: "U3-57",
    words: threadWords(
      "Clauseo stopped {name}: part of it tried to run outside the Safe workspace.",
    ),
    screen: "room/thread",
    render: thread({ entries: [ASKED, SO_FAR, ended({ stop: "left_safe_workspace" })] }),
  },
  {
    id: "U3-58",
    words: threadWords(
      "Ask again to carry on. If this keeps happening, tell whoever looks after this computer.",
    ),
    screen: "room/thread",
    render: thread({ entries: [ASKED, SO_FAR, ended({ stop: "left_safe_workspace" })] }),
  },
  {
    id: "U3-59",
    words: limitWords("Try again"),
    screen: "room/composer",
    // the full pool's refusal arriving while the lawyer watches: Try again asks the question once more
    render: thread({
      entries: [ASKED],
      status: "running",
      later: [ended({ stop: "failed", failure: "sandbox.pool_full" })],
    }),
  },
];
