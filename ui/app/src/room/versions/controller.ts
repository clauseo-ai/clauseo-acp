// Versions' state for one open matter: the timeline, read from the matter's stream with the conversations' titles and
// statuses (conversations.ts), and the lawyer's acts on it, each through the host: undo_step for one turn's change,
// restore_version for a whole version, after preview_restore has said what it would write (undoing a restore too: the
// dialog asks to restore the version just before it), compare_versions for what differs from the folder now. A small
// store the parts read with useSyncExternalStore: every change makes a new snapshot, and nothing changes it but the
// host's answers, the view store's flushes and the lawyer's acts, so it runs no timer and reads no clock at rest.
import type { ViewStore } from "../../store/store.ts";
import { streamOf, type StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { toHostError } from "../../transport/host-transport.ts";
import type {
  FileChoice,
  HostCommand,
  MatterId,
  RestorePreview,
  StepEntry,
  ThreadId,
  TurnId,
  ViewEntry,
} from "../../transport/protocol.ts";
import { matterStreamOf, readMatter } from "../matter-stream.ts";
import { localZone } from "../when.ts";
import { Conversations } from "./conversations.ts";
import { lockedRestore, movedPast, pageBack, unsavedOf, type Own } from "./reading.ts";
import {
  changedSince,
  defaultChoices,
  sameChanges,
  type Ask,
  type Done,
  type Look,
  type Outcome,
  type Snapshot,
  type Unsaved,
} from "./state.ts";
import { newestUndo, timelineOf, undoable, type Timeline, type VersionMeta } from "./timeline.ts";

export type { Ask, Done, Look, Snapshot, Unsaved };

const EMPTY: Timeline = { history: [], meta: new Map() };

type UndoAsk = Extract<Ask, { act: "undo" }>;
type RestoreAsk = Extract<Ask, { act: "restore" }>;

export class VersionsController {
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  readonly matter: MatterId;
  readonly #stream: string;
  #snap: Snapshot;
  readonly #listeners = new Set<() => void>();
  #stops: (() => void)[] = [];
  readonly #talks: Conversations;
  /** The matter's older pages, for the epoch they were read in: a Reset starts the reading again. */
  #older: { epoch: string; entries: ViewEntry[] } | null = null;
  /** The epoch whose older pages are being read. */
  #paging: string | null = null;
  readonly #pagedSteps = new Map<TurnId, StepEntry>();
  #held: { thread: ThreadId; release: () => void } | null = null;
  #seeking: TurnId | null = null;
  #sig = "";
  #at: string | null = null;
  /** Undo was asked for from the toolbar: the dialog opens once the newest undoable step is in. */
  #undoNewest = false;
  /** The turns whose step was sought and isn't there: the toolbar's Undo walks past them. */
  readonly #missing = new Set<TurnId>();
  /** The dialog the toolbar's Undo opened, and where Cancel goes back to: out of Versions, or to where the lawyer was. */
  #fromToolbar: { id: string | null; back: Look | "leave" } | null = null;
  #acting = false;
  /** The newest preview asked for: one that comes back after a newer ask, or after the dialog closed, is dropped. */
  #asked = 0;
  /** The matter's entries as an act was sent: the line saying what it did goes once an entry not its own comes. */
  #base: ReadonlySet<string> = new Set();
  #doneBy: { base: ReadonlySet<string>; own: Own } | null = null;
  /** As the last refusal said an answer runs: the next list_threads' number, and how many conversations had rested. */
  #refusedAt = { list: 0, rests: 0 };

  constructor(transport: HostTransport, store: ViewStore, matter: MatterId) {
    this.#transport = transport;
    this.#store = store;
    this.matter = matter;
    this.#stream = matterStreamOf(matter);
    this.#talks = new Conversations(transport, store, matter, () => {
      this.#sig = "";
      this.#read();
    });
    this.#snap = {
      ready: false,
      timeline: EMPTY,
      look: { kind: "now" },
      busy: null,
      refused: false,
      ask: null,
      done: null,
      canUndo: true,
      failed: null,
      unsaved: null,
      compare: null,
      titles: new Map(),
      restores: new Map(),
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
    this.#talks.list();
    this.#read();
    return () => {
      for (const stop of this.#stops) stop();
      this.#stops = [];
      this.#talks.stop();
      this.#held?.release();
      this.#held = null;
    };
  }

  /* ---------- reading ---------- */

  #stepOf = (thread: ThreadId, turn: TurnId): StepEntry | undefined => {
    const e = this.#store.view(streamOf(thread))?.entries.get(`step:${turn}`);
    return e?.step ?? this.#pagedSteps.get(turn);
  };

  /** Re-reads what the store holds, and makes a new snapshot only when something Versions shows changed. */
  #read(): void {
    const view = this.#store.view(this.#stream);
    const talks = this.#talks;
    talks.read();
    const statuses = talks.threads.map((t) => JSON.stringify(talks.statusOf(t))).join(",");
    const held = this.#held === null ? null : this.#store.view(streamOf(this.#held.thread));
    const sig = [
      view?.epoch,
      view?.cursor,
      this.#older?.epoch,
      this.#older?.entries.length,
      statuses,
      talks.threads.length,
      talks.listed,
      held?.cursor,
      this.#pagedSteps.size,
    ].join("|");
    if (sig === this.#sig) return;
    this.#sig = sig;
    const epoch = view?.epoch ?? null;
    const at = view?.cursor == null ? null : `${epoch}:${view.cursor}`;
    if (at !== null && at !== this.#at) {
      const moved = this.#at !== null;
      this.#at = at;
      // a new version or marker: a turn began or ended somewhere in the matter, so the conversations' statuses moved,
      // and what a restore would write may have too
      if (moved) {
        talks.list();
        this.#repreview();
      }
    }
    if (this.#older !== null && this.#older.epoch !== epoch) this.#older = null;
    if (view !== undefined && epoch !== null && view.hasOlder && this.#older === null)
      void this.#pageMatter(view, epoch);
    const read = readMatter(view, this.#older?.entries ?? []);
    const titles = new Map(talks.threads.map((t) => [t.thread, t.title ?? ""]));
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
    const busy = talks.busy();
    // a refusal said an answer runs: it stands until a list_threads asked after it, or a conversation coming to rest,
    // leaves none running
    const settled =
      busy === null &&
      (talks.listed >= this.#refusedAt.list || talks.rests > this.#refusedAt.rests);
    this.#set({
      ready: epoch !== null,
      timeline,
      busy,
      refused: this.#snap.refused && !settled,
      titles,
      restores: read.restores,
      now,
      unsaved: unsavedOf(this.#store, talks.threads),
      canUndo: newestUndo(timeline, this.#missing) !== null,
      done: this.#doneAfter(this.#snap.done, view),
    });
    this.#seek();
    if (this.#undoNewest) this.#openNewestUndo();
  }

  /** The line for the room's last act stands until the matter's stream gains an entry not the act's own. */
  #doneAfter(done: Done | null, view: StreamView | undefined): Done | null {
    const by = this.#doneBy;
    if (done === null || by === null || view === undefined || !movedPast(view, by.base, by.own))
      return done;
    this.#doneBy = null;
    return null;
  }

  /** Every older page of the matter's stream for an epoch, once: the timeline lists every version. A page read for an
   * epoch a Reset has replaced is dropped, and the new epoch's are read. */
  async #pageMatter(view: StreamView, epoch: string): Promise<void> {
    if (this.#paging === epoch) return;
    this.#paging = epoch;
    const older = await pageBack(this.#transport, this.#stream, view);
    if (this.#paging === epoch) this.#paging = null;
    if (this.#store.view(this.#stream)?.epoch === epoch) this.#older = { epoch, entries: older };
    this.#sig = "";
    this.#read();
  }

  /* ---------- looking back ---------- */

  meta(id: string): VersionMeta | undefined {
    return this.#snap.timeline.meta.get(id);
  }

  /** The lawyer looks at a version, or at now (null, or the newest): a pending Undo from the toolbar is dropped. */
  look(id: string | null): void {
    this.#undoNewest = false;
    this.#fromToolbar = null;
    this.#asked += 1;
    this.#look(id);
  }

  /** Choosing a turn's version holds its conversation and pages back to its step, so Undo has its files. */
  #look(id: string | null): void {
    const last = this.#snap.timeline.history.at(-1)?.id;
    const at = id === null || id === last ? null : id;
    // the newest version is now; a turn's stays picked, so the band can offer its Undo
    const newest = id !== null && id === last && this.meta(id)?.kind === "after_turn" ? id : null;
    const look: Look =
      at === null
        ? newest === null
          ? { kind: "now" }
          : { kind: "now", id: newest }
        : {
            kind: "back",
            id: at,
            comparing: this.#snap.look.kind === "back" && this.#snap.look.comparing,
          };
    this.#hold(this.meta(at ?? newest ?? ""));
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

  /** Pages the held conversation back until the sought step is in; paged to its start without it, a pending Undo from
   * the toolbar is dropped. */
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
    if (view === undefined || view.epoch === null) return;
    this.#seeking = null;
    if (!view.hasOlder) {
      this.#missing.add(turn);
      return;
    }
    void pageBack(this.#transport, stream, view, (page) => {
      for (const e of page) if (e.step !== undefined) this.#pagedSteps.set(e.step.turn, e.step);
      return this.#pagedSteps.has(turn);
    }).then(() => {
      if (!this.#pagedSteps.has(turn)) this.#missing.add(turn);
      this.#sig = "";
      this.#read();
    });
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
    const choices = defaultChoices(m.step, new Set(since.keys()));
    this.#set({ ask: { act: "undo", id, choices, stage: "confirm", failed: null }, failed: null });
  }

  /** The toolbar's Undo: looks at the newest turn's version whose change can be undone, paging steps in as it goes,
   * and opens its undo dialog. opened: the press opened Versions too, so Cancel closes it again. */
  undoNewest(opened = false): void {
    this.#undoNewest = true;
    this.#fromToolbar = { id: null, back: opened ? "leave" : this.#snap.look };
    this.#openNewestUndo();
  }

  #openNewestUndo(): void {
    if (!this.#snap.ready) return;
    for (;;) {
      const next = newestUndo(this.#snap.timeline, this.#missing);
      if (next === null) {
        // nothing to take back: Versions stays open on the timeline, and the toolbar's Undo waits
        this.#undoNewest = false;
        this.#fromToolbar = null;
        return;
      }
      const there = this.#snap.look.id === next.id;
      if (next.kind === "ready") {
        this.#undoNewest = false;
        if (!there) this.#look(next.id);
        if (this.#fromToolbar !== null) this.#fromToolbar = { ...this.#fromToolbar, id: next.id };
        this.askUndo(next.id);
        return;
      }
      // its step isn't in: looking at it holds its conversation and pages back to it, then the walk goes on
      if (there) return;
      this.#look(next.id);
      const turn = this.meta(next.id)?.turn;
      if (turn === undefined || !this.#missing.has(turn)) return;
    }
  }

  choose(path: string, choice: FileChoice): void {
    const ask = this.#snap.ask;
    if (ask?.act !== "undo" || ask.stage !== "confirm") return;
    const choices = new Map(ask.choices);
    choices.set(path, choice);
    this.#set({ ask: { ...ask, choices } });
  }

  async #preview(id: string): Promise<RestorePreview | null> {
    const r = await this.#transport.query({
      preview_restore: { matter: this.matter, checkpoint: id, scope: "matter" },
    });
    return "restore_preview" in r ? r.restore_preview : null;
  }

  /** Opens the restore dialog for a version, with what restoring it would write now; undoes: the restore this one
   * takes back. Only the newest ask opens it. */
  async askRestore(id: string, undoes: string | null = null): Promise<void> {
    this.#asked += 1;
    const asked = this.#asked;
    try {
      const preview = await this.#preview(id);
      if (asked !== this.#asked || preview === null) return;
      this.#set({
        ask: { act: "restore", id, preview, undoes, stage: "confirm", failed: null },
        failed: null,
      });
    } catch (e) {
      if (asked === this.#asked) await this.#refused(toHostError(e).code);
    }
  }

  /** The matter moved while the restore dialog asks: what restoring writes now replaces the list it showed. */
  #repreview(): void {
    const ask = this.#snap.ask;
    if (ask?.act !== "restore" || ask.stage !== "confirm" || this.#acting) return;
    this.#asked += 1;
    const asked = this.#asked;
    this.#preview(ask.id).then(
      (preview) => {
        const now = this.#snap.ask;
        if (asked !== this.#asked || preview === null) return;
        if (now?.act === "restore" && now.stage === "confirm" && now.id === ask.id)
          this.#set({ ask: { ...now, preview } });
      },
      (error: unknown) => console.warn(`preview_restore failed: ${JSON.stringify(error)}`),
    );
  }

  /** Undoes a restore by restoring the version shown just before it, through the restore dialog, which lists what
   * that writes now. Only a restore version can be undone this way. */
  undoRestore(checkpoint: string): void {
    if (this.meta(checkpoint)?.kind !== "restore" || this.#acting) return;
    const { history } = this.#snap.timeline;
    const i = history.findIndex((v) => v.id === checkpoint);
    const prior = i > 0 ? history[i - 1] : undefined;
    if (prior !== undefined) void this.askRestore(prior.id, checkpoint);
  }

  /** Cancel: the dialog closes. One the toolbar's Undo opened goes back to where the lawyer was; true when that is out
   * of Versions, which the caller closes. */
  close(): boolean {
    this.#asked += 1;
    this.#undoNewest = false;
    const from = this.#fromToolbar;
    const ask = this.#snap.ask;
    this.#fromToolbar = null;
    this.#set({ ask: null });
    if (from === null || ask === null || ask.id !== from.id) return false;
    if (from.back === "leave") return true;
    this.#look(from.back.kind === "back" ? from.back.id : null);
    return false;
  }

  /** Goes ahead with what the dialog asks. */
  async confirm(): Promise<void> {
    const ask = this.#snap.ask;
    if (ask === null || this.#acting) return;
    if (ask.act === "undo") await this.#undo(ask);
    else await this.#restore(ask);
  }

  async #undo(ask: UndoAsk): Promise<void> {
    const m = this.meta(ask.id);
    const step = m?.step;
    if (m?.thread === undefined || step == null) return;
    const { id, choices } = ask;
    const files = step.files.map((f) => ({
      path: f.path,
      choice: choices.get(f.path) ?? ("undo" as FileChoice),
    }));
    // undo_step needs at least one file to put back: with every file kept the act waits (the dialog holds it)
    if (!files.some((f) => f.choice === "undo")) return;
    const papers = files.filter((f) => f.choice === "undo").map((f) => f.path);
    const outcome = await this.#act({ undo_step: { thread: m.thread, turn: step.turn, files } });
    if (outcome.kind === "refused") {
      // undo_step's refusal doesn't name the file a program holds: an undo of one file can only mean that one; with
      // more, the dialog keeps the lawyer's choices, says why, and offers Try again
      const only = papers.length === 1 ? papers[0] : undefined;
      await this.#refused(outcome.code, () =>
        only === undefined
          ? { act: "undo", id, choices, stage: "confirm", failed: outcome.code }
          : { act: "undo", id, choices, stage: "word", file: only },
      );
      return;
    }
    this.#finish(
      { kind: "undidChange", of: m.ts, papers },
      { checkpoint: outcome.kind === "wrote" ? outcome.checkpoint : null, turn: step.turn },
    );
  }

  async #restore(ask: RestoreAsk): Promise<void> {
    const { id, preview, undoes } = ask;
    const outcome = await this.#act({
      restore_version: { matter: this.matter, checkpoint: id, scope: "matter" },
    });
    if (outcome.kind === "refused") {
      await this.#refused(outcome.code, () =>
        lockedRestore(ask, outcome.code, (id) => this.#preview(id)),
      );
      return;
    }
    this.#finish(
      undoes === null
        ? { kind: "restored", to: id, n: preview.changes.length, outcome }
        : { kind: "undidRestore", at: this.meta(id)?.ts ?? this.#snap.now },
      { checkpoint: outcome.kind === "wrote" ? outcome.checkpoint : null, turn: null },
    );
  }

  /** An act done: back at now, with the line saying what it did. */
  #finish(done: Done, own: Own): void {
    this.#fromToolbar = null;
    this.#doneBy = { base: this.#base, own };
    this.#set({
      ask: null,
      look: { kind: "now" },
      done: this.#doneAfter(done, this.#store.view(this.#stream)),
    });
  }

  /** Sends one act; resolves with what it wrote, or the code the host refused it with. */
  async #act(command: HostCommand): Promise<Outcome | { kind: "refused"; code: string }> {
    this.#acting = true;
    this.#base = new Set(this.#store.view(this.#stream)?.ids ?? []);
    try {
      const outcome = await this.#transport.dispatch({
        command_id: crypto.randomUUID(),
        command,
      });
      this.#set({ failed: null });
      if (typeof outcome === "object" && "step_undone" in outcome)
        return { kind: "wrote", checkpoint: outcome.step_undone.checkpoint };
      if (typeof outcome === "object" && "version_restored" in outcome)
        return { kind: "wrote", checkpoint: outcome.version_restored.checkpoint };
      return { kind: "nothing" };
    } catch (e) {
      return { kind: "refused", code: toHostError(e).code };
    } finally {
      this.#acting = false;
    }
  }

  /** Says why the host refused; locked: the dialog to show when a program holds a file. */
  async #refused(code: string, locked?: () => Ask | Promise<Ask>): Promise<void> {
    if (code === "matter.turn_running" || code === "thread.turn_running") {
      // an answer runs after all: Restore and Undo wait, and the conversations' statuses are read again
      this.#refusedAt = { list: this.#talks.next, rests: this.#talks.rests };
      this.#set({ failed: code, refused: true });
      this.#talks.list();
      return;
    }
    if (code === "matter.file_locked" && locked !== undefined) {
      // a program holds a file: the dialog says which, with Try again, and the page says why
      this.#asked += 1;
      const asked = this.#asked;
      const ask = await locked();
      if (asked === this.#asked) this.#set({ failed: code, ask });
      return;
    }
    this.#set({ failed: code, ask: null });
  }

  /** Try again, from the dialog that says a program holds a file, or that the act failed. */
  async tryAgain(): Promise<void> {
    const ask = this.#snap.ask;
    if (ask === null || this.#acting || (ask.stage === "confirm" && ask.failed === null)) return;
    if (ask.act === "undo") {
      await this.#undo(ask);
      return;
    }
    // what the restore writes may have changed while it waited: asked again, a different list goes back to the
    // dialog for the lawyer to read, and the same goes ahead
    this.#asked += 1;
    const asked = this.#asked;
    let preview: RestorePreview | null;
    try {
      preview = await this.#preview(ask.id);
    } catch (e) {
      if (asked === this.#asked) await this.#refused(toHostError(e).code, () => ask);
      return;
    }
    if (asked !== this.#asked || preview === null) return;
    const next: RestoreAsk = {
      act: "restore",
      id: ask.id,
      preview,
      undoes: ask.undoes,
      stage: "confirm",
      failed: null,
    };
    if (sameChanges(ask.preview, preview)) await this.#restore(next);
    else this.#set({ ask: next, failed: null });
  }
}
