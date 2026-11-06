// The room's matters: the list, the matter that's open, and what each row says it's doing. One small store the rail and
// the frame read (useSyncExternalStore), built once per transport.
//
// With the NextHost's matters part (host-next.ts: H3, H5, H6 and H20's rows) the list is `list_matters`'s MatterInfo,
// each row's status is the host's (H5), and opening a matter sends open_matter. H4's app events (the app part) keep the
// list current, applied once a frame. H6's seen marks are the conversations' (conversations.ts). Without the matters
// part (a host without H5) the list is `list_matters`, with no dates, pins or statuses, and a row says what
// it's doing only from the thread headers the room holds: the open conversation's thread, and the threads of
// conversations the lawyer left while they were working, kept subscribed until they rest. A list answer older than the list shown is dropped.
import { frameBatch, type Batch } from "../store/batch.ts";
import type { ViewStore } from "../store/store.ts";
import { streamOf } from "../store/view.ts";
import { pickReopenMatter } from "../store/reopen.ts";
import { quietly } from "./open.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  FolderRefId,
  FolderScanned,
  FolderSupportKind,
  MatterId,
  MatterSummary,
  ThreadId,
  ThreadStatus,
  Timestamp,
} from "../transport/protocol.ts";
import {
  nextHostOf,
  type AppEvent,
  type MatterInfo,
  type MattersCommand,
  type MattersHost,
  type MatterStatus,
  type NextHost,
  type NextMatterSource,
} from "./host-next.ts";
import { localZone } from "./when.ts";

/** What a row says its matter is doing: the rail's MatterRow states. */
export type RowState =
  | "idle"
  | "working"
  | "question"
  | "permission"
  | "ready"
  | "stopped"
  | "limit"
  | "moved";

/** A row's state, with the moment it names: when it stopped, or when the limit resets (none when not given). */
export type RowStatus = { state: RowState; at?: Timestamp };

export type MattersView = {
  /** loading until the first list arrives; failed when it couldn't be read. */
  phase: "loading" | "ready" | "failed";
  /** Whether the host gives each matter's details (the NextHost's matters part): dates, pins, clients, statuses. */
  detailed: boolean;
  /** Every matter, the scratch matter too. Without details each has none: no dates, unpinned, its folder found. */
  matters: readonly MatterInfo[];
  /** The matter the room shows, or null. */
  open: MatterInfo | null;
  /** The open matter's folder isn't where it was (H3): the composer waits until it is located. */
  folderMissing: boolean;
  /** Each matter's row state, where it is doing something; a matter missing here is idle. */
  status: ReadonlyMap<MatterId, RowStatus>;
  /** When this view was made, and the lawyer's zone: today, and the clock times the rows name, are taken from them,
   * so nothing ticks at rest and a render reads no clock. */
  now: Timestamp;
  zone: string;
};

/** A thread working, or waiting on the lawyer: the statuses a left matter keeps its thread subscription for. A thread
 * paused at the limit is kept too, so its row keeps saying so until it rests or ends. */
const keeps = (status: ThreadStatus): boolean =>
  status === "starting" ||
  status === "running" ||
  status === "needs_you" ||
  (typeof status === "object" && ("queued" in status || "paused_at_limit" in status));

/** A matter as today's host lists it, with the details it can't give. */
const undetailed = (m: MatterSummary): MatterInfo => ({
  ...m,
  pinned: false,
  folder_state: "found",
  status: "idle",
});

/** A list with H4's changed rows in place of their matter's (a new matter's added last, as the newest) and the matters
 * that left it taken off. */
export function merged(
  list: readonly MatterInfo[],
  rows: readonly MatterInfo[],
  removed: readonly MatterId[],
): readonly MatterInfo[] {
  if (rows.length === 0 && removed.length === 0) return list;
  const gone = new Set(removed);
  const byId = new Map(rows.map((m) => [m.matter, m]));
  const out = list.filter((m) => !gone.has(m.matter)).map((m) => byId.get(m.matter) ?? m);
  const had = new Set(list.map((m) => m.matter));
  for (const m of rows) if (!had.has(m.matter) && !gone.has(m.matter)) out.push(m);
  return out;
}

/** H5's status as the row's state; a folder that moved says so over any status. */
export function rowOf(info: MatterInfo): RowStatus | null {
  if (info.folder_state === "missing") return { state: "moved" };
  const status: MatterStatus = info.status;
  switch (status) {
    case "idle":
      return null;
    case "working":
      return { state: "working" };
    case "needs_permission":
      return { state: "permission" };
    case "needs_answer":
      return { state: "question" };
    case "ready":
      return { state: "ready" };
    default:
      if ("stopped" in status) return { state: "stopped", at: status.stopped.at };
      return status.paused_at_limit.resets_at === undefined
        ? { state: "limit" }
        : { state: "limit", at: status.paused_at_limit.resets_at };
  }
}

/** Which of a matter's threads speaks for its row: waiting on the lawyer first, then the limit, then working. */
const RANK: Record<RowState, number> = {
  moved: 6,
  permission: 5,
  question: 4,
  limit: 3,
  working: 2,
  ready: 1,
  stopped: 1,
  idle: 0,
};

const sameStatus = (a: ReadonlyMap<MatterId, RowStatus>, b: ReadonlyMap<MatterId, RowStatus>) =>
  a.size === b.size &&
  [...a].every(([id, s]) => b.get(id)?.state === s.state && b.get(id)?.at === s.at);

/** The matter to open on start: with details, the one opened last, then the latest active (store/reopen.ts orders
 * them); without, the newest folder matter. The scratch matter only when it was the one opened last. */
function reopenOf(matters: readonly MatterInfo[]): MatterInfo | null {
  const opened = matters.filter((m) => m.last_opened !== undefined);
  const last = pickReopenMatter(opened);
  if (last !== null) return last;
  return pickReopenMatter(matters.filter((m) => m.kind !== "scratch"));
}

export class Matters {
  /** The host rows beyond today's, each part null where the host lacks it. */
  readonly next: NextHost;
  /** The matters part (H3, H5, H6, H20's rows): what `detailed` and the H5 behaviour key on. */
  readonly #rows: MattersHost | null;
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  readonly #batch: Batch;
  /** Each list read and each app event takes the next ticket; a list older than the one shown is dropped. */
  #ticket = 0;
  #shown = 0;
  /** App events heard since the last frame, applied together, and the ticket of the last one. */
  #heard: AppEvent[] = [];
  #heardAt = 0;
  #cancelHeard: (() => void) | null = null;
  /** Matters removed, and rows changed, before any list arrived: applied to the first list, which may not know yet. A
   * whole list heard (`all`) is newer than both, so it drops them. */
  readonly #removedEarly = new Set<MatterId>();
  readonly #changedEarly = new Map<MatterId, MatterInfo>();
  readonly #listeners = new Set<() => void>();
  #view: MattersView;
  /** Thread views the room watches without the matters part, for their headers: the open thread, and threads of left
   * matters still working. */
  readonly #held = new Map<ThreadId, { matter: MatterId; release: () => void }>();
  #thread: ThreadId | null = null;
  #stop: (() => void)[] = [];
  #live = false;

  /** `batch` schedules the app events' one apply: once a frame unless given. */
  constructor(transport: HostTransport, store: ViewStore, batch: Batch = frameBatch) {
    this.#transport = transport;
    this.#store = store;
    this.#batch = batch;
    this.next = nextHostOf(transport);
    this.#rows = this.next.matters;
    this.#view = {
      phase: "loading",
      detailed: this.#rows !== null,
      matters: [],
      open: null,
      folderMissing: false,
      status: new Map(),
      now: Date.now(),
      zone: localZone(),
    };
  }

  view = (): MattersView => this.#view;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Reads the list and listens for changes; returns the stop. Stopping releases every header the room holds. */
  start(): () => void {
    this.#live = true;
    const app = this.next.app;
    if (app !== null) this.#stop.push(app.subscribeApp((event) => this.#hear(event)));
    if (this.#rows === null) this.#stop.push(this.#store.subscribe(() => this.#headersChanged()));
    void this.refresh();
    return () => {
      this.#live = false;
      this.#cancelHeard?.();
      this.#cancelHeard = null;
      this.#heard = [];
      for (const stop of this.#stop.splice(0)) stop();
      for (const held of this.#held.values()) held.release();
      this.#held.clear();
      this.#thread = null;
    };
  }

  /** Reads the list again, and opens the matter to reopen if none is open. */
  async refresh(): Promise<void> {
    const ticket = (this.#ticket += 1);
    try {
      const matters =
        this.#rows === null
          ? ((await this.#transport.query("list_matters")).matters ?? []).map(undetailed)
          : await this.#rows.listMatters();
      // an answer older than the list shown (a later read's, or an app event's) changes nothing
      if (!this.#live || ticket < this.#shown) return;
      this.#shown = ticket;
      this.#list(matters);
    } catch (error) {
      console.warn(`list_matters failed: ${JSON.stringify(error)}`);
      if (this.#live && this.#view.phase === "loading") this.#set({ phase: "failed" });
    }
  }

  /** Opens a matter: the title, the rail's current row and the work move to it. With H5 the host hears it. */
  open(matter: MatterId): void {
    this.#open(matter, false);
  }

  /** Opens a matter; `always` tells the host even when it is already open (a matter just made, which the new list may
   * have opened already). */
  #open(matter: MatterId, always: boolean): void {
    const info = this.#view.matters.find((m) => m.matter === matter);
    if (info === undefined) return;
    if (this.#view.open?.matter !== matter) {
      this.#leaveThread();
      this.#set({ open: info });
    } else if (!always) return;
    this.#rows?.command({ open_matter: { matter } }).catch((error: unknown) => {
      console.warn(`open_matter failed: ${JSON.stringify(error)}`);
    });
  }

  /** The open conversation's thread, as the work reports it (U3's onThread). Without the matters part the room holds
   * its thread's view, so its matter's row and its conversation's row can say what it's doing and keep saying it once
   * left (U2's rule until H4). H6's seen marks are the conversations' (conversations.ts). */
  onThread = (thread: ThreadId | null): void => {
    if (thread === this.#thread) return;
    this.#leaveThread();
    this.#thread = thread;
    const open = this.#view.open;
    if (thread === null || open === null || this.#rows !== null) return;
    // the watch receives the thread's whole stream and the store keeps its whole view (entries and streamed text);
    // the room reads only the header's status, and for needs_you whether a permission entry is pending
    // U3: if U3's view store gains a header-only watch, use it here, so a left thread holds no entries
    if (!this.#held.has(thread))
      this.#held.set(thread, {
        matter: open.matter,
        release: this.#store.watch(streamOf(thread)),
      });
    this.#headersChanged();
  };

  /** Makes a matter and opens it: it ends shown, even when the list read after fails, and the host hears it opened
   * (H5). Rejects with the host's error, for the form to say. */
  async create(
    name: string,
    source: NextMatterSource,
    details: { client?: string; next_date?: string } = {},
  ): Promise<MatterId> {
    let matter: MatterId;
    if (this.#rows !== null) {
      const outcome = await this.#rows.command({ create_matter: { name, source, ...details } });
      if (outcome === "done" || !("matter_created" in outcome))
        throw new Error("create_matter gave no matter");
      matter = outcome.matter_created.matter;
    } else {
      if (source === "default_location") throw new Error("this host has no default location");
      const outcome = await this.#transport.dispatch({
        command_id: crypto.randomUUID(),
        command: { create_matter: { name, source } },
      });
      if (outcome === "done" || !("matter_created" in outcome))
        throw new Error("create_matter gave no matter");
      matter = outcome.matter_created.matter;
    }
    await this.refresh();
    if (this.#live && !this.#view.matters.some((m) => m.matter === matter))
      this.#fresh([...this.#view.matters, undetailed({ matter, name, kind: "folder" })]);
    this.#open(matter, true);
    return matter;
  }

  /** H5: pins a matter above every date, or unpins it. */
  async pin(matter: MatterId, pinned: boolean): Promise<void> {
    await this.#command({ update_matter: { matter, pinned } }, "update_matter");
  }

  /** H5: takes a matter off the list; its folder stays where it is. */
  async forget(matter: MatterId): Promise<void> {
    this.#fresh(this.#view.matters.filter((m) => m.matter !== matter));
    await this.#command({ forget_matter: { matter } }, "forget_matter");
  }

  /** H3: asks for the folder of a matter that moved, and tells the host where it is now. */
  async locate(matter: MatterId): Promise<void> {
    const picked = await this.#transport.pickFolder();
    if (picked === null) return;
    await this.#command({ relocate_matter: { matter, folder: picked.folder } }, "relocate_matter");
  }

  /** H11: shows the matter's folder in Finder or File Explorer; a refusal only goes to the console. */
  reveal(matter: MatterId): Promise<void> {
    return quietly("reveal", this.#transport.reveal({ matter }));
  }

  /** H20: where a new matter's folder goes, as the lawyer would write it; null without it, or when the read fails. */
  async defaultLocation(): Promise<string | null> {
    const location = this.next.location;
    if (location === null) return null;
    try {
      return await location.getDefaultLocation();
    } catch (error) {
      console.warn(`get_default_location failed: ${JSON.stringify(error)}`);
      return null;
    }
  }

  /** New matter's folder check (FolderSupport) of where a create with `source` would go: the host's reading, or
   * undefined for a fine folder. Rejects with the host's error, create's own refusal included. */
  async folderSupport(source: NextMatterSource): Promise<FolderSupportKind | undefined> {
    return (await this.#transport.query({ folder_support: { source } })).folder_support?.support;
  }

  /** The folder picker, for the form: the folder and how it reads, or null when the lawyer cancelled. */
  pickFolder(): Promise<{ folder: FolderRefId; display: string } | null> {
    return this.#transport.pickFolder();
  }

  /** R20a: the scan of a folder the picker gave, before it becomes a matter; its progress comes as `folder_scanned`
   * (onScan). Rejects with the host's error: a `matter.folder_*` refusal is create's own. */
  async scan(folder: FolderRefId): Promise<void> {
    await this.#transport.dispatch({
      command_id: crypto.randomUUID(),
      command: { scan_folder: { folder } },
    });
  }

  /** Stops the scan of `folder`; its last `folder_scanned` says `cancelled`. A refusal only goes to the console. */
  cancelScan(folder: FolderRefId): void {
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { cancel_scan: { folder } } })
      .catch((error: unknown) => console.warn(`cancel_scan failed: ${JSON.stringify(error)}`));
  }

  /** Hears each `folder_scanned` (an app event); returns the stop. Nothing without the app feed. */
  onScan(listener: (scanned: FolderScanned) => void): () => void {
    const app = this.next.app;
    if (app === null) return () => {};
    return app.subscribeApp((event) => {
      if ("folder_scanned" in event) listener(event.folder_scanned);
    });
  }

  async #command(command: MattersCommand, name: string): Promise<void> {
    if (this.#rows === null) return;
    try {
      await this.#rows.command(command);
    } catch (error) {
      console.warn(`${name} failed: ${JSON.stringify(error)}`);
    }
    await this.refresh();
  }

  /** An app event (H4): held until the frame, so a burst is one apply and one render. */
  #hear(event: AppEvent): void {
    // a scan's progress, up to four a second, is the form's alone (onScan): no ticket, no frame's apply
    if (!this.#live || "folder_scanned" in event) return;
    // before the first list there is no list to change a row of or take a matter off: keep the rows and the ids for
    // the list to come, and take no ticket, so the read on its way still applies
    if (this.#view.phase !== "ready" && "matters_changed" in event && !event.matters_changed.all) {
      for (const m of event.matters_changed.matters) {
        this.#changedEarly.set(m.matter, m);
        this.#removedEarly.delete(m.matter);
      }
      for (const id of event.matters_changed.removed) {
        this.#removedEarly.add(id);
        this.#changedEarly.delete(id);
      }
      return;
    }
    if ("matters_changed" in event && event.matters_changed.all) {
      this.#removedEarly.clear();
      this.#changedEarly.clear();
    }
    this.#heard.push(event);
    this.#heardAt = this.#ticket += 1;
    this.#cancelHeard ??= this.#batch(() => {
      this.#cancelHeard = null;
      this.#applyHeard();
    });
  }

  #applyHeard(): void {
    const events = this.#heard.splice(0);
    // a list read that started after the last event and was shown first is newer than all of them
    if (!this.#live || this.#heardAt < this.#shown) return;
    let matters = this.#view.matters;
    let changed = false;
    for (const event of events) {
      if ("matters_changed" in event) {
        const change = event.matters_changed;
        matters = change.all ? change.matters : merged(matters, change.matters, change.removed);
        changed = true;
      }
      // threads_changed is the conversations' (conversations.ts); a matter row's status is H5's
    }
    if (!changed) return;
    this.#shown = this.#heardAt;
    this.#list(matters);
  }

  /** A list the room made itself (a matter taken off, a matter just made): newer than any read still on its way. */
  #fresh(matters: readonly MatterInfo[]): void {
    this.#shown = this.#ticket += 1;
    this.#list(matters);
  }

  /** A new list: the open matter follows it, or, gone or not yet chosen, the matter to reopen takes its place. */
  #list(listed: readonly MatterInfo[]): void {
    const matters =
      this.#removedEarly.size === 0 && this.#changedEarly.size === 0
        ? listed
        : merged(listed, [...this.#changedEarly.values()], [...this.#removedEarly]);
    this.#removedEarly.clear();
    this.#changedEarly.clear();
    const was = this.#view.open;
    const open =
      (was === null ? undefined : matters.find((m) => m.matter === was.matter)) ??
      reopenOf(matters);
    if (open?.matter !== was?.matter) this.#leaveThread();
    this.#set({ phase: "ready", matters, open: open ?? null, status: this.#statusOf(matters) });
  }

  /** The open thread is left: its header stays held while it works or waits on the lawyer, else it goes. */
  #leaveThread(): void {
    const thread = this.#thread;
    this.#thread = null;
    if (thread !== null) this.#prune();
  }

  /** Lets go of each held header, but the open thread's, whose thread rests. */
  #prune(): void {
    for (const [thread, held] of this.#held) {
      if (thread === this.#thread) continue;
      const header = this.#store.view(streamOf(thread))?.header;
      // held before its header arrives, until it says; and while it works or waits on the lawyer
      if (header === null) continue;
      if (header !== undefined && "header" in header && keeps(header.header.status)) continue;
      held.release();
      this.#held.delete(thread);
    }
  }

  #headersChanged(): void {
    if (!this.#live || this.#rows !== null) return;
    this.#prune();
    const status = this.#statusOf(this.#view.matters);
    if (!sameStatus(status, this.#view.status)) this.#set({ status });
  }

  /** Each row's state: H5's, or without it the held headers'. */
  #statusOf(matters: readonly MatterInfo[]): ReadonlyMap<MatterId, RowStatus> {
    const status = new Map<MatterId, RowStatus>();
    if (this.#rows !== null) {
      for (const m of matters) {
        const row = rowOf(m);
        if (row !== null) status.set(m.matter, row);
      }
      return status;
    }
    for (const [thread, held] of this.#held) {
      const row = this.#threadRow(thread);
      if (row === null) continue;
      const was = status.get(held.matter);
      if (was === undefined || RANK[row.state] > RANK[was.state]) status.set(held.matter, row);
    }
    return status;
  }

  /** A held thread's header as a row state: working, a question or a permission, or the limit. */
  #threadRow(thread: ThreadId): RowStatus | null {
    const view = this.#store.view(streamOf(thread));
    const header = view?.header;
    if (header === null || header === undefined || !("header" in header)) return null;
    const s = header.header.status;
    if (s === "starting" || s === "running") return { state: "working" };
    if (s === "needs_you") {
      // a permission is a question, not the reverse: a pending permission entry says which
      for (const entry of view?.entries.values() ?? [])
        if ("permission" in entry && entry.permission.state === "pending")
          return { state: "permission" };
      return { state: "question" };
    }
    if (typeof s === "object" && "queued" in s) return { state: "working" };
    if (typeof s === "object" && "paused_at_limit" in s)
      return s.paused_at_limit.resets_at === undefined
        ? { state: "limit" }
        : { state: "limit", at: s.paused_at_limit.resets_at };
    return null;
  }

  #set(patch: Partial<MattersView>): void {
    const view = { ...this.#view, ...patch };
    view.folderMissing = view.open?.folder_state === "missing";
    view.now = Date.now();
    this.#view = view;
    for (const listener of this.#listeners) listener();
  }
}
