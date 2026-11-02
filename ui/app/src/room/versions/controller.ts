// Versions' state for one open matter: the timeline, read from the matter's stream with the conversations' titles and
// statuses, and the lawyer's acts on it, each through the host: undo_step for one turn's change, restore_version for a
// whole version (and for undoing a restore: the version just before it), preview_restore for what a restore would
// write, compare_versions for what differs from the folder now. A small store the parts read with
// useSyncExternalStore: every change makes a new snapshot, and nothing changes it but the host's answers, the view
// store's flushes and the lawyer's acts, so it runs no timer and reads no clock at rest.
import type { ViewStore } from "../../store/store.ts";
import { streamOf } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { toHostError } from "../../transport/host-transport.ts";
import type {
  FileChoice,
  HostCommand,
  MatterId,
  RestorePreview,
  StepEntry,
  ThreadId,
  ThreadStatus,
  ThreadSummary,
  TurnId,
  VersionEntry,
  ViewEntry,
} from "../../transport/protocol.ts";
import { matterStreamOf, orderOf, pageBefore, readMatter } from "../matter-stream.ts";
import { localZone } from "../when.ts";
import {
  changedSince,
  defaultChoices,
  isBusy,
  type Ask,
  type Done,
  type Look,
  type Snapshot,
  type Unsaved,
} from "./state.ts";
import { before, timelineOf, undoable, type Timeline, type VersionMeta } from "./timeline.ts";

export type { Ask, Done, Look, Snapshot, Unsaved };

const EMPTY: Timeline = { history: [], meta: new Map() };

export class VersionsController {
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  readonly matter: MatterId;
  readonly #stream: string;
  #snap: Snapshot;
  readonly #listeners = new Set<() => void>();
  #stops: (() => void)[] = [];
  #older: ViewEntry[] = [];
  #paging = false;
  readonly #pagedSteps = new Map<TurnId, StepEntry>();
  #threads: readonly ThreadSummary[] = [];
  #listing: Promise<void> | null = null;
  #listAgain = false;
  #held: { thread: ThreadId; release: () => void } | null = null;
  #seeking: TurnId | null = null;
  #sig = "";
  #cursor: number | null = null;
  /** Undo was asked for from the toolbar: the dialog opens once the newest undoable step is in. */
  #undoNewest = false;
  #acting = false;

  constructor(transport: HostTransport, store: ViewStore, matter: MatterId) {
    this.#transport = transport;
    this.#store = store;
    this.matter = matter;
    this.#stream = matterStreamOf(matter);
    this.#snap = {
      ready: false,
      timeline: EMPTY,
      look: { kind: "now" },
      busy: null,
      refused: false,
      ask: null,
      done: null,
      failed: null,
      unsaved: null,
      compare: null,
      titles: new Map(),
      now: Date.now(),
      zone: localZone(),
    };
  }

  snapshot = (): Snapshot => this.#snap;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  #set(next: Partial<Snapshot>): void {
    this.#snap = { ...this.#snap, ...next };
    for (const fn of this.#listeners) fn();
  }

  /** Holds the matter's stream and reads it until the stop it returns. */
  start(): () => void {
    this.#stops.push(this.#store.watch(this.#stream));
    this.#stops.push(this.#store.subscribe(() => this.#read()));
    this.#list();
    this.#read();
    return () => {
      for (const stop of this.#stops) stop();
      this.#stops = [];
      this.#held?.release();
      this.#held = null;
    };
  }

  /* ---------- reading ---------- */

  #stepOf = (thread: ThreadId, turn: TurnId): StepEntry | undefined => {
    const e = this.#store.view(streamOf(thread))?.entries.get(`step:${turn}`);
    return e?.step ?? this.#pagedSteps.get(turn);
  };

  #statusOf(t: ThreadSummary): ThreadStatus {
    const h = this.#store.view(streamOf(t.thread))?.header;
    return h !== undefined && h !== null && "header" in h && h.header !== undefined
      ? h.header.status
      : t.status;
  }

  /** Re-reads what the store holds, and makes a new snapshot only when something Versions shows changed. */
  #read(): void {
    const view = this.#store.view(this.#stream);
    const statuses = this.#threads.map((t) => JSON.stringify(this.#statusOf(t))).join(",");
    const held = this.#held === null ? null : this.#store.view(streamOf(this.#held.thread));
    const sig = [
      view?.epoch,
      view?.cursor,
      this.#older.length,
      statuses,
      this.#threads.length,
      held?.cursor,
      this.#pagedSteps.size,
    ].join("|");
    if (sig === this.#sig) return;
    this.#sig = sig;
    if (view?.cursor !== undefined && view.cursor !== null && view.cursor !== this.#cursor) {
      const moved = this.#cursor !== null;
      this.#cursor = view.cursor;
      // a new version or marker: a turn began or ended somewhere in the matter, so the conversations' statuses moved
      if (moved) this.#list();
      if (moved && this.#snap.refused) this.#snap = { ...this.#snap, refused: false };
    }
    if (view?.hasOlder === true && this.#older.length === 0) void this.#pageMatter(view);
    const read = readMatter(view, this.#older);
    const titles = new Map(this.#threads.map((t) => [t.thread, t.title ?? ""]));
    const now = Date.now();
    const timeline = timelineOf({
      versions: read.versions,
      header: read.header,
      restores: read.restores,
      titles,
      untitled: "",
      step: this.#stepOf,
      now,
      zone: this.#snap.zone,
    });
    const busy = this.#threads.find((t) => isBusy(this.#statusOf(t)))?.thread ?? null;
    this.#versions = read.versions;
    this.#set({
      ready: view?.epoch !== null && view?.epoch !== undefined,
      timeline,
      busy,
      titles,
      now,
      unsaved: this.#unsavedOf(),
    });
    this.#seek();
    if (this.#undoNewest) this.#openNewestUndo();
  }

  #versions: readonly VersionEntry[] = [];

  /** The newest step of a held conversation with files and no version after it. */
  #unsavedOf(): Unsaved | null {
    let found: { order: number; u: Unsaved } | null = null;
    for (const t of this.#threads) {
      const view = this.#store.view(streamOf(t.thread));
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

  /** Every older page of the matter's stream, once: the timeline lists every version. */
  async #pageMatter(view: { ids: readonly string[]; entries: ReadonlyMap<string, ViewEntry> }) {
    if (this.#paging) return;
    this.#paging = true;
    const first = view.ids[0] === undefined ? undefined : view.entries.get(view.ids[0]);
    let before = first === undefined ? null : orderOf(first);
    const older: ViewEntry[] = [];
    try {
      while (before !== null) {
        const page = await pageBefore(this.#transport, this.#stream, before);
        older.unshift(...page.entries);
        const oldest = page.entries[0];
        before = page.more && oldest !== undefined ? orderOf(oldest) : null;
      }
    } catch (error: unknown) {
      console.warn(`page_entries failed: ${JSON.stringify(error)}`);
    }
    this.#older = older;
    this.#paging = false;
    this.#read();
  }

  /** list_threads, one at a time; asked again while one runs, it runs once more after. */
  #list(): void {
    if (this.#listing !== null) {
      this.#listAgain = true;
      return;
    }
    this.#listing = this.#transport
      .query({ list_threads: { matter: this.matter } })
      .then(
        (r) => {
          if ("threads" in r) this.#threads = r.threads;
        },
        (error: unknown) => console.warn(`list_threads failed: ${JSON.stringify(error)}`),
      )
      .finally(() => {
        this.#listing = null;
        this.#sig = "";
        this.#read();
        if (this.#listAgain) {
          this.#listAgain = false;
          this.#list();
        }
      });
  }

  /* ---------- looking back ---------- */

  meta(id: string): VersionMeta | undefined {
    return this.#snap.timeline.meta.get(id);
  }

  /** Looks at a version, or at now (null, or the newest). Choosing a turn's version holds its conversation and pages
   * back to its step, so Undo has its files. */
  look(id: string | null): void {
    const last = this.#snap.timeline.history.at(-1)?.id;
    const at = id === null || id === last ? null : id;
    const look: Look =
      at === null
        ? { kind: "now" }
        : {
            kind: "back",
            id: at,
            comparing: this.#snap.look.kind === "back" && this.#snap.look.comparing,
          };
    this.#hold(at === null ? undefined : this.meta(at));
    this.#set({ look, done: null, failed: null, compare: null });
    if (look.kind === "back" && look.comparing) void this.#compare(look.id);
  }

  #hold(m: VersionMeta | undefined): void {
    const thread = m?.kind === "after_turn" ? m.thread : undefined;
    if (this.#held?.thread !== thread) {
      this.#held?.release();
      this.#held =
        thread === undefined ? null : { thread, release: this.#store.watch(streamOf(thread)) };
    }
    this.#seeking = m?.kind === "after_turn" && m.step === null ? (m.turn ?? null) : null;
    this.#seek();
  }

  /** Pages the held conversation back until the sought step is in. */
  #seek(): void {
    const held = this.#held;
    const turn = this.#seeking;
    if (held === null || turn === null) return;
    if (this.#stepOf(held.thread, turn) !== undefined) {
      this.#seeking = null;
      return;
    }
    const stream = streamOf(held.thread);
    const view = this.#store.view(stream);
    if (view === undefined || view.epoch === null || !view.hasOlder) return;
    this.#seeking = null;
    const first = view.ids[0] === undefined ? undefined : view.entries.get(view.ids[0]);
    void (async () => {
      let before = first === undefined ? null : orderOf(first);
      try {
        while (before !== null && !this.#pagedSteps.has(turn)) {
          const page = await pageBefore(this.#transport, stream, before);
          for (const e of page.entries)
            if (e.step !== undefined) this.#pagedSteps.set(e.step.turn, e.step);
          const oldest = page.entries[0];
          before = page.more && oldest !== undefined ? orderOf(oldest) : null;
        }
      } catch (error: unknown) {
        console.warn(`page_entries failed: ${JSON.stringify(error)}`);
      }
      this.#read();
    })();
  }

  /** Compare with now: on, it asks what differs between the version and the folder now. */
  toggleCompare(): void {
    const look = this.#snap.look;
    if (look.kind !== "back") return;
    const comparing = !look.comparing;
    this.#set({ look: { ...look, comparing }, compare: null });
    if (comparing) void this.#compare(look.id);
  }

  async #compare(id: string): Promise<void> {
    this.#set({ compare: { id, changes: null } });
    try {
      const r = await this.#transport.query({
        compare_versions: { matter: this.matter, from: id },
      });
      if (this.#snap.compare?.id !== id) return;
      this.#set({ compare: { id, changes: "file_changes" in r ? r.file_changes : [] } });
    } catch (e) {
      if (this.#snap.compare?.id === id) this.#set({ compare: null, failed: toHostError(e).code });
    }
  }

  /* ---------- undoing and restoring ---------- */

  /** Opens the undo dialog for a turn's version. */
  askUndo(id: string): void {
    const m = this.meta(id);
    if (!undoable(m) || m?.step == null) return;
    const since = changedSince(this.#snap.timeline, id);
    this.#set({
      ask: {
        act: "undo",
        stage: "confirm",
        id,
        choices: defaultChoices(m.step, new Set(since.keys())),
      },
      failed: null,
    });
  }

  /** The toolbar's Undo: looks at the newest turn's version and opens its undo dialog once its step is in. */
  undoNewest(): void {
    this.#undoNewest = true;
    this.#openNewestUndo();
  }

  #openNewestUndo(): void {
    const { history, meta } = this.#snap.timeline;
    if (!this.#snap.ready) return;
    const newest = [...history]
      .reverse()
      .find((v) => meta.get(v.id)?.kind === "after_turn" && meta.get(v.id)?.step?.undone !== true);
    if (newest === undefined) {
      this.#undoNewest = false;
      return;
    }
    const m = meta.get(newest.id);
    if (m?.step === null) {
      if (this.#seeking === null && this.#held?.thread !== m.thread) this.look(newest.id);
      return;
    }
    this.#undoNewest = false;
    if (this.#snap.look.kind !== "back" || this.#snap.look.id !== newest.id) this.look(newest.id);
    this.askUndo(newest.id);
  }

  choose(path: string, choice: FileChoice): void {
    const ask = this.#snap.ask;
    if (ask?.act !== "undo" || ask.stage !== "confirm") return;
    const choices = new Map(ask.choices);
    choices.set(path, choice);
    this.#set({ ask: { ...ask, choices } });
  }

  /** Opens the restore dialog for a version, with what restoring it would write now. */
  async askRestore(id: string): Promise<void> {
    try {
      const r = await this.#transport.query({
        preview_restore: { matter: this.matter, checkpoint: id, scope: "matter" },
      });
      if (!("restore_preview" in r)) return;
      this.#set({
        ask: { act: "restore", stage: "confirm", id, preview: r.restore_preview },
        failed: null,
      });
    } catch (e) {
      this.#refused(toHostError(e).code, null);
    }
  }

  close(): void {
    this.#set({ ask: null });
  }

  /** Goes ahead with what the dialog asks. */
  async confirm(): Promise<void> {
    const ask = this.#snap.ask;
    if (ask === null || this.#acting) return;
    if (ask.act === "undo") await this.#undo(ask);
    else await this.#restore(ask.id, ask.stage === "confirm" ? ask.preview : null);
  }

  async #undo(ask: { id: string; choices?: ReadonlyMap<string, FileChoice> | undefined }) {
    const m = this.meta(ask.id);
    const step = m?.step;
    if (m?.thread === undefined || step == null) return;
    const choices = ask.choices ?? null;
    const files = step.files.map((f) => ({
      path: f.path,
      choice: choices?.get(f.path) ?? ("undo" as FileChoice),
    }));
    // undo_step needs at least one file to put back: with every file kept the act waits (the dialog holds it)
    if (!files.some((f) => f.choice === "undo")) return;
    const papers = files.filter((f) => f.choice === "undo").map((f) => f.path);
    const ok = await this.#act(
      { undo_step: { thread: m.thread, turn: step.turn, files } },
      // the file a program may hold: undo_step's refusal doesn't name it, so only an undo of one file can say which
      papers.length === 1 ? (papers[0] ?? "") : "",
      {
        act: "undo",
        stage: "word",
        id: ask.id,
        file: "",
        ...(choices === null ? {} : { choices }),
      },
    );
    // false is a refusal; any checkpoint, or none ("" for a bare done), is the undo made
    if (ok !== false)
      this.#set({
        ask: null,
        look: { kind: "now" },
        done: { kind: "undidChange", of: m.ts, papers },
      });
  }

  async #restore(id: string, preview: RestorePreview | null): Promise<void> {
    const n = preview?.changes.length ?? 0;
    // the file a program holds: the preview names it; failing that, a restore of one file can only mean that one
    const locked =
      preview?.locked[0] ?? (preview?.changes.length === 1 ? (preview.changes[0]?.path ?? "") : "");
    const ask = this.#snap.ask;
    const done = await this.#act(
      { restore_version: { matter: this.matter, checkpoint: id, scope: "matter" } },
      locked,
      ask ?? { act: "restore", stage: "word", id, file: locked },
    );
    if (done !== false)
      this.#set({
        ask: null,
        look: { kind: "now" },
        done: { kind: "restored", to: id, n, checkpoint: done },
      });
  }

  /** Undoes a restore: restores the version just before it. */
  async undoRestore(checkpoint: string): Promise<void> {
    const prior = before(this.#versions, checkpoint);
    if (prior === null || this.#acting) return;
    const done = await this.#act(
      { restore_version: { matter: this.matter, checkpoint: prior.checkpoint, scope: "matter" } },
      "",
      null,
    );
    if (done !== false)
      this.#set({ ask: null, look: { kind: "now" }, done: { kind: "undidRestore", at: prior.ts } });
  }

  /** Sends one act; resolves with the checkpoint it wrote, or false when the host refused it. */
  async #act(command: HostCommand, file: string, ask: Ask | null): Promise<string | false> {
    this.#acting = true;
    try {
      const outcome = await this.#transport.dispatch({
        command_id: crypto.randomUUID(),
        command,
      });
      this.#set({ failed: null });
      if (typeof outcome === "object" && "step_undone" in outcome)
        return outcome.step_undone.checkpoint;
      if (typeof outcome === "object" && "version_restored" in outcome)
        return outcome.version_restored.checkpoint;
      return "";
    } catch (e) {
      this.#refused(toHostError(e).code, ask === null ? null : { ...ask, file });
      return false;
    } finally {
      this.#acting = false;
    }
  }

  #refused(code: string, ask: (Ask & { file: string }) | null): void {
    if (code === "matter.turn_running" || code === "thread.turn_running") {
      // an answer runs after all: Restore and Undo wait, and the conversations' statuses are read again
      this.#set({ failed: code, refused: true });
      this.#list();
      return;
    }
    if (code === "matter.file_locked" && ask !== null && ask.file !== "") {
      // a program holds the file: the dialog says which, with Try again, and the page says why
      this.#set({
        failed: code,
        ask: {
          act: ask.act,
          stage: "word",
          id: ask.id,
          file: ask.file,
          ...(ask.act === "undo" && ask.choices !== undefined ? { choices: ask.choices } : {}),
        },
      });
      return;
    }
    this.#set({ failed: code, ask: null });
  }

  /** Try again, from the dialog that says a program holds the file. */
  async tryAgain(): Promise<void> {
    const ask = this.#snap.ask;
    if (ask?.stage !== "word" || this.#acting) return;
    if (ask.act === "restore") {
      // what the restore writes may have changed while the file was held: ask again, then restore
      try {
        const r = await this.#transport.query({
          preview_restore: { matter: this.matter, checkpoint: ask.id, scope: "matter" },
        });
        await this.#restore(ask.id, "restore_preview" in r ? r.restore_preview : null);
      } catch (e) {
        this.#refused(toHostError(e).code, ask);
      }
    } else {
      await this.#undo(ask);
    }
  }
}
