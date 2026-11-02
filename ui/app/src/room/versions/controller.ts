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
  ViewEntry,
} from "../../transport/protocol.ts";
import { matterStreamOf, readMatter } from "../matter-stream.ts";
import { localZone } from "../when.ts";
import { wordLock } from "../word/lock.ts";
import { Conversations } from "./conversations.ts";
import {
  DoneWatch,
  lockedRestore,
  pageBack,
  stepsSig,
  unsavedOf,
  type Own,
  type Undid,
} from "./reading.ts";
import {
  changedSince,
  defaultChoices,
  freshSnapshot,
  inRowOrder,
  keepChangedSince,
  outcomeOf,
  sameChanges,
  staleNewest,
  type Ask,
  type Done,
  type Look,
  type Outcome,
  type Snapshot,
  type Unsaved,
} from "./state.ts";
import { StepSeeker } from "./seeking.ts";
import { newestUndo, timelineOf, undoable, undoesAsRestore, type VersionMeta } from "./timeline.ts";

export type { Ask, Done, Look, Snapshot, Unsaved };

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
  readonly #steps: StepSeeker;
  #sig = "";
  #at: string | null = null;
  /** Undo was asked for from the toolbar: the dialog opens once the newest undoable step is in. */
  #undoNewest = false;
  /** The dialog the toolbar's Undo opened, and where Cancel goes back to: out of Versions, or to where the lawyer was. */
  #fromToolbar: { id: string | null; back: Look | "leave" } | null = null;
  #acting = false;
  /** The files of the open undo dialog the lawyer chose for: a move of the matter leaves their choices. */
  #chosen = new Set<string>();
  /** The newest comparison asked for. */
  #compared = 0;
  /** The newest preview asked for: one that comes back after a newer ask, or after the dialog closed, is dropped. */
  #asked = 0;
  /** The line saying what the last act did. */
  readonly #done = new DoneWatch();
  /** As the last refusal said an answer runs: the next list_threads' number, and how many conversations had rested. */
  #refusedAt = { list: 0, rests: 0 };

  constructor(transport: HostTransport, store: ViewStore, matter: MatterId) {
    this.#transport = transport;
    this.#store = store;
    this.matter = matter;
    this.#stream = matterStreamOf(matter);
    const again = () => {
      this.#sig = "";
      this.#read();
    };
    this.#talks = new Conversations(transport, store, matter, again);
    this.#steps = new StepSeeker(transport, store, again);
    this.#snap = freshSnapshot(Date.now(), localZone());
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
      this.#steps.release();
    };
  }

  /* ---------- reading ---------- */

  /** Re-reads what the store holds, and makes a new snapshot only when something Versions shows changed. */
  #read(): void {
    const view = this.#store.view(this.#stream);
    const talks = this.#talks;
    talks.read();
    const statuses = talks.threads.map((t) => JSON.stringify(talks.statusOf(t))).join(",");
    // the steps of every conversation the room holds: a step undone there changes what Undo offers; their streamed
    // text doesn't count
    const steps = talks.threads
      .map((t) => stepsSig(this.#store.view(streamOf(t.thread))))
      .join(",");
    const sig = [
      view?.epoch,
      view?.cursor,
      this.#older?.epoch,
      this.#older?.entries.length,
      statuses,
      talks.threads.length,
      talks.listed,
      steps,
      this.#steps.sig(),
    ].join("|");
    if (sig === this.#sig) return;
    this.#sig = sig;
    const epoch = view?.epoch ?? null;
    const at = view?.cursor == null ? null : `${epoch}:${view.cursor}`;
    const moved = at !== null && at !== this.#at && this.#at !== null;
    if (at !== null) this.#at = at;
    // a new version or marker: a turn began or ended somewhere in the matter, so the conversations' statuses moved,
    // and what a restore would write may have too
    if (moved) {
      talks.list();
      this.#repreview();
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
      step: this.#steps.stepOf,
      now,
      zone: this.#snap.zone,
    });
    const busy = talks.busy();
    // a step sought in vain that came after all: the toolbar's Undo finds it
    const found = this.#steps.found(timeline);
    // a refusal said an answer runs: it stands until a list_threads asked after it, or a conversation coming to rest,
    // leaves none running
    const settled =
      busy === null &&
      (talks.listed >= this.#refusedAt.list || talks.rests > this.#refusedAt.rests);
    this.#set({
      ready: epoch !== null,
      timeline,
      look: staleNewest(this.#snap.look, timeline),
      busy,
      refused: this.#snap.refused && !settled,
      titles,
      restores: read.restores,
      now,
      unsaved: unsavedOf(this.#store, talks.threads),
      canUndo: newestUndo(timeline, this.#steps.missing, read.restores) !== null,
      done: this.#done.after(this.#snap.done, view, this.#store, timeline),
    });
    if (moved) this.#afterMove();
    if (found) this.#rewalk();
    this.#steps.seek();
    if (this.#undoNewest) this.#openNewestUndo();
  }

  /** The matter moved: the undo dialog keeps each file changed since, and the comparison is asked for again. */
  #afterMove(): void {
    const { ask, look, timeline } = this.#snap;
    if (ask?.act === "undo" && ask.stage === "confirm") {
      const choices = keepChangedSince(timeline, ask.id, ask.choices, this.#chosen);
      if (choices !== null) this.#set({ ask: { ...ask, choices } });
    }
    if (look.kind === "back" && look.comparing) void this.#compare(look.id, true);
  }

  /** A dialog the toolbar's Undo opened for an older turn, untouched, while the newest turn's step came late: the
   * toolbar's Undo walks again, to the newest. */
  #rewalk(): void {
    const { ask } = this.#snap;
    const from = this.#fromToolbar;
    if (from === null || ask?.act !== "undo" || ask.stage !== "confirm" || ask.id !== from.id)
      return;
    if (this.#chosen.size > 0 || this.#acting) return;
    const next = newestUndo(this.#snap.timeline, this.#steps.missing, this.#snap.restores);
    if (next === null || next.id === ask.id) return;
    this.#set({ ask: null });
    this.#undoNewest = true;
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
    const { history } = this.#snap.timeline;
    const last = history.at(-1)?.id;
    const at = id === null || id === last ? null : id;
    // the newest version is now; a turn's, or a restore the host marked, stays picked, so the band can offer its Undo
    const kind = id === null ? undefined : this.meta(id)?.kind;
    const newest =
      id !== null &&
      id === last &&
      (kind === "after_turn" || undoesAsRestore(this.#snap.timeline, this.#snap.restores, id))
        ? id
        : null;
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
    this.#steps.hold(this.meta(at ?? newest ?? ""));
    this.#set({ look, done: null, failed: null, compare: null });
    if (look.kind === "back" && look.comparing) void this.#compare(look.id);
  }

  /** Compare with now: on, it asks what differs between the version and the folder now. */
  toggleCompare(): void {
    const look = this.#snap.look;
    if (look.kind !== "back") return;
    const comparing = !look.comparing;
    this.#set({ look: { ...look, comparing }, compare: null });
    if (comparing) void this.#compare(look.id);
  }

  /** Asks what differs between a version and the folder now; again: the matter moved, and the list shown stays until
   * the answer. Only the newest ask's answer is shown; an answer of the wrong shape is a failure. */
  async #compare(id: string, again = false): Promise<void> {
    this.#compared += 1;
    const asked = this.#compared;
    if (!again) this.#set({ compare: { id, changes: null } });
    const current = () => asked === this.#compared && this.#snap.compare?.id === id;
    try {
      const r = await this.#transport.query({
        compare_versions: { matter: this.matter, from: id },
      });
      if (!current()) return;
      if ("file_changes" in r) this.#set({ compare: { id, changes: r.file_changes } });
      else this.#set({ compare: null, failed: "internal" });
    } catch (e) {
      if (current()) this.#set({ compare: null, failed: toHostError(e).code });
    }
  }

  /* ---------- undoing and restoring ---------- */

  /** Opens the undo dialog for a turn's version. */
  askUndo(id: string): void {
    const m = this.meta(id);
    if (!undoable(m) || m?.step == null) return;
    const since = changedSince(this.#snap.timeline, id);
    const choices = defaultChoices(m.step, new Set(since.keys()));
    this.#chosen = new Set();
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
      const next = newestUndo(this.#snap.timeline, this.#steps.missing, this.#snap.restores);
      if (next === null) {
        // nothing to take back: Versions stays open on the timeline, and the toolbar's Undo waits
        this.#undoNewest = false;
        this.#fromToolbar = null;
        return;
      }
      const there = this.#snap.look.id === next.id;
      if (next.kind !== "unknown") {
        this.#undoNewest = false;
        if (!there) this.#look(next.id);
        if (this.#fromToolbar !== null) this.#fromToolbar = { ...this.#fromToolbar, id: next.id };
        // the newest version is a restore: Undo restores the version before it, through the restore dialog
        if (next.kind === "restore") this.undoRestore(next.id);
        else this.askUndo(next.id);
        return;
      }
      // its step isn't in: looking at it holds its conversation and pages back to it, then the walk goes on
      if (there) return;
      this.#look(next.id);
      const turn = this.meta(next.id)?.turn;
      if (turn === undefined || !this.#steps.missing.has(turn)) return;
    }
  }

  choose(path: string, choice: FileChoice): void {
    const ask = this.#snap.ask;
    if (ask?.act !== "undo" || ask.stage !== "confirm") return;
    const choices = new Map(ask.choices);
    choices.set(path, choice);
    this.#chosen.add(path);
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
      (error: unknown) => {
        // the list may be out of date: the dialog says the act failed, and Try again asks again
        const now = this.#snap.ask;
        if (asked !== this.#asked || now?.act !== "restore" || now.stage !== "confirm") return;
        if (now.id === ask.id) this.#set({ ask: { ...now, failed: toHostError(error).code } });
      },
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
    // a restore that undoes another was opened for the restore it undoes
    const opened = ask?.act === "restore" ? (ask.undoes ?? ask.id) : ask?.id;
    if (from === null || ask === null || opened !== from.id) return false;
    if (from.back === "leave") return true;
    this.#look(from.back.kind === "back" ? from.back.id : null);
    return false;
  }

  /** A restore or undo runs: the room ends Versions only once it is done (use-versions.ts's canLeave). */
  get busy(): boolean {
    return this.#acting;
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
      // undo_step's refusal doesn't name the file a program holds: an undo of one Word document can only mean Word holds
      // that one (word/lock.ts); otherwise the dialog keeps the lawyer's choices, says why, and offers Try again
      const word = wordLock(outcome.code, papers);
      await this.#refused(
        outcome.code,
        outcome.current
          ? () =>
              word === null
                ? { act: "undo", id, choices, stage: "confirm", failed: outcome.code }
                : { act: "undo", id, choices, stage: "word", file: word }
          : undefined,
      );
      return;
    }
    // the line names the files chosen, in the order the version's row names them, until the host's marker says
    // which it put back
    this.#finish(
      { kind: "undidChange", of: m.ts, papers: inRowOrder(this.#snap.timeline, id, papers) },
      { checkpoint: outcome.kind === "wrote" ? outcome.checkpoint : null, turn: step.turn },
      { thread: m.thread, turn: step.turn, id },
    );
  }

  async #restore(ask: RestoreAsk): Promise<void> {
    const { id, preview, undoes } = ask;
    const outcome = await this.#act({
      restore_version: { matter: this.matter, checkpoint: id, scope: "matter" },
    });
    if (outcome.kind === "refused") {
      await this.#refused(
        outcome.code,
        outcome.current
          ? () => lockedRestore(ask, outcome.code, (id) => this.#preview(id))
          : undefined,
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
  #finish(done: Done, own: Own, undid: Undid | null = null): void {
    this.#fromToolbar = null;
    this.#done.finished(own, undid);
    const view = this.#store.view(this.#stream);
    this.#set({
      ask: null,
      look: { kind: "now" },
      done: this.#done.after(done, view, this.#store, this.#snap.timeline),
    });
  }

  /** Sends one act; resolves with what it wrote, or the code the host refused it with (current: its dialog wasn't
   * closed meanwhile, so a dialog may open again to say why). */
  async #act(
    command: HostCommand,
  ): Promise<Outcome | { kind: "refused"; code: string; current: boolean }> {
    this.#acting = true;
    this.#done.sent(this.#store.view(this.#stream));
    this.#asked += 1;
    const asked = this.#asked;
    try {
      const outcome = outcomeOf(
        await this.#transport.dispatch({ command_id: crypto.randomUUID(), command }),
      );
      this.#set({ failed: null });
      return outcome;
    } catch (e) {
      return { kind: "refused", code: toHostError(e).code, current: asked === this.#asked };
    } finally {
      this.#acting = false;
      this.#set({}); // busy changed, though nothing else may have: the room's New conversation hears it
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
