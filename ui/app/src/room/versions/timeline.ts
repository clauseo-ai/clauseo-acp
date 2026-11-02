// The matter's versions as the Versions parts read them (ui/components versions-model.ts's Version), from the host's
// `version` entries on the matter's stream. Who made each comes from its kind, as HANDOFF item 11's table gives it:
// after_turn is Clauseo in the conversation `list_threads` names, outside_edits the lawyer outside Clauseo, undo an undo
// of the change its turn made, restore a restore to the version its marker names, import the papers first brought in.
// A before_turn version is shown only when it holds files: the lawyer's edits caught as a turn began. A kind this
// version doesn't know shows its time alone. A turn's papers come from its step (StepEntry) where the room holds its
// conversation; elsewhere a version says who and when only. Pure: the clock and the zone come in as arguments.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type {
  Calendar,
  Day,
  DayForm,
  Version,
  Who,
} from "@clauseo/components/clauseo/versions-model.ts";
import type {
  CheckpointKind,
  FileChange,
  MatterHeaderEntry,
  StepEntry,
  ThreadId,
  Timestamp,
  TurnId,
  VersionEntry,
} from "../../transport/protocol.ts";
import { clockOf, dayOf } from "../when.ts";

const DAY_MS = 86_400_000;

/** A moment's calendar day in the zone, as an ISO date: "2026-10-14". */
export const isoDayOf = (at: Timestamp, zone: string): Day =>
  new Date(dayOf(at, zone) * DAY_MS).toISOString().slice(0, 10);

const LOCALE = { en: "en-GB", hi: "hi-IN" } as const satisfies Record<Lang, string>;
const DAY_FORMS = {
  long: { weekday: "long", day: "numeric", month: "long" },
  date: { day: "numeric", month: "long" },
  short: { day: "numeric", month: "short" },
} as const satisfies Record<DayForm, Intl.DateTimeFormatOptions>;
const dayFormats = new Map<string, Intl.DateTimeFormat>();

/** "Wednesday 14 October", "14 October" or "14 Oct", in the language. */
function dayWords(lang: Lang, day: Day, form: DayForm): string {
  const key = `${lang}|${form}`;
  let format = dayFormats.get(key);
  if (format === undefined) {
    format = new Intl.DateTimeFormat(LOCALE[lang], { ...DAY_FORMS[form], timeZone: "UTC" });
    dayFormats.set(key, format);
  }
  return format.format(new Date(`${day}T00:00:00Z`));
}

/** Today in the lawyer's zone, and the words for any day. */
export const calendarOf = (now: Timestamp, zone: string): Calendar => ({
  today: isoDayOf(now, zone),
  words: dayWords,
});

/** A file's name: the last part of its path in the matter folder. */
export const fileName = (path: string): string => path.split(/[\\/]/).pop() ?? path;

/** What the room knows of a version beyond the parts' Version: what the host says made it, and its turn's step. */
export type VersionMeta = {
  kind: CheckpointKind;
  checkpoint: string;
  thread?: ThreadId;
  turn?: TurnId;
  ts: Timestamp;
  /** The files it changed, where known: an outside edit's own, or its turn's step's. */
  files: readonly FileChange[];
  /** The turn's step, where the room holds it. */
  step: StepEntry | null;
};

export type Timeline = {
  /** Oldest first, as the parts take it; the last is now. */
  history: readonly Version[];
  /** Each shown version's meta, by its id (the checkpoint). */
  meta: ReadonlyMap<string, VersionMeta>;
};

export type TimelineInput = {
  /** The matter's `version` entries, oldest first. */
  versions: readonly VersionEntry[];
  header: MatterHeaderEntry | null;
  /** Each restore's checkpoint, to the checkpoint it restored from (the matter stream's version_restored markers). */
  restores: ReadonlyMap<string, string>;
  /** Each conversation's title, as list_threads gives it; one without a title takes `untitled`. */
  titles: ReadonlyMap<ThreadId, string>;
  untitled: string;
  /** A turn's step, where the room holds its conversation. */
  step: (thread: ThreadId, turn: TurnId) => StepEntry | undefined;
  now: Timestamp;
  zone: string;
};

/** Whether a version shows on the timeline: a before_turn only with files of its own. */
export const shown = (v: VersionEntry): boolean => v.kind !== "before_turn" || v.files.length > 0;

/** A turn's step for a version, where the room holds its conversation. */
const stepOf = (v: VersionEntry, input: TimelineInput): StepEntry | null =>
  v.kind === "after_turn" && v.thread !== undefined && v.turn !== undefined
    ? (input.step(v.thread, v.turn) ?? null)
    : null;

function whoOf(v: VersionEntry, input: TimelineInput, afterTurn: ReadonlyMap<TurnId, string>): Who {
  switch (v.kind) {
    case "after_turn":
      return {
        kind: "clauseo",
        conversation:
          (v.thread === undefined ? undefined : input.titles.get(v.thread)) ?? input.untitled,
      };
    case "before_turn":
    case "outside_edits":
      return { kind: "outside" };
    case "undo": {
      // the version the undone turn made, by the turn they share
      const of = v.turn === undefined ? undefined : afterTurn.get(v.turn);
      return { kind: "undo", of: of ?? "" };
    }
    case "restore":
      return { kind: "restore", to: input.restores.get(v.checkpoint) ?? "" };
    case "import":
      return { kind: "import", n: input.header?.import?.files ?? 0 };
    default:
      return { kind: "unknown" };
  }
}

/** The files a version changed, as the parts split them: made and touched by Clauseo, added and touched outside. */
function papersOf(by: "clauseo" | "outside", files: readonly FileChange[]) {
  const touched: string[] = [];
  const added: string[] = [];
  const made: string[] = [];
  for (const f of files) {
    if (f.change === "added") (by === "clauseo" ? made : added).push(f.path);
    else touched.push(f.path);
  }
  return { touched, added, made };
}

export function timelineOf(input: TimelineInput): Timeline {
  const afterTurn = new Map<TurnId, string>();
  for (const v of input.versions)
    if (v.kind === "after_turn" && v.turn !== undefined) afterTurn.set(v.turn, v.checkpoint);
  const history: Version[] = [];
  const meta = new Map<string, VersionMeta>();
  for (const v of input.versions) {
    if (!shown(v)) continue;
    const step = stepOf(v, input);
    const files = step?.files ?? v.files;
    const who = whoOf(v, input, afterTurn);
    const large = step?.skipped.find((s) => s.reason === "over_size_cap");
    const { time } = clockOf(v.ts, input.now, input.zone, "en");
    history.push({
      id: v.checkpoint,
      day: isoDayOf(v.ts, input.zone),
      at: time,
      who,
      ...papersOf(who.kind === "clauseo" ? "clauseo" : "outside", files),
      // Versions keep no paper list per version yet (H12): each version's papers are the ones it touched
      state: { papers: [], docs: {} },
      // no size on a skipped file yet: the panel's line names the file alone
      ...(large === undefined ? {} : { large: { file: fileName(large.path), size: "" } }),
    });
    meta.set(v.checkpoint, {
      kind: v.kind,
      checkpoint: v.checkpoint,
      ...(v.thread === undefined ? {} : { thread: v.thread }),
      ...(v.turn === undefined ? {} : { turn: v.turn }),
      ts: v.ts,
      files,
      step,
    });
  }
  return { history, meta };
}

/** Whether a version's own change can be undone with undo_step: a turn's, whose step the room holds, kept as a
 * version, not undone, with a file to put back. */
export const undoable = (m: VersionMeta | undefined): boolean =>
  m !== undefined &&
  m.kind === "after_turn" &&
  m.step !== null &&
  m.step.checkpoint_after !== undefined &&
  !m.step.undone &&
  m.step.files.length > 0;

/** The version just before one, by its place in the matter's versions (shown or not): what undoing a restore goes
 * back to. */
export function before(versions: readonly VersionEntry[], checkpoint: string): VersionEntry | null {
  const i = versions.findIndex((v) => v.checkpoint === checkpoint);
  return i > 0 ? (versions[i - 1] ?? null) : null;
}
