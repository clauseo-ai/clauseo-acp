// U9's copy review entries (Word), apart from registry.tsx to keep that file under the size limit: the change note over a
// turn that changed one Word document, whose Undo Word refuses while the file is open; the Versions restore that the
// lock refuses; and the note's and the wall's Open in Word. Two files held open name both (06 and 07). Each state is the real room over a host scripted for it
// (hosts.ts, registry-u8.tsx's versionsHost), then the presses that bring it.
import { NO_STAND_INS } from "../room/versions/stand-ins.ts";
import { agentText, step, userMessage, uuid } from "../testing/stream-script.ts";
import type { PaperSummary, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { versionsHost, type Matter } from "./registry-u8.tsx";
import { RoomClicking, RoomFocusing } from "./states.tsx";
import { readerWords, versionWords } from "./words.ts";

/** A local time, `days` from today. */
const atLocal = (days: number, h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h, m).getTime();
};
const ASKED = atLocal(0, 16, 31);
const ENDED = atLocal(0, 16, 32);

/** The Word document the turn changed, and the one the wall lists. */
const NOTICE = "Notice to the bank.docx";
/** A second Word document, which a program also holds open (06 and 07). */
const LEASE = "Lease of the office.docx";

const QUESTION: ViewEntry = {
  user_message: {
    ...userMessage(5, 1, "Put the notice's date in the bank letter.").user_message!,
    ts: ASKED,
  },
};
const ANSWER = agentText(6, 1, "The notice's date is now **30 October 2026**.");
/** The turn's change note: one Word document changed, kept as checkpoint 9, and not yet undone. */
const changed = (paths: readonly string[] = [NOTICE]): ViewEntry =>
  step(9, {
    ts: ENDED,
    files: paths.map((path) => ({
      path,
      change: "modified",
      size_bytes: 1200,
      may_include_lawyer_edits: false,
    })),
    checkpoint_after: uuid(1, 9),
  });
/** The wall's papers: the Word document, which the wall lists with its file ref. */
const PAPERS: PaperSummary[] = [{ file: uuid(1, 8), path: NOTICE, size: 1200, modified: ENDED }];

/** Versions' restore of a version three days ago, whose preview names the Word document as locked. */
const LOCKED_RESTORE: Matter = { locked: [NOTICE] };
/** The same restore, with two Word documents held open. */
const LOCKED_TWO_RESTORE: Matter = { locked: [NOTICE, LEASE] };

/** Versions open, the version of the turn three days ago, its Restore, and the dialog's confirm (U8's selectors). */
const OPEN = ".room-pair button";
const EARLIER = '[data-testid="version"][data-checkpoint="cp2"]';
const RESTORE = '[data-testid="version-restore"]';
const CONFIRM = '[data-testid="dialog-confirm"]';
const UNDO = '[data-testid="undo"]';

/** A window past the room's 1200 px breakpoint, so the wall stands beside the thread, tall enough for the lock. */
const WINDOW = { width: 1280, height: 760 } as const;

/** The room over the thread, with the change note's Undo pressed once it shows: Word refuses it, and the lock shows. */
const undoRefused = () => (
  <RoomClicking
    host={() =>
      threadHost({
        entries: [QUESTION, ANSWER, changed()],
        undoRefused: "matter.file_locked",
      })
    }
    clicks={[UNDO]}
  />
);

/** U9's entries, each id written as it is: an id never moves when an entry is added or taken out. The brief's names
 * map to the numbers: 01 the undo's line, 02 the hold's line, 03 Try again, 04 the restore's line, 05 Open in Word, 06
 * the undo's line with two Word documents held, 07 the restore's (the id form is "U9-" and two digits, status.ts). */
export const U9: Entry[] = [
  {
    id: "U9-01",
    words: versionWords("Close '{file}' in Word to undo the change."),
    screen: "room/thread",
    ...WINDOW,
    render: undoRefused,
  },
  {
    id: "U9-02",
    words: versionWords("Word holds the file while it is open. Nothing has changed yet."),
    screen: "room/thread",
    ...WINDOW,
    render: undoRefused,
  },
  {
    id: "U9-03",
    words: versionWords("Try again"),
    screen: "room/thread",
    ...WINDOW,
    render: undoRefused,
  },
  {
    id: "U9-04",
    words: versionWords("Close '{file}' in Word to restore it."),
    screen: "room/versions",
    ...WINDOW,
    render: () => (
      <RoomClicking
        host={() => versionsHost(LOCKED_RESTORE)}
        clicks={[OPEN, EARLIER, RESTORE, CONFIRM]}
        standIns={NO_STAND_INS}
      />
    ),
  },
  {
    id: "U9-05",
    words: readerWords("Open in Word"),
    screen: "room/thread",
    ...WINDOW,
    render: () => (
      <RoomFocusing
        host={() => threadHost({ entries: [QUESTION, ANSWER, changed()], papers: PAPERS })}
        focus='aside.room-pane li.tile button.tile-face[data-testid="open-in-word"]'
      />
    ),
  },
  {
    id: "U9-06",
    words: versionWords("Close {files} in Word to undo the change."),
    screen: "room/thread",
    ...WINDOW,
    render: () => (
      <RoomClicking
        host={() =>
          threadHost({
            entries: [QUESTION, ANSWER, changed([NOTICE, LEASE])],
            undoRefused: "matter.file_locked",
            undoLocked: [NOTICE, LEASE],
          })
        }
        clicks={[UNDO]}
      />
    ),
  },
  {
    id: "U9-07",
    words: versionWords("Close {files} in Word to restore it."),
    screen: "room/versions",
    ...WINDOW,
    render: () => (
      <RoomClicking
        host={() => versionsHost(LOCKED_TWO_RESTORE)}
        clicks={[OPEN, EARLIER, RESTORE, CONFIRM]}
        standIns={NO_STAND_INS}
      />
    ),
  },
];
