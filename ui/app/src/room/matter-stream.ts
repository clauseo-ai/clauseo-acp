// The matter's own stream, `matter:<id>`: its header (MatterHeaderEntry), each version of its files (VersionEntry) and
// its markers (a restore, a relocation). The room's one subscription to it: every part that reads it (Versions, and
// U9's Word lines) holds it through the view store, which subscribes the host once however many hold it. The newest
// window comes with the subscription; older entries page in with page_entries while the view says some are left.
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  MatterHeaderEntry,
  MatterId,
  StreamId,
  VersionEntry,
  ViewEntry,
} from "../transport/protocol.ts";

/** The matter's stream, in protocol's text form. */
export const matterStreamOf = (matter: MatterId): StreamId => `matter:${matter}`;

/** Holds the matter's stream until the release it returns is called. */
export const watchMatter = (store: ViewStore, matter: MatterId): (() => void) =>
  store.watch(matterStreamOf(matter));

/** What the matter's stream holds, read from its entries, oldest first. */
export type MatterRead = {
  header: MatterHeaderEntry | null;
  versions: readonly VersionEntry[];
  /** Each restore's checkpoint, to the checkpoint it restored from. */
  restores: ReadonlyMap<string, string>;
};

/** Reads entries, oldest first: older pages first, then the view's window. */
export function readMatter(view: StreamView | undefined, older: readonly ViewEntry[]): MatterRead {
  const header =
    view?.header !== undefined && view.header !== null && "matter_header" in view.header
      ? (view.header.matter_header ?? null)
      : null;
  const versions: VersionEntry[] = [];
  const restores = new Map<string, string>();
  const seen = new Set<string>();
  const take = (e: ViewEntry) => {
    if (e.version !== undefined) {
      if (seen.has(e.version.checkpoint)) return;
      seen.add(e.version.checkpoint);
      versions.push(e.version);
    } else if (e.marker !== undefined && e.marker.kind.version_restored !== undefined) {
      const r = e.marker.kind.version_restored;
      restores.set(r.checkpoint, r.from);
    }
  };
  for (const e of older) take(e);
  for (const id of view?.ids ?? []) {
    const e = view?.entries.get(id);
    if (e !== undefined) take(e);
  }
  versions.sort((a, b) => a.order - b.order);
  return { header: header ?? null, versions, restores };
}

/** The order of an entry, whatever its kind. */
export function orderOf(entry: ViewEntry): number {
  for (const key in entry)
    return (entry as unknown as Record<string, { order: number }>)[key]!.order;
  return 0;
}

/** The page size for a matter's or a conversation's older entries. */
export const PAGE = 200;

/** One page of entries before `before` (an order), oldest first, and whether any older are left. */
export async function pageBefore(
  transport: HostTransport,
  stream: StreamId,
  before: number,
): Promise<{ entries: ViewEntry[]; more: boolean }> {
  const result = await transport.query({
    page_entries: { stream, before_order: before, limit: PAGE },
  });
  const entries = "entries" in result ? result.entries : [];
  return { entries, more: entries.length >= PAGE };
}
