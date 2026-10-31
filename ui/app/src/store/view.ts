// One stream's view as the window holds it, and the pure fold of the host's patch batches into it. Entries live in
// one map keyed by entry id; streaming text lives in its own map, so an `AppendText` replaces one string and never
// an entry object, and a row that reads only its entry is never rendered again while another row's text grows.
// A patch applies only at the next cursor of the view's epoch; a replayed one is a no-op. A `Reset` replaces the view.
// An `AppendText` whose `from_len` is not the text's UTF-16 length, or a gap in the cursors, asks the caller to
// subscribe again after the last applied cursor.
import type {
  Cursor,
  EntryId,
  PatchBatch,
  StreamHeader,
  StreamId,
  ViewEntry,
  ViewEpoch,
  ViewPatch,
  ViewPosition,
  ViewWindow,
} from "../transport/protocol.ts";

export type StreamView = {
  readonly stream: StreamId;
  /** The epoch the cursor belongs to; null before the first `Reset`. */
  readonly epoch: ViewEpoch | null;
  /** The last cursor applied; null before the first `Reset`. */
  readonly cursor: Cursor | null;
  readonly header: StreamHeader | null;
  /** Every entry but the header, by id. An agent text's and a user message's text blocks read from `texts`. */
  readonly entries: ReadonlyMap<EntryId, ViewEntry>;
  /** The entry ids, sorted by `order`. The same array while no entry is added or removed. */
  readonly ids: readonly EntryId[];
  /** Streaming text by text key (`textKey`): an agent text's text, a user message's text blocks. A plain object, not
   * a Map: copying it per batch measured faster (0.069 ms vs 0.097 ms median per scripted turn, Apple M6, node 26). */
  readonly texts: Readonly<Record<string, string>>;
  readonly hasOlder: boolean;
};

export const emptyView = (stream: StreamId): StreamView => ({
  stream,
  epoch: null,
  cursor: null,
  header: null,
  entries: new Map(),
  ids: [],
  texts: {},
  hasOlder: false,
});

export const positionOf = (view: StreamView | undefined): ViewPosition | null =>
  view === undefined || view.epoch === null || view.cursor === null
    ? null
    : { epoch: view.epoch, cursor: view.cursor };

/** Where a view entry's id comes from: its kind, and the field holding the domain id (crates/protocol view.rs). */
const ID_FIELD = {
  header: "thread",
  user_message: "message",
  agent_text: "message",
  agent_content: "event",
  tool_call: "tool_call",
  permission: "request",
  elicitation: "elicitation",
  step: "turn",
  limit_stop: "turn",
  notice: "event",
  marker: "event",
  matter_header: "matter",
  version: "checkpoint",
} as const;

type Kind = keyof typeof ID_FIELD;

/** The entry's kind: its one key. */
export function kindOf(entry: ViewEntry): Kind {
  for (const key in entry) return key as Kind;
  throw new Error("a view entry has no kind");
}

/** The body of an entry, whatever its kind. */
function bodyOf(entry: ViewEntry): { order: number } & Record<string, unknown> {
  return (entry as unknown as Record<string, { order: number } & Record<string, unknown>>)[
    kindOf(entry)
  ]!;
}

/** `<kind>:<domain id>`, as `ViewEntry::id` builds it. */
export function entryIdOf(entry: ViewEntry): EntryId {
  const kind = kindOf(entry);
  return `${kind}:${String(bodyOf(entry)[ID_FIELD[kind]])}`;
}

export const orderOf = (entry: ViewEntry): number => bodyOf(entry).order;

/** The key of a text in `texts`: an agent text's entry id, or a user message's entry id and block index. */
export const textKey = (entryId: EntryId, block?: number): string =>
  block === undefined ? entryId : `${entryId}#${block}`;

/** What applying a batch did. */
export type Applied = {
  readonly view: StreamView;
  /** Whether any patch applied (the cursor moved or a `Reset` replaced the view). */
  readonly advanced: boolean;
  /** Set when the view cannot follow the batch: subscribe again after `positionOf(view)`. */
  readonly resubscribe: boolean;
};

/** A view being changed inside one batch: entries, ids and texts each copied at most once, on its first write. */
class Draft {
  view: StreamView;
  cursor: Cursor | null;
  #entries: Map<EntryId, ViewEntry> | null = null;
  #texts: Record<string, string> | null = null;
  #ids: EntryId[] | null = null;

  constructor(view: StreamView) {
    this.view = view;
    this.cursor = view.cursor;
  }

  get entries(): Map<EntryId, ViewEntry> {
    this.#entries ??= new Map(this.view.entries);
    return this.#entries;
  }
  get texts(): Record<string, string> {
    this.#texts ??= { ...this.view.texts };
    return this.#texts;
  }
  get ids(): EntryId[] {
    this.#ids ??= this.view.ids.slice();
    return this.#ids;
  }
  readText(key: string): string | undefined {
    return (this.#texts ?? this.view.texts)[key];
  }
  hasEntry(id: EntryId): boolean {
    return (this.#entries ?? this.view.entries).has(id);
  }
  orderAt(i: number): number {
    const ids = this.#ids ?? this.view.ids;
    const entry = (this.#entries ?? this.view.entries).get(ids[i]!);
    return entry === undefined ? 0 : orderOf(entry);
  }
  idsLength(): number {
    return (this.#ids ?? this.view.ids).length;
  }

  done(): StreamView {
    const same = this.#entries === null && this.#texts === null && this.#ids === null;
    if (same && this.cursor === this.view.cursor) return this.view;
    return {
      ...this.view,
      cursor: this.cursor,
      entries: this.#entries ?? this.view.entries,
      texts: this.#texts ?? this.view.texts,
      ids: this.#ids ?? this.view.ids,
    };
  }
}

/** Puts an entry's texts in the text map. */
function takeTexts(texts: Record<string, string>, id: EntryId, entry: ViewEntry): void {
  if (entry.agent_text !== undefined) {
    texts[textKey(id)] = entry.agent_text.text;
  } else if (entry.user_message !== undefined) {
    entry.user_message.content.forEach((block, i) => {
      if (block.text !== undefined) texts[textKey(id, i)] = block.text.text;
    });
  }
}

function upsert(draft: Draft, entry: ViewEntry): void {
  const kind = kindOf(entry);
  if (kind === "header" || kind === "matter_header") {
    draft.view = {
      ...draft.view,
      header:
        entry.header !== undefined
          ? { header: entry.header }
          : { matter_header: entry.matter_header! },
    };
    return;
  }
  const id = entryIdOf(entry);
  if (!draft.hasEntry(id)) {
    // Entries usually arrive newest last; look from the end for the place that keeps `order` sorted.
    const order = orderOf(entry);
    let at = draft.idsLength();
    while (at > 0 && draft.orderAt(at - 1) > order) at -= 1;
    draft.ids.splice(at, 0, id);
  }
  draft.entries.set(id, entry);
  takeTexts(draft.texts, id, entry);
}

function remove(draft: Draft, id: EntryId): void {
  if (id.startsWith("header:") || id.startsWith("matter_header:")) {
    draft.view = { ...draft.view, header: null };
    return;
  }
  if (!draft.hasEntry(id)) return;
  draft.entries.delete(id);
  const at = draft.ids.indexOf(id);
  if (at >= 0) draft.ids.splice(at, 1);
  for (const key of Object.keys(draft.texts)) {
    if (key === id || key.startsWith(`${id}#`)) delete draft.texts[key];
  }
}

function fromWindow(stream: StreamId, epoch: ViewEpoch, cursor: Cursor, w: ViewWindow): StreamView {
  const entries = new Map<EntryId, ViewEntry>();
  const texts: Record<string, string> = {};
  const ids: EntryId[] = [];
  for (const entry of w.entries) {
    const id = entryIdOf(entry);
    if (!entries.has(id)) ids.push(id);
    entries.set(id, entry);
    takeTexts(texts, id, entry);
  }
  return {
    stream,
    epoch,
    cursor,
    header: w.header ?? null,
    entries,
    ids,
    texts,
    hasOlder: w.has_older,
  };
}

/** Applies one patch body that is not a `Reset`; false when the view cannot take it. */
function applyBody(draft: Draft, patch: ViewPatch): boolean {
  const body = patch.body;
  if (body.upsert !== undefined) {
    upsert(draft, body.upsert.entry);
    return true;
  }
  if (body.remove !== undefined) {
    remove(draft, body.remove.entry_id);
    return true;
  }
  if (body.append_text !== undefined) {
    const a = body.append_text;
    const key =
      a.target === "agent_text"
        ? textKey(`agent_text:${a.message_id}`)
        : textKey(`user_message:${a.message_id}`, a.target.user_block);
    const was = draft.readText(key);
    if (was === undefined || was.length !== a.from_len) return false;
    draft.texts[key] = was + a.text;
    return true;
  }
  // A body this version does not know: skip it, but keep the cursor moving.
  return true;
}

/** Folds one batch into a view. The view passed in is never changed. */
export function applyBatch(view: StreamView, batch: PatchBatch): Applied {
  let draft = new Draft(view);
  let advanced = false;
  for (const patch of batch.patches) {
    const v = draft.view;
    const cursor = draft.cursor;
    if (patch.body.reset !== undefined) {
      const fresh = v.epoch !== batch.epoch || cursor === null || patch.cursor > cursor;
      if (!fresh) continue;
      draft = new Draft(
        fromWindow(v.stream, batch.epoch, patch.cursor, patch.body.reset.view_window),
      );
      advanced = true;
      continue;
    }
    if (v.epoch !== batch.epoch || cursor === null) {
      // Another epoch without its `Reset`: nothing here can follow it.
      return { view: draft.done(), advanced, resubscribe: true };
    }
    if (patch.cursor <= cursor) continue;
    if (patch.cursor !== cursor + 1 || !applyBody(draft, patch)) {
      return { view: draft.done(), advanced, resubscribe: true };
    }
    draft.cursor = patch.cursor;
    advanced = true;
  }
  return { view: draft.done(), advanced, resubscribe: false };
}
