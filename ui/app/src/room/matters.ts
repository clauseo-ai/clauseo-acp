// The room's matters: the list, the matter that's open, and what each row says it's doing. One small store the rail and
// the frame read (useSyncExternalStore), built once per transport.
//
// With a NextHost (host-next.ts: H3 to H6 and H20) the list is `list_matters`'s MatterInfo, each row's status is the
// host's (H5), H4's app events keep it current, opening a matter sends open_matter and the open thread is marked seen
// (H6). Without one (today's desktop host) the list is today's `list_matters`, with no dates, pins or statuses, and a
// row says what it's doing only from the thread headers the room holds: the open matter's thread, and until H4, the
// thread of a matter the lawyer left while it was working, kept subscribed (header only) until it rests.
import type { ViewStore } from "../store/store.ts";
import { streamOf } from "../store/view.ts";
import { pickReopenMatter } from "../store/reopen.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  FolderRefId,
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
  /** Whether the host gives each matter's details (a NextHost): dates, pins, clients, statuses. */
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

/** A thread working, or waiting on the lawyer: the statuses a left matter keeps its header subscription for. A thread
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
  readonly next: NextHost | null;
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  readonly #listeners = new Set<() => void>();
  #view: MattersView;
  /** Thread headers the room holds without H4: the open thread, and threads of left matters still working. */
  readonly #held = new Map<ThreadId, { matter: MatterId; release: () => void }>();
  #thread: ThreadId | null = null;
  #stop: (() => void)[] = [];
  #live = false;

  constructor(transport: HostTransport, store: ViewStore) {
    this.#transport = transport;
    this.#store = store;
    this.next = nextHostOf(transport);
    this.#view = {
      phase: "loading",
      detailed: this.next !== null,
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
    const next = this.next;
    if (next !== null) this.#stop.push(next.subscribeApp((event) => this.#heard(event)));
    else this.#stop.push(this.#store.subscribe(() => this.#headersChanged()));
    void this.refresh();
    return () => {
      this.#live = false;
      for (const stop of this.#stop.splice(0)) stop();
      for (const held of this.#held.values()) held.release();
      this.#held.clear();
      this.#thread = null;
    };
  }

  /** Reads the list again, and opens the matter to reopen if none is open. */
  async refresh(): Promise<void> {
    try {
      const matters =
        this.next === null
          ? ((await this.#transport.query("list_matters")).matters ?? []).map(undetailed)
          : await this.next.listMatters();
      if (this.#live) this.#list(matters);
    } catch (error) {
      console.warn(`list_matters failed: ${JSON.stringify(error)}`);
      if (this.#live && this.#view.phase === "loading") this.#set({ phase: "failed" });
    }
  }

  /** Opens a matter: the title, the rail's current row and the work move to it. With H5 the host hears it. */
  open(matter: MatterId): void {
    const info = this.#view.matters.find((m) => m.matter === matter);
    if (info === undefined || this.#view.open?.matter === matter) return;
    this.#leaveThread();
    this.#set({ open: info });
    this.next?.matters({ open_matter: { matter } }).catch((error: unknown) => {
      console.warn(`open_matter failed: ${JSON.stringify(error)}`);
    });
  }

  /** The open matter's thread, as the work reports it (U3's onThread). Without H4 the room holds its header, so the
   * row can say what it's doing and keep saying it once left; with H6 the thread is marked seen. */
  onThread = (thread: ThreadId | null): void => {
    if (thread === this.#thread) return;
    this.#leaveThread();
    this.#thread = thread;
    const open = this.#view.open;
    if (thread === null || open === null) return;
    if (this.next !== null) {
      this.next.matters({ mark_seen: { thread } }).catch((error: unknown) => {
        console.warn(`mark_seen failed: ${JSON.stringify(error)}`);
      });
      return;
    }
    if (!this.#held.has(thread))
      this.#held.set(thread, {
        matter: open.matter,
        release: this.#store.watch(streamOf(thread)),
      });
    this.#headersChanged();
  };

  /** Makes a matter and opens it. Rejects with the host's error, for the form to say. */
  async create(
    name: string,
    source: NextMatterSource,
    details: { client?: string; next_date?: string } = {},
  ): Promise<MatterId> {
    let matter: MatterId;
    if (this.next !== null) {
      const outcome = await this.next.matters({ create_matter: { name, source, ...details } });
      if (outcome === "done") throw new Error("create_matter gave no matter");
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
    this.open(matter);
    return matter;
  }

  /** H5: pins a matter above every date, or unpins it. */
  async pin(matter: MatterId, pinned: boolean): Promise<void> {
    await this.#command({ update_matter: { matter, pinned } }, "update_matter");
  }

  /** H5: takes a matter off the list; its folder stays where it is. */
  async forget(matter: MatterId): Promise<void> {
    this.#list(this.#view.matters.filter((m) => m.matter !== matter));
    await this.#command({ forget_matter: { matter } }, "forget_matter");
  }

  /** H3: asks for the folder of a matter that moved, and tells the host where it is now. */
  async locate(matter: MatterId): Promise<void> {
    const picked = await this.#transport.pickFolder();
    if (picked === null) return;
    await this.#command({ relocate_matter: { matter, folder: picked.folder } }, "relocate_matter");
  }

  /** H20: where a new matter's folder goes, as the lawyer would write it; null without it, or when the read fails. */
  async defaultLocation(): Promise<string | null> {
    if (this.next === null) return null;
    try {
      return await this.next.getDefaultLocation();
    } catch (error) {
      console.warn(`get_default_location failed: ${JSON.stringify(error)}`);
      return null;
    }
  }

  /** The folder picker, for the form: the folder and how it reads, or null when the lawyer cancelled. */
  pickFolder(): Promise<{ folder: FolderRefId; display: string } | null> {
    return this.#transport.pickFolder();
  }

  async #command(command: Parameters<NextHost["matters"]>[0], name: string): Promise<void> {
    if (this.next === null) return;
    try {
      await this.next.matters(command);
    } catch (error) {
      console.warn(`${name} failed: ${JSON.stringify(error)}`);
    }
    await this.refresh();
  }

  #heard(event: AppEvent): void {
    if (!this.#live) return;
    if ("matters_changed" in event) this.#list(event.matters_changed.matters);
    else if ("matters_removed" in event) {
      const gone = new Set(event.matters_removed.matters);
      this.#list(this.#view.matters.filter((m) => !gone.has(m.matter)));
    }
    // threads_changed carries the threads' own marks, for the work's conversations (U3); a row's status is H5's
  }

  /** A new list: the open matter follows it, or, gone or not yet chosen, the matter to reopen takes its place. */
  #list(matters: readonly MatterInfo[]): void {
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
    if (!this.#live || this.next !== null) return;
    this.#prune();
    const status = this.#statusOf(this.#view.matters);
    if (!sameStatus(status, this.#view.status)) this.#set({ status });
  }

  /** Each row's state: H5's, or without it the held headers'. */
  #statusOf(matters: readonly MatterInfo[]): ReadonlyMap<MatterId, RowStatus> {
    const status = new Map<MatterId, RowStatus>();
    if (this.next !== null) {
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
