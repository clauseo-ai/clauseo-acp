// One stream's view as the window holds it, and the fold of the host's patch batches into it. The view is kept in
// place: entries and streaming texts live in maps that a patch changes key by key, so an `AppendText` writes one
// string and an `Upsert` of a known entry writes one map slot; nothing a patch does copies anything that grows with
// the thread. Only adding or removing an entry makes a new id list, once per batch. Readers (hooks.ts) take one value
// from the view (a text, an entry object, the id list, the header), and each of those changes identity exactly when
// its part changes, so a flush renders only the rows whose part changed.
// A patch applies only at the next cursor of the view's epoch; a replayed one is a no-op. A `Reset` replaces the view.
// An `AppendText` whose `from_len` is not the text's UTF-16 length, or a gap in the cursors, asks the caller to
// subscribe again after the last applied cursor.
import type {
  Cursor,
  EntryId,
  PatchBatch,
  StreamHeader,
  StreamId,
  ThreadId,
  ViewEntry,
  ViewEpoch,
  ViewPatch,
  ViewPosition,
  ViewWindow,
} from "../transport/protocol.ts";

/** A stream's view, read-only to its readers. The store changes it only inside a flush. */
export interface StreamView {
  readonly stream: StreamId;
  /** The epoch the cursor belongs to; null before the first `Reset`. */
  readonly epoch: ViewEpoch | null;
  /** The last cursor applied; null before the first `Reset`. */
  readonly cursor: Cursor | null;
  readonly header: StreamHeader | null;
  /** Every entry but the header, by id. An upsert puts a new entry object; an agent text's and a user message's text
   * blocks read from `texts`. */
  readonly entries: ReadonlyMap<EntryId, ViewEntry>;
  /** The entry ids, sorted by `order`. The same array while no entry is added or removed. */
  readonly ids: readonly EntryId[];
  /** Streaming text by text key (`textKey`): an agent text's text, a user message's text blocks. */
  readonly texts: ReadonlyMap<string, string>;
  readonly hasOlder: boolean;
}

/** The same view, writable: only `applyBatch` writes it. */
export type LiveView = {
  -readonly [K in keyof StreamView]: StreamView[K];
} & {
  readonly entries: Map<EntryId, ViewEntry>;
  readonly texts: Map<string, string>;
};

export const newView = (stream: StreamId): LiveView => ({
  stream,
  epoch: null,
  cursor: null,
  header: null,
  entries: new Map(),
  ids: [],
  texts: new Map(),
  hasOlder: false,
});

export const positionOf = (view: StreamView | undefined): ViewPosition | null =>
  view === undefined || view.epoch === null || view.cursor === null
    ? null
    : { epoch: view.epoch, cursor: view.cursor };

/** Every key of the `ViewEntry` union: its kinds. */
type Kind = keyof ViewEntry;

/** Where a view entry's id comes from: for each kind, the field of its body holding the domain id, as
 * crates/protocol view.rs's `view_entries!` table says. Typed over every kind of the generated union, so a kind
 * protocol adds fails typecheck here until it has a row, and a row naming a field the body lacks fails too. */
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
} as const satisfies { readonly [K in Kind]-?: keyof NonNullable<ViewEntry[K]> };

/** The stream of a thread's view, in protocol's text form. */
export const streamOf = (thread: ThreadId): StreamId => `thread:${thread}`;

/** An entry id from its kind and domain id, as `ViewEntry::id` builds it: `<kind>:<domain id>`. */
export const entryId = (kind: Kind, id: string): EntryId => `${kind}:${id}`;

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

/** The entry's id. */
export function entryIdOf(entry: ViewEntry): EntryId {
  const kind = kindOf(entry);
  return entryId(kind, String(bodyOf(entry)[ID_FIELD[kind]]));
}

export const orderOf = (entry: ViewEntry): number => bodyOf(entry).order;

/** The key of a text in `texts`: an agent text's entry id, or a user message's entry id and block index. */
export const textKey = (entry: EntryId, block?: number): string =>
  block === undefined ? entry : `${entry}#${block}`;

/** What applying a batch did. */
export type Applied = {
  /** Whether any patch applied: the view changed, so readers must look again. */
  readonly changed: boolean;
  /** Set when the view cannot follow the batch: subscribe again after `positionOf(view)`. */
  readonly resubscribe: boolean;
};

/** Puts an entry's texts in the text map. */
function takeTexts(texts: Map<string, string>, id: EntryId, entry: ViewEntry): void {
  if (entry.agent_text !== undefined) {
    texts.set(textKey(id), entry.agent_text.text);
  } else if (entry.user_message !== undefined) {
    entry.user_message.content.forEach((block, i) => {
      if (block.text !== undefined) texts.set(textKey(id, i), block.text.text);
    });
  }
}

/** One batch's writes to a view: the id list is copied at most once, on the first add or remove. */
class Writer {
  readonly view: LiveView;
  #ids: EntryId[] | null = null;

  constructor(view: LiveView) {
    this.view = view;
  }

  get ids(): readonly EntryId[] {
    return this.#ids ?? this.view.ids;
  }
  writableIds(): EntryId[] {
    this.#ids ??= this.view.ids.slice();
    return this.#ids;
  }
  /** Ends the batch: a new id list, if any, becomes the view's. */
  done(): void {
    if (this.#ids !== null) this.view.ids = this.#ids;
    this.#ids = null;
  }
}

function upsert(w: Writer, entry: ViewEntry): void {
  const view = w.view;
  const kind = kindOf(entry);
  if (kind === "header" || kind === "matter_header") {
    view.header =
      entry.header !== undefined
        ? { header: entry.header }
        : { matter_header: entry.matter_header! };
    return;
  }
  const id = entryIdOf(entry);
  if (!view.entries.has(id)) {
    // Entries usually arrive newest last; look from the end for the place that keeps `order` sorted.
    const order = orderOf(entry);
    const ids = w.ids;
    let at = ids.length;
    while (at > 0 && orderOf(view.entries.get(ids[at - 1]!)!) > order) at -= 1;
    w.writableIds().splice(at, 0, id);
  }
  view.entries.set(id, entry);
  takeTexts(view.texts, id, entry);
}

function remove(w: Writer, id: EntryId): void {
  const view = w.view;
  if (id.startsWith("header:") || id.startsWith("matter_header:")) {
    view.header = null;
    return;
  }
  const entry = view.entries.get(id);
  if (entry === undefined) return;
  view.entries.delete(id);
  const ids = w.writableIds();
  const at = ids.lastIndexOf(id);
  if (at >= 0) ids.splice(at, 1);
  view.texts.delete(textKey(id));
  entry.user_message?.content.forEach((_, i) => view.texts.delete(textKey(id, i)));
}

function reset(view: LiveView, epoch: ViewEpoch, cursor: Cursor, w: ViewWindow): void {
  view.entries.clear();
  view.texts.clear();
  const ids: EntryId[] = [];
  for (const entry of w.entries) {
    const id = entryIdOf(entry);
    if (!view.entries.has(id)) ids.push(id);
    view.entries.set(id, entry);
    takeTexts(view.texts, id, entry);
  }
  view.epoch = epoch;
  view.cursor = cursor;
  view.header = w.header ?? null;
  view.ids = ids;
  view.hasOlder = w.has_older;
}

/** Applies one patch body that is not a `Reset`; false when the view cannot take it. */
function applyBody(w: Writer, patch: ViewPatch): boolean {
  const body = patch.body;
  if (body.upsert !== undefined) {
    upsert(w, body.upsert.entry);
    return true;
  }
  if (body.remove !== undefined) {
    remove(w, body.remove.entry_id);
    return true;
  }
  if (body.append_text !== undefined) {
    const a = body.append_text;
    const key =
      a.target === "agent_text"
        ? textKey(entryId("agent_text", a.message_id))
        : textKey(entryId("user_message", a.message_id), a.target.user_block);
    const texts = w.view.texts;
    const was = texts.get(key);
    if (was === undefined || was.length !== a.from_len) return false;
    texts.set(key, was + a.text);
    return true;
  }
  // A body this version does not know: skip it, but keep the cursor moving.
  return true;
}

/** Folds one batch into a view, in place. Patches before one the view cannot take stay applied. */
export function applyBatch(view: LiveView, batch: PatchBatch): Applied {
  const w = new Writer(view);
  let changed = false;
  try {
    for (const patch of batch.patches) {
      if (patch.body.reset !== undefined) {
        const fresh =
          view.epoch !== batch.epoch || view.cursor === null || patch.cursor > view.cursor;
        if (!fresh) continue;
        w.done();
        reset(view, batch.epoch, patch.cursor, patch.body.reset.view_window);
        changed = true;
        continue;
      }
      // Another epoch without its `Reset`: nothing here can follow it.
      if (view.epoch !== batch.epoch || view.cursor === null) return { changed, resubscribe: true };
      if (patch.cursor <= view.cursor) continue;
      if (patch.cursor !== view.cursor + 1 || !applyBody(w, patch))
        return { changed, resubscribe: true };
      view.cursor = patch.cursor;
      changed = true;
    }
    return { changed, resubscribe: false };
  } finally {
    w.done();
  }
}
