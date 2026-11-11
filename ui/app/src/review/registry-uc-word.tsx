// UC's entries for a Word file's tracked changes (UC-120 on; UC-113 to UC-119 are another brief's): the Tracked
// changes press on a .docx tile, and the panel it opens in the wall's place with each line it can show, in the real
// room over a host whose wall lists one .docx and whose list_revisions and decide_revisions answer as the state needs;
// then Versions' row for a decision, in the room with Versions open over U8's host. Each state shows after the presses
// that bring it (RoomClicking). The English was settled by the orchestrator: every entry is provisional, its Hindi too.
import { detailed, fakeNext } from "../room/fake-next.ts";
import type { PaperSummary } from "../room/host-next.ts";
import { MATTER, THREAD } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { MatterSummary, Revision, RevisionList } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { versionsHost } from "./registry-u8.tsx";
import { UC_ALL as UC_BEFORE_WORD } from "./registry-uc-blackline.tsx";
import { RoomClicking } from "./states.tsx";
import { errorWords, readerWords, versionWords } from "./words.ts";

const THE_MATTER: MatterSummary = {
  matter: MATTER,
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
const REPLY = "Reply to the bank.docx";
const DOCX: PaperSummary = {
  file: "00000000-0000-4000-8000-0000000000d1",
  path: REPLY,
  size: 24_000,
  modified: Date.UTC(2026, 10, 9, 9, 0),
  // the host found tracked changes in it: its tile offers Tracked changes
  tracked: true,
};
/** A .docx with no tracked changes or comments: its tile offers Open in Word alone. */
const CLEAN: PaperSummary = {
  file: "00000000-0000-4000-8000-0000000000d2",
  path: "Vakalatnama.docx",
  size: 18_000,
  modified: Date.UTC(2026, 10, 9, 9, 0),
};

/** A change made this morning, so it reads "Today, …". */
const morning = (h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};
const change = (id: number, kind: Revision["kind"], text?: string): Revision => ({
  ids: [id],
  kind,
  author: "A. Mehta",
  at: morning(9, 10 + id),
  block: `p${id}`,
  ...(text === undefined ? {} : { text }),
});
const LIST: RevisionList = {
  path: REPLY,
  version: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  revisions: [
    change(1, "insert", "within fourteen days of this notice"),
    change(2, "delete", "forthwith"),
    change(3, "move", "The bank's letter of 2 March is annexed."),
    change(4, "format"),
  ],
  truncated: false,
};

/** The room over a matter whose wall lists one .docx; list_revisions gives `list`, or is refused with that code, and
 * decide_revisions is refused with `decide`. */
const wordHost =
  (list: RevisionList | string, decide = "word.edit_failed") =>
  (): FakeTransport =>
    new FakeTransport({
      next: fakeNext({
        matters: () => [detailed(THE_MATTER)],
        papers: [DOCX, CLEAN],
        parts: ["papers"],
      }),
      query: (query) => {
        if (query === "list_matters") return { matters: [THE_MATTER] };
        if (typeof query === "object" && query.list_threads !== undefined)
          return {
            threads: [{ thread: THREAD, matter: MATTER, agent: "claude-acp", status: "idle" }],
          };
        if (typeof query === "object" && query.list_revisions !== undefined) {
          if (typeof list === "string") throw { code: list, message: "", retryable: false };
          return { revisions: list };
        }
        throw new Error(`not scripted: ${JSON.stringify(query)}`);
      },
      dispatch: () => {
        throw { code: decide, message: "", retryable: decide === "word.edit_failed" };
      },
    });

/** The wall beside the thread: a window past the room's 1200 px breakpoint. */
const WINDOW = { width: 1280, height: 760 } as const;
const TRACKED = 'aside.room-pane [data-testid="tracked-changes"]';
const ACCEPT = '[data-testid="word-changes"] .rd-accept';
/** Versions open, from the toolbar's pair (as U8's scenes open it). */
const VERSIONS = ".room-pair button";

const panel = (
  id: string,
  words: Entry["words"],
  host: () => FakeTransport,
  clicks: readonly string[] = [TRACKED],
): Entry => ({
  id,
  words,
  screen: "room/tracked-changes",
  ...WINDOW,
  render: () => <RoomClicking host={host} clicks={clicks} />,
});

/** The entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const UC_WORD: Entry[] = [
  {
    id: "UC-120",
    words: readerWords("Tracked changes"),
    screen: "room/wall",
    ...WINDOW,
    render: () => <RoomClicking host={wordHost(LIST)} clicks={[]} />,
  },
  panel(
    "UC-121",
    readerWords("No tracked changes in this file."),
    wordHost({ ...LIST, revisions: [] }),
  ),
  panel("UC-122", readerWords("Formatting changed"), wordHost(LIST)),
  panel("UC-123", readerWords("Moved"), wordHost(LIST)),
  panel(
    "UC-124",
    errorWords(
      "This file changed after the list was made, so nothing was accepted or rejected. The list now shows its current changes.",
    ),
    wordHost(LIST, "word.revisions_stale"),
    [TRACKED, ACCEPT],
  ),
  panel(
    "UC-125",
    errorWords(
      "Clauseo can't read the tracked changes in this file. It may be damaged, or it may not be a Word document.",
    ),
    wordHost("word.not_docx"),
  ),
  panel(
    "UC-126",
    errorWords("Clauseo's Word tool is missing. Reinstall Clauseo to accept or reject changes."),
    wordHost("word.stetdoc_missing"),
  ),
  panel(
    "UC-127",
    errorWords("Clauseo couldn't save your decision. The file is as it was."),
    wordHost(LIST, "word.edit_failed"),
    [TRACKED, ACCEPT],
  ),
  {
    id: "UC-128",
    words: versionWords("You accepted or rejected tracked changes in {file}"),
    screen: "room/versions",
    ...WINDOW,
    render: () => (
      <RoomClicking
        host={() =>
          versionsHost({
            revisions: [
              {
                path: REPLY,
                change: "modified",
                size_bytes: 24_000,
                may_include_lawyer_edits: true,
              },
            ],
          })
        }
        clicks={[VERSIONS]}
      />
    ),
  },
];

/** registry-uc-blackline.tsx's UC_ALL (registry-uc.tsx's UC, then the Word blackline's scenes), then these. */
export const UC_ALL: Entry[] = [...UC_BEFORE_WORD, ...UC_WORD];
