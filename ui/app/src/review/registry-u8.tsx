// U8's copy review entries (Versions), apart from registry.tsx to keep that file under the size limit: each string in
// the room with Versions open over a host scripted for the matter's stream (its versions) and its one conversation's
// stream (its steps), as room/versions/testing.ts scripts them for the tests, then the presses that bring the state.
import {
  batch,
  event,
  header,
  MATTER,
  reset,
  THREAD,
  TURN,
  uuid,
} from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  CheckpointKind,
  FileChange,
  MatterSummary,
  RestorePreview,
  StepEntry,
  ViewEntry,
  ViewPatchBody,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { RoomClicking } from "./states.tsx";
import { versionWords, type WordsRef } from "./words.ts";

const THE_MATTER: MatterSummary = {
  matter: MATTER,
  name: "Sharma v. State of Delhi",
  kind: "folder",
};
const MATTER_STREAM = `matter:${MATTER}`;
/** The turn whose change couldn't be kept as a version. */
const UNSAVED_TURN = uuid(2, 4);

const MINUTE = 60_000;
/** A local time, `days` from today. */
const atLocal = (days: number, h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h, m).getTime();
};
/** Today's versions, `n` minutes apart from forty minutes ago (from just past midnight, so they stay today). */
const today = (n: number) => Math.max(atLocal(0, 0, 1), Date.now() - 40 * MINUTE) + n * MINUTE;

const NOTICE = "Notice to the bank.docx";
const REPLY = "Reply to the bank.docx";
const LEASE = "Lease.docx";
const file = (path: string, may = false): FileChange => ({
  path,
  change: "modified",
  size_bytes: 1200,
  may_include_lawyer_edits: may,
});

const version = (
  n: number,
  kind: CheckpointKind,
  ts: number,
  files: FileChange[] = [],
  turn?: string,
): ViewEntry => ({
  version: {
    order: n,
    checkpoint: `cp${n}`,
    kind,
    ts,
    files,
    ...(turn === undefined ? {} : { thread: THREAD, turn }),
  },
});

const stepEntry = (order: number, over: Partial<StepEntry>): ViewEntry => ({
  step: {
    order,
    turn: TURN,
    stop: "end_turn",
    files: [],
    skipped: [],
    undone: false,
    ts: atLocal(-3, 10, 30),
    ...over,
  },
});

type Matter = {
  /** The files of the conversation's turn three days ago, kept as version cp2. */
  files?: FileChange[];
  /** What the lawyer changed outside Clauseo today, version cp3. */
  outside?: FileChange[];
  /** The conversation's answer is running. */
  running?: boolean;
  /** A later turn changed a paper, and the change couldn't be kept as a version. */
  unsaved?: boolean;
  /** compare_versions' answer. */
  compare?: FileChange[];
  /** preview_restore's answer, past its changes. */
  kept?: string[];
};

/** The matter's history: its papers brought in three days ago, a turn of the conversation's that morning, the
 * lawyer's edits outside Clauseo this hour, and the conversation's next turn, now. */
function versionsHost(m: Matter): FakeTransport {
  const files = m.files ?? [file(NOTICE)];
  const versions = [
    version(1, "import", atLocal(-3, 10, 0)),
    version(2, "after_turn", atLocal(-3, 10, 30), files, TURN),
    version(3, "outside_edits", today(0), m.outside ?? [file(LEASE)]),
    version(4, "after_turn", today(10), [], uuid(3, 4)),
  ];
  const steps = [
    stepEntry(9, { files, checkpoint_after: "cp2" }),
    ...(m.unsaved === true
      ? [stepEntry(19, { turn: UNSAVED_TURN, files: [file(REPLY)], ts: today(20) })]
      : []),
  ];
  const matterReset: ViewPatchBody = {
    reset: {
      view_window: {
        header: {
          matter_header: {
            order: 0,
            matter: MATTER,
            name: THE_MATTER.name,
            kind: "folder",
            import: { files: 3, bytes: 1000, cloud_only: 0 },
          },
        },
        entries: versions,
        has_older: false,
      },
    },
  };
  const status = m.running === true ? "running" : "idle";
  const preview: RestorePreview = {
    changes: [file(NOTICE)],
    added_since: [],
    kept: m.kept ?? [],
    locked: [],
  };
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [THE_MATTER] };
      if (typeof query !== "object") throw new Error(`not scripted: ${JSON.stringify(query)}`);
      if (query.list_threads !== undefined)
        return {
          threads: [
            { thread: THREAD, matter: MATTER, agent: "claude", status, title: "Draft reply" },
          ],
        };
      if (query.compare_versions !== undefined) return { file_changes: m.compare ?? [] };
      if (query.preview_restore !== undefined) return { restore_preview: preview };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    dispatch: (envelope) =>
      envelope.command.restore_version === undefined
        ? "done"
        : { version_restored: { checkpoint: "cp5" } },
    subscribe: (stream) =>
      queueMicrotask(() =>
        transport.deliver(
          event(
            stream === MATTER_STREAM
              ? batch(1, [matterReset], undefined, stream)
              : batch(1, [reset(steps, header(status))], undefined, stream),
          ),
        ),
      ),
  });
  return transport;
}

/** Versions open, from the toolbar's pair. */
const OPEN = ".room-pair button";
/** The version of the conversation's turn three days ago, and the lawyer's edits today. */
const EARLIER = '[data-testid="version"][data-checkpoint="cp2"]';
const TODAY = '[data-testid="version"][data-checkpoint="cp3"]';
const COMPARE = ".ver-compare";
const RESTORE = '[data-testid="version-restore"]';
const UNDO = '[data-testid="version-undo"]';
const CONFIRM = '[data-testid="dialog-confirm"]';

/** Versions open beside the work: a window past the room's 1200 px breakpoint, tall enough for the dialogs. */
const WINDOW = { width: 1280, height: 760 } as const;

const state = (id: string, words: WordsRef, m: Matter, clicks: readonly string[]): Entry => ({
  id,
  words,
  screen: "room/versions",
  ...WINDOW,
  render: () => <RoomClicking host={() => versionsHost(m)} clicks={clicks} />,
});

const SHEET = [file(NOTICE)];

/** U8's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U8: Entry[] = [
  // the design's foot, in place of "The matter's versions are kept on this computer."
  state("U8-01", versionWords("Version history is kept on this computer."), {}, [OPEN]),
  state(
    "U8-02",
    versionWords("Restore and Undo wait until the answer in {conversation} is done."),
    { running: true },
    [OPEN, EARLIER],
  ),
  state(
    "U8-03",
    versionWords("Clauseo changed {paper}, and the change couldn't be kept as a version."),
    { unsaved: true },
    [OPEN],
  ),
  state("U8-04", versionWords("Not kept as a version."), { unsaved: true }, [OPEN]),
  state("U8-05", versionWords("Undo the restore"), {}, [OPEN, EARLIER, RESTORE, CONFIRM]),
  state("U8-06", versionWords("No changes since then."), { compare: [] }, [OPEN, EARLIER, COMPARE]),
  state("U8-07", versionWords("Changed since {time} on {date}"), { compare: SHEET }, [
    OPEN,
    EARLIER,
    COMPARE,
  ]),
  state("U8-08", versionWords("Changed since {time} today"), { compare: SHEET }, [
    OPEN,
    TODAY,
    COMPARE,
  ]),
  state("U8-09", versionWords("As it was at {time} on {date}"), { compare: SHEET }, [
    OPEN,
    EARLIER,
    COMPARE,
  ]),
  state("U8-10", versionWords("As it was at {time} today"), { compare: SHEET }, [
    OPEN,
    TODAY,
    COMPARE,
  ]),
  state(
    "U8-11",
    versionWords("'{papers}' can't be restored and stays as it is."),
    { kept: ["Annexure 3.pdf"] },
    [OPEN, EARLIER, RESTORE],
  ),
  // the undo dialog's rows for a document that may hold the lawyer's own edits
  state(
    "U8-12",
    versionWords("It may hold your own edits"),
    { files: [file(NOTICE, true), file(REPLY)] },
    [OPEN, EARLIER, UNDO],
  ),
  state(
    "U8-13",
    versionWords("1 document may hold your own edits."),
    { files: [file(NOTICE, true), file(REPLY)] },
    [OPEN, EARLIER, UNDO],
  ),
  state(
    "U8-14",
    versionWords("{n} documents may hold your own edits."),
    { files: [file(NOTICE, true), file(REPLY, true)] },
    [OPEN, EARLIER, UNDO],
  ),
  // one row that may hold the lawyer's edits, one changed again since (outside Clauseo, today)
  state(
    "U8-15",
    versionWords("Choose for each document."),
    { files: [file(NOTICE, true), file(REPLY)], outside: [file(REPLY)] },
    [OPEN, EARLIER, UNDO],
  ),
];
