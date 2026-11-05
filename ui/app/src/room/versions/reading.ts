// What Versions' controller (controller.ts) reads beyond the matter's timeline: every older page of a stream, the
// newest change that couldn't be kept as a version, whether the matter's stream has gained an entry that isn't the
// room's last act's own (which ends the line saying what the act did), and which file a refused restore was held on.
import type { ViewStore } from "../../store/store.ts";
import { streamOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type {
  HostError,
  MatterId,
  RestorePreview,
  StreamId,
  ThreadId,
  ThreadSummary,
  TurnId,
  ViewEntry,
} from "../../transport/protocol.ts";
import { orderOf, pageBefore } from "../matter-stream.ts";
import { lockedFiles, wordLocks } from "../word/lock.ts";
import { inRowOrder, type Ask, type Done, type Unsaved } from "./state.ts";
import type { Timeline } from "./timeline.ts";

/** Pages a stream back from its window's first entry, oldest first, until none are left or `enough` says so. A page
 * that fails ends the reading with what came. */
export async function pageBack(
  transport: HostTransport,
  stream: StreamId,
  view: StreamView,
  enough: (entries: readonly ViewEntry[]) => boolean = () => false,
): Promise<ViewEntry[]> {
  const first = view.ids[0] === undefined ? undefined : view.entries.get(view.ids[0]);
  let before = first === undefined ? null : orderOf(first);
  const older: ViewEntry[] = [];
  try {
    while (before !== null) {
      const page = await pageBefore(transport, stream, before);
      older.unshift(...page.entries);
      if (enough(page.entries)) break;
      const oldest = page.entries[0];
      before = page.more && oldest !== undefined ? orderOf(oldest) : null;
    }
  } catch (error: unknown) {
    console.warn(`page_entries failed: ${JSON.stringify(error)}`);
  }
  return older;
}

/** What Versions reads of a conversation's stream, as one string: its steps, each by id with whether it is undone,
 * kept as a version and how many files it changed, and the markers of its undone steps. Streamed text, tool calls and
 * the rest of the conversation change none of it, so they make no new snapshot. */
export function stepsSig(view: StreamView | undefined): string {
  if (view === undefined) return "";
  let out = String(view.epoch);
  for (const id of view.ids) {
    if (id.startsWith("step:")) {
      const s = view.entries.get(id)?.step;
      out += `|${id}:${s?.undone}:${s?.checkpoint_after}:${s?.files.length}`;
    } else if (id.startsWith("marker:") && view.entries.get(id)?.marker?.kind.step_undone) {
      out += `|${id}`;
    }
  }
  return out;
}

/** The newest step of a held conversation with files and no version after it. */
export function unsavedOf(store: ViewStore, threads: readonly ThreadSummary[]): Unsaved | null {
  let found: { order: number; u: Unsaved } | null = null;
  for (const t of threads) {
    const view = store.view(streamOf(t.thread));
    if (view === undefined) continue;
    for (const id of view.ids) {
      if (!id.startsWith("step:")) continue;
      const step = view.entries.get(id)?.step;
      const first = step?.files[0];
      if (step === undefined || first === undefined || step.checkpoint_after !== undefined)
        continue;
      if (found === null || step.order > found.order)
        found = { order: step.order, u: { thread: t.thread, paper: first.path, ts: step.ts } };
    }
  }
  return found?.u ?? null;
}

/** What marks an act's own entries on the matter's stream: the version it wrote and that version's marker, or, for an
 * undo the host answered with a bare done, an undo version of its turn. */
export type Own = { checkpoint: string | null; turn: TurnId | null };

/** The act's own version: the one it wrote, or its turn's undo. */
const isOwnVersion = (e: ViewEntry, own: Own): boolean =>
  own.checkpoint !== null
    ? e.version?.checkpoint === own.checkpoint
    : own.turn !== null && e.version?.kind === "undo" && e.version.turn === own.turn;

const isOwnMarker = (e: ViewEntry, own: Own): boolean =>
  own.checkpoint !== null && e.marker?.kind.version_restored?.checkpoint === own.checkpoint;

/** Whether the matter's stream holds an entry it didn't hold as the act was sent (`base`) and that isn't the act's. The
 * act's are its version, its marker, and every entry at or below its version's order: the host writes the outside
 * edits it found before writing (an outside_edits version) in the same commit as the act's own version. */
export function movedPast(view: StreamView, base: ReadonlySet<string>, own: Own): boolean {
  let upTo = Number.NEGATIVE_INFINITY;
  for (const id of view.ids) {
    const e = view.entries.get(id);
    if (e !== undefined && isOwnVersion(e, own)) upTo = Math.max(upTo, orderOf(e));
  }
  for (const id of view.ids) {
    const e = view.entries.get(id);
    if (base.has(id) || e === undefined || isOwnMarker(e, own) || orderOf(e) <= upTo) continue;
    return true;
  }
  return false;
}

/** The files the host put back for a turn's undo, as the step_undone marker on its conversation's stream says; null
 * until the marker comes. */
export function restoredOf(
  store: ViewStore,
  thread: ThreadId,
  turn: TurnId,
): readonly string[] | null {
  const view = store.view(streamOf(thread));
  let restored: readonly string[] | null = null;
  for (const id of view?.ids ?? []) {
    const undone = view?.entries.get(id)?.marker?.kind.step_undone;
    if (undone?.turn === turn) restored = undone.restored;
  }
  return restored;
}

/** The undo a line says was done: the turn's conversation, the turn, and the version it took back. */
export type Undid = { thread: ThreadId; turn: TurnId; id: string };

/** The line for the room's last act: it stands until the matter's stream gains an entry not the act's own, and an
 * undo's names the files the host put back once the marker on the conversation's stream says which: a file the host
 * couldn't write stays as it is, whatever the lawyer chose. */
export class DoneWatch {
  /** The matter's entries as the act was sent. */
  #base: ReadonlySet<string> = new Set();
  #by: { base: ReadonlySet<string>; own: Own; undid: Undid | null } | null = null;

  sent(view: StreamView | undefined): void {
    this.#base = new Set(view?.ids ?? []);
  }

  finished(own: Own, undid: Undid | null): void {
    this.#by = { base: this.#base, own, undid };
  }

  after(
    done: Done | null,
    view: StreamView | undefined,
    store: ViewStore,
    timeline: Timeline,
  ): Done | null {
    const by = this.#by;
    if (done === null || by === null || view === undefined) return done;
    if (movedPast(view, by.base, by.own)) {
      this.#by = null;
      return null;
    }
    if (done.kind !== "undidChange" || by.undid === null) return done;
    const { thread, turn, id } = by.undid;
    const restored = restoredOf(store, thread, turn);
    // none put back says nothing the line could name: it keeps the lawyer's choices
    if (restored === null || restored.length === 0) return done;
    const papers = inRowOrder(timeline, id, restored);
    return papers.join("\n") === done.papers.join("\n") ? done : { ...done, papers };
  }
}

/** The restore dialog after a program refused the restore a file: Word's lines when the files the host names as held,
 * or else the files the preview names as locked, or else the files the restore writes, are all Word documents
 * (word/lock.ts). A preview naming none locked and writing more than one is asked again first, in case it now names
 * one. Otherwise the dialog stays with what restoring writes, says why, and offers Try again. */
export async function lockedRestore(
  ask: Extract<Ask, { act: "restore" }>,
  refusal: { code: string; files?: readonly string[] | undefined },
  previewOf: (id: string) => Promise<RestorePreview | null>,
): Promise<Ask> {
  const { id, undoes } = ask;
  let preview = ask.preview;
  if (preview.locked.length === 0 && preview.changes.length !== 1) {
    try {
      preview = (await previewOf(id)) ?? preview;
    } catch (error: unknown) {
      console.warn(`preview_restore failed: ${JSON.stringify(error)}`);
    }
  }
  const files = wordLocks(
    refusal.code,
    lockedFiles(
      refusal,
      preview.changes.map((c) => c.path),
      preview.locked,
    ),
  );
  return files === null
    ? { act: "restore", id, preview, undoes, stage: "confirm", failed: refusal.code }
    : { act: "restore", id, preview, undoes, stage: "word", files };
}

/** What restoring a version would write now, by the host's preview_restore: null when the answer isn't one. */
export async function previewRestore(
  transport: HostTransport,
  matter: MatterId,
  id: string,
): Promise<RestorePreview | null> {
  const r = await transport.query({ preview_restore: { matter, checkpoint: id, scope: "matter" } });
  return "restore_preview" in r ? r.restore_preview : null;
}

/** A restore whose preview rejected: the rejection is logged, and a file a program holds (matter.file_locked, with its
 * files) gets lockedRestore's Word or lock dialog. That dialog needs what restoring writes, which the rejection didn't
 * give, so the preview is asked again (lockedRestore does that). Null otherwise, and when the dialog would list
 * nothing: the code's line says why. */
export async function refusedPreview(
  id: string,
  undoes: string | null,
  error: HostError,
  previewOf: (id: string) => Promise<RestorePreview | null>,
): Promise<Ask | null> {
  console.warn(`preview_restore failed: ${JSON.stringify(error)}`);
  if (error.code !== "matter.file_locked" || !error.files?.length) return null;
  const none: RestorePreview = { changes: [], added_since: [], kept: [], locked: [] };
  const ask = await lockedRestore(
    { act: "restore", id, preview: none, undoes, stage: "confirm", failed: null },
    error,
    previewOf,
  );
  return ask.stage === "word" || (ask.act === "restore" && ask.preview !== none) ? ask : null;
}
