// What Versions' controller (controller.ts) reads from the view store beyond the matter's timeline: every older page of
// a stream, the newest change that couldn't be kept as a version, and whether the matter's stream has gained an entry
// that isn't the room's last act's own (which ends the line saying what the act did).
import type { ViewStore } from "../../store/store.ts";
import { streamOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId, ThreadSummary, TurnId, ViewEntry } from "../../transport/protocol.ts";
import { orderOf, pageBefore } from "../matter-stream.ts";
import type { Unsaved } from "./state.ts";

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

const isOwn = (e: ViewEntry, own: Own): boolean =>
  own.checkpoint !== null
    ? e.version?.checkpoint === own.checkpoint ||
      e.marker?.kind.version_restored?.checkpoint === own.checkpoint
    : own.turn !== null && e.version?.kind === "undo" && e.version.turn === own.turn;

/** Whether the matter's stream holds an entry it didn't hold as the act was sent (`base`) and that isn't the act's. */
export function movedPast(view: StreamView, base: ReadonlySet<string>, own: Own): boolean {
  for (const id of view.ids) {
    const e = view.entries.get(id);
    if (!base.has(id) && e !== undefined && !isOwn(e, own)) return true;
  }
  return false;
}
