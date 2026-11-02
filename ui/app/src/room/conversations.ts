// The open matter's conversations: its threads as rows, the one open, the draft a New conversation makes, and what each
// row says it's doing. One small store the rail and the work read (useSyncExternalStore), built once per room; it
// follows the matter matters.ts has open.
//
// - The list is `list_threads` (with H6's seen marks and last stops where the NextHost's matters part gives them),
//   read each time a matter opens. Archived threads are hidden. H4's `threads_changed` keeps it current where the
//   host sends it. A thread's header in the view store (the open thread's, and the threads of conversations left
//   while they work, which matters.ts keeps subscribed until they rest: U2's rule until H4) is newer than the list, so
//   its title and status win, and are kept once its view goes. A held view that goes while its thread was working
//   reads the list again, so the row doesn't stay "Answering." after the turn's end slipped past.
// - New conversation is a draft row, the room's alone (U6, "Decided here"): nothing reaches the host until its first
//   send, which runs `create_thread`, then `send_prompt` (the session's), then `rename_thread` with the title its
//   first message gives (conversation-title.ts). Leaving a draft with nothing typed drops it.
// - Each conversation has a key: the work column's key and its composer draft's, so moving between conversations
//   remounts the work and each keeps its own words. A thread's key is its id, unless the room opened it before knowing
//   it (a matter just opened, before its list is in; a draft that became the thread): then it keeps that first key, so
//   the work isn't remounted under a question on its way.
// - H6: a thread is marked seen when it opens with the window focused, and when its turn ends while open and focused.
import { late } from "@clauseo/components/lib/late.ts";
import { pickReopenThread } from "../store/reopen.ts";
import type { ViewStore } from "../store/store.ts";
import { streamOf } from "../store/view.ts";
import { internalError, toHostError, type HostTransport } from "../transport/host-transport.ts";
import type {
  AgentId,
  ErrorCode,
  MatterId,
  MessageId,
  ThreadId,
  ThreadStatus,
  Timestamp,
} from "../transport/protocol.ts";
import { agentFor, matterPlan, planOptions } from "./agent.ts";
import type { AgentSummary, AppEvent, NextHost, ThreadInfo, ThreadsChanged } from "./host-next.ts";
import type { Matters } from "./matters.ts";

/** A thread as a row knows it: the list's summary, with H6's marks where given. */
type Known = Omit<ThreadInfo, "seen"> & { seen?: boolean };

/** One row: a thread, or the draft (no thread, no agent yet). */
export type ConversationRow = {
  key: string;
  thread: ThreadId | null;
  /** Its title; absent until it has one, when the row says "New conversation". */
  title?: string;
  /** Its agent; null for a draft, whose agent is fixed by its first send. */
  agent: AgentId | null;
  status: ThreadStatus;
  /** H6: false while its last answer waits to be seen; absent without H6. */
  seen?: boolean;
  last_stop?: Known["last_stop"];
};

export type ConversationsView = {
  matter: MatterId | null;
  /** loading until the matter's list is in; failed when it couldn't be read. */
  phase: "loading" | "ready" | "failed";
  /** The code of the refused list, until a read succeeds. */
  refused: ErrorCode | null;
  /** The draft first, if shown, then the threads, newest activity first; archived threads left out. */
  rows: readonly ConversationRow[];
  /** The key of the conversation the work shows; null with no matter open. */
  open: string | null;
  /** Its thread: null for a draft, or while the list loads. */
  thread: ThreadId | null;
  /** Whether the open conversation is a draft (no thread yet), once the list is in. */
  drafting: boolean;
  /** The plan the rows are measured against: the oldest thread's (agent.ts's matterPlan). */
  plan: string | undefined;
  /** The agents the host lists (R15), for the plan names. */
  agents: readonly AgentSummary[];
  /** The composer's plan choice for a draft: the agents the host lists (R15), none with one or none. */
  options: readonly { value: AgentId; label: string }[];
  /** The agent the draft would run: the lawyer's choice, else agent.ts's order. */
  agent: AgentId;
  /** The lawyer's last move to a conversation (New conversation, a row pressed, a retry): its key, and a number that
   * grows with each, so the work puts the focus in the field once per move. Null after the room moved by itself. */
  focus: { key: string; n: number } | null;
  /** Whether Retry from here can be offered (H13). */
  canRetry: boolean;
  /** H13: the open thread was retried, in this room, from a question of another: that one's key and title, and the
   * question's time. Until H13 says it on the thread, a restart forgets it. */
  retried: { from: string; title?: string; at: Timestamp } | null;
};

/** Each visited matter's conversations, kept while the room lives. */
type MatterState = {
  matter: MatterId;
  threads: Map<ThreadId, Known>;
  /** Threads known to the room by a key other than their id. */
  keys: Map<ThreadId, string>;
  /** The draft: its key, whether its row shows (New conversation was pressed), and the plan chosen for it. */
  draft: { key: string; shown: boolean; chosen: AgentId | null } | null;
  /** The conversation open, by key; null before the matter is first opened. */
  open: string | null;
  /** The key that stands for what opens before the list says what that is: the thread to reopen, or a draft. */
  pending: string | null;
  phase: ConversationsView["phase"];
  refused: ErrorCode | null;
  /** The refused list's error, for a first send that can't go without it. */
  refusal: unknown;
  /** The list read in flight, for a first send while it loads. */
  listing: Promise<void> | null;
  ticket: number;
  /** Threads given a rename by this room: one each. */
  renamed: Set<ThreadId>;
  /** H13: threads this room retried, with the thread and the time of the question they came from. */
  retried: Map<ThreadId, { from: ThreadId; at: Timestamp }>;
  /** Threads this room made (create_thread, retry_from): a list read that started before one was made still keeps it. */
  made: Set<ThreadId>;
  /** Threads this room made whose first question the host hasn't accepted yet (send_prompt refused): left with
   * nothing typed, they are archived, so no empty thread stays on the list. */
  unsent: Set<ThreadId>;
};

const resting = (s: ThreadStatus) => !(s === "starting" || s === "running" || s === "needs_you");

/** Newest activity first: H6's last activity, else the newest created (v7 ids sort by creation). */
const newestFirst = (a: Known, b: Known) => {
  const at = (b.last_activity ?? -Infinity) - (a.last_activity ?? -Infinity);
  if (at !== 0 && !Number.isNaN(at)) return at;
  return a.thread < b.thread ? 1 : a.thread > b.thread ? -1 : 0;
};

export class Conversations {
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  readonly #matters: Matters;
  readonly #next: NextHost;
  /** The composer drafts, by conversation key (session.ts's Drafts): an empty one drops its draft row on leaving. */
  readonly #drafts: Map<string, string>;
  readonly #focused: () => boolean;
  readonly #by = new Map<MatterId, MatterState>();
  readonly #listeners = new Set<() => void>();
  #agents: AgentSummary[] = [];
  #current: MatterState | null = null;
  #n = 0;
  #focus: { key: string; n: number } | null = null;
  /** Each held thread's header as last read: what changed, and which views went. */
  #seenHeaders = new Map<ThreadId, unknown>();
  #view: ConversationsView;
  #stop: (() => void)[] = [];
  #live = false;

  constructor(
    transport: HostTransport,
    store: ViewStore,
    matters: Matters,
    drafts: Map<string, string>,
    /** Whether the window has focus, for H6's marks: the document's, unless given. */
    focused: () => boolean = () => document.hasFocus(),
  ) {
    this.#transport = transport;
    this.#store = store;
    this.#matters = matters;
    this.#next = matters.next;
    this.#drafts = drafts;
    this.#focused = focused;
    this.#view = this.#make();
  }

  view = (): ConversationsView => this.#view;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Follows the open matter and listens for changes; returns the stop. */
  start(): () => void {
    this.#live = true;
    this.#stop.push(this.#matters.subscribe(() => this.#follow()));
    this.#stop.push(this.#store.subscribe(() => this.#headers()));
    const app = this.#next.app;
    if (app !== null) this.#stop.push(app.subscribeApp((event) => this.#hear(event)));
    const agents = this.#next.agents;
    if (agents !== null)
      agents.listAgents().then(
        (list) => {
          if (!this.#live) return;
          this.#agents = list;
          this.#emit();
        },
        (error: unknown) => console.warn(`list_agents failed: ${JSON.stringify(error)}`),
      );
    this.#follow();
    return () => {
      this.#live = false;
      for (const stop of this.#stop.splice(0)) stop();
    };
  }

  /** Opens a conversation by its key. Leaving a draft with nothing typed drops it. */
  open(key: string): void {
    const m = this.#current;
    if (m === null) return;
    if (m.open === key) {
      // the open one pressed again: the field takes the focus, as for any other
      this.#focusOn(key);
      this.#emit();
      return;
    }
    this.#leave(m);
    m.open = key;
    this.#opened(m);
    this.#focusOn(key);
    this.#emit();
  }

  #focusOn(key: string): void {
    this.#focus = { key, n: (this.#focus?.n ?? 0) + 1 };
  }

  /** New conversation: the draft opens, its row shown; a draft already there opens again. */
  startNew(): void {
    const m = this.#current;
    if (m === null) return;
    if (m.draft === null || m.open !== m.draft.key) this.#leave(m);
    m.draft ??= { key: this.#key(), shown: true, chosen: null };
    m.draft.shown = true;
    m.open = m.draft.key;
    this.#focusOn(m.open);
    this.#emit();
  }

  /** The plan the lawyer chose for the draft, in the composer. With the list failed the open key is a draft with no
   * draft yet: the choice makes one, unshown, under that key. */
  choose(agent: AgentId): void {
    const m = this.#current;
    if (m === null || m.open === null) return;
    if (m.draft === null || m.draft.key !== m.open) {
      if (this.#threadOf(m, m.open) !== null) return;
      m.draft = { key: m.open, shown: false, chosen: agent };
    } else m.draft.chosen = agent;
    this.#emit();
  }

  /** The thread the first send of the conversation `key` (the session's own) goes to: its thread once the list is in,
   * else a new one, made with the agent its draft runs (`create_thread`), which takes the key. The key is the
   * session's, not whatever is open when the answers come back, so a move made meanwhile can't take the question.
   * Rejects with the host's error. */
  create = async (key: string): Promise<ThreadId> => {
    const m = this.#current;
    if (m === null) throw internalError("no matter is open");
    // a failed list is read again: a thread made now might sit beside the one to reopen
    if (m.phase === "failed") await this.#list(m);
    else if (m.phase === "loading") await m.listing;
    if (m.phase !== "ready") throw toHostError(m.refusal);
    const open = this.#threadOf(m, key);
    if (open !== null) return open;
    const agent = this.#agentOf(m, key);
    const made = await this.#transport.dispatch({
      command_id: crypto.randomUUID(),
      command: { create_thread: { matter: m.matter, agent } },
    });
    if (typeof made === "string" || made.thread_created === undefined)
      throw internalError("create_thread answered without a thread");
    const thread = made.thread_created.thread;
    // idle until its question is accepted and its header says otherwise
    m.threads.set(thread, { thread, matter: m.matter, agent, status: "idle", queue_held: false });
    m.made.add(thread);
    m.unsent.add(thread);
    m.keys.set(thread, key);
    if (m.draft !== null && m.draft.key === key) m.draft = null;
    if (m === this.#current) this.#emit();
    return thread;
  };

  /** A prompt was accepted in `thread`: a thread with no title yet, renamed by nobody here, takes its title from it.
   * A message that gives no title the host would take sends nothing. */
  sent = (thread: ThreadId, text: string): void => {
    const m = this.#stateOf(thread);
    const known = m?.threads.get(thread);
    if (m === undefined || known === undefined) return;
    m.unsent.delete(thread);
    const header = this.#store.view(streamOf(thread))?.header?.header;
    if ((header?.title ?? known.title) !== undefined || m.renamed.has(thread)) return;
    m.renamed.add(thread);
    // the title's rules load with the first question, off the first paint
    void late(import("./conversation-title.ts")).then(({ titleOf }) => {
      const title = titleOf(text);
      if (title !== null && this.#live) this.#rename(m, thread, title);
    });
  };

  #rename(m: MatterState, thread: ThreadId, title: string): void {
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { rename_thread: { thread, title } } })
      .then(
        () => {
          const now = m.threads.get(thread);
          if (now === undefined || now.title !== undefined) return;
          m.threads.set(thread, { ...now, title });
          if (m === this.#current) this.#emit();
        },
        (error: unknown) =>
          console.warn(`rename_thread failed: ${JSON.stringify(toHostError(error).code)}`),
      );
  }

  /** H13, Retry from here: a new thread from the question `message` of `thread`, asked again; it opens once the
   * host has made it, and nothing is deleted. `at` is the question's time, for the line the new thread opens with.
   * Rejects with the host's error. */
  retry = async (thread: ThreadId, message: MessageId, at: Timestamp): Promise<void> => {
    const host = this.#next.retry;
    const m = this.#stateOf(thread);
    if (host === null || m === undefined) throw internalError("this host can't retry a question");
    // where the lawyer is now: the new thread opens only if they haven't moved on while it was made
    const was = m.open;
    const made = await host.retryFrom(thread, message);
    m.made.add(made);
    const from = m.threads.get(thread);
    m.threads.set(made, {
      thread: made,
      matter: m.matter,
      agent: from?.agent ?? this.#agentOf(m),
      status: "starting",
      queue_held: false,
    });
    m.retried.set(made, { from: thread, at });
    if (!this.#live || m !== this.#current) return;
    if (m.open !== was) {
      this.#emit();
      return;
    }
    this.#leave(m);
    m.open = made;
    this.#opened(m);
    this.#focusOn(made);
    this.#emit();
  };

  /* ---------- following the matter ---------- */

  #follow(): void {
    if (!this.#live) return;
    const matter = this.#matters.view().open?.matter ?? null;
    if (matter === (this.#current?.matter ?? null)) return;
    if (this.#current !== null) this.#leave(this.#current);
    this.#focus = null;
    if (matter === null) {
      this.#current = null;
      this.#emit();
      return;
    }
    let m = this.#by.get(matter);
    if (m === undefined) {
      m = {
        matter,
        threads: new Map(),
        keys: new Map(),
        draft: null,
        open: null,
        pending: null,
        phase: "loading",
        refused: null,
        refusal: null,
        listing: null,
        ticket: 0,
        renamed: new Set(),
        retried: new Map(),
        made: new Set(),
        unsent: new Set(),
      };
      this.#by.set(matter, m);
    }
    const revisit = m.open !== null && m.phase === "ready";
    if (m.open === null) {
      m.open = this.#key();
      m.pending = m.open;
    }
    this.#current = m;
    // back in a matter: the conversation open there opens again, and H6 hears it seen
    if (revisit) this.#opened(m);
    void this.#list(m);
    this.#emit();
  }

  /** Reads the matter's list; an answer older than a later read is dropped. */
  #list(m: MatterState): Promise<void> {
    const ticket = (m.ticket += 1);
    const rows = this.#next.matters;
    const read = (async () => {
      try {
        const threads: Known[] =
          rows !== null
            ? await rows.listThreads(m.matter)
            : ((await this.#transport.query({ list_threads: { matter: m.matter } })).threads ?? []);
        if (!this.#live || ticket !== m.ticket) return;
        const listed = new Map(threads.map((t) => [t.thread, this.#withHeader(t)]));
        // a thread this room made that the answer predates stays
        for (const made of m.made) {
          const known = m.threads.get(made);
          if (!listed.has(made) && known !== undefined) listed.set(made, known);
        }
        m.threads = listed;
        m.phase = "ready";
        m.refused = null;
        this.#settle(m);
      } catch (error) {
        if (!this.#live || ticket !== m.ticket) return;
        console.warn(`list_threads failed: ${JSON.stringify(error)}`);
        if (m.phase === "loading") m.phase = "failed";
        m.refused = toHostError(error).code;
        m.refusal = error;
      }
      if (m === this.#current) this.#emit();
    })();
    m.listing = read;
    return read;
  }

  /** With the list in: an open thread still listed stays open, and so does a draft. A key waiting for the list takes
   * the thread to reopen (the newest not archived), keeping the key, or becomes the draft; an open thread archived or
   * gone gives way to the thread to reopen, or the draft, under its own key. */
  #settle(m: MatterState): void {
    const key = m.open;
    if (key === null || m.draft?.key === key) return;
    const thread = this.#threadOf(m, key);
    if (thread !== null && m.threads.get(thread)?.status !== "archived") return;
    const pending = m.pending === key;
    m.pending = null;
    const pick = pickReopenThread([...m.threads.values()]);
    if (pick !== null) {
      if (pending && !m.keys.has(pick.thread)) m.keys.set(pick.thread, key);
      else m.open = m.keys.get(pick.thread) ?? pick.thread;
      this.#opened(m);
      return;
    }
    m.draft ??= { key: pending ? key : this.#key(), shown: false, chosen: null };
    m.open = m.draft.key;
  }

  /** The listed thread a key names, or null (a draft, a key waiting for the list, a thread gone). */
  #threadOf(m: MatterState, key: string): ThreadId | null {
    for (const [thread, k] of m.keys) if (k === key && m.threads.has(thread)) return thread;
    return m.threads.has(key) ? key : null;
  }

  /** The conversation open is left: a draft with nothing typed goes, and so does a thread this room made whose first
   * question the host refused (archived: nothing is deleted, and no empty thread stays listed or reopens). */
  #leave(m: MatterState): void {
    const key = m.open;
    if (key === null) return;
    const typed = (this.#drafts.get(key) ?? "").trim() !== "";
    if (typed) return;
    // words of whitespace only go with their conversation
    this.#drafts.delete(key);
    const d = m.draft;
    if (d !== null && d.key === key) {
      m.draft = null;
      return;
    }
    const thread = this.#threadOf(m, key);
    if (thread === null || !m.unsent.has(thread)) return;
    m.unsent.delete(thread);
    const known = m.threads.get(thread);
    if (known !== undefined) m.threads.set(thread, { ...known, status: "archived" });
    this.#transport
      .dispatch({ command_id: crypto.randomUUID(), command: { archive_thread: { thread } } })
      .catch((error: unknown) =>
        console.warn(`archive_thread failed: ${JSON.stringify(toHostError(error).code)}`),
      );
  }

  /** A conversation opened: H6 marks its thread seen, if the window has focus. */
  #opened(m: MatterState): void {
    const thread = m.open === null ? null : this.#threadOf(m, m.open);
    if (thread !== null) this.#markSeen(m, thread);
  }

  #markSeen(m: MatterState, thread: ThreadId): void {
    const rows = this.#next.matters;
    if (rows === null || !this.#focused()) return;
    const known = m.threads.get(thread);
    if (known !== undefined && known.seen !== undefined)
      m.threads.set(thread, { ...known, seen: true });
    rows.command({ mark_seen: { thread } }).catch((error: unknown) => {
      console.warn(`mark_seen failed: ${JSON.stringify(error)}`);
    });
  }

  /* ---------- live updates ---------- */

  /** A thread with its header's title and status, where the store holds its view. */
  #withHeader(t: Known): Known {
    const header = this.#store.view(streamOf(t.thread))?.header?.header;
    if (header === undefined) return t;
    return {
      ...t,
      status: header.status,
      agent: header.agent,
      ...(header.title === undefined ? {} : { title: header.title }),
    };
  }

  /** After each flush: the current matter's headers into its rows. A held view gone while its thread worked reads the
   * list again; the open thread's turn ending while the window has focus marks it seen (H6). */
  #headers(): void {
    const m = this.#current;
    if (!this.#live || m === null) return;
    let changed = false;
    let lost = false;
    const openThread = m.open === null ? null : this.#threadOf(m, m.open);
    for (const [thread, known] of m.threads) {
      const header = this.#store.view(streamOf(thread))?.header ?? undefined;
      const was = this.#seenHeaders.get(thread);
      if (header === was) continue;
      if (header === undefined) {
        this.#seenHeaders.delete(thread);
        if (!resting(known.status) || typeof known.status === "object") lost = true;
        continue;
      }
      this.#seenHeaders.set(thread, header);
      const next = this.#withHeader(known);
      if (next.status === known.status && next.title === known.title && next.agent === known.agent)
        continue;
      m.threads.set(thread, next);
      changed = true;
      if (thread === openThread && !resting(known.status) && resting(next.status))
        this.#markSeen(m, thread);
    }
    if (lost) void this.#list(m);
    if (changed) this.#emit();
  }

  /** H4's threads_changed: a matter's whole list, or what changed in it. Matters not visited are read when opened. */
  #hear(event: AppEvent): void {
    if (!this.#live || !("threads_changed" in event)) return;
    const change: ThreadsChanged = event.threads_changed;
    const m = this.#by.get(change.matter);
    if (m === undefined) return;
    const threads = change.all ? new Map<ThreadId, Known>() : new Map(m.threads);
    for (const t of change.threads) threads.set(t.thread, this.#withHeader(t));
    // a whole list that predates a thread this room made keeps it, as a read does
    if (change.all)
      for (const made of m.made) {
        const known = m.threads.get(made);
        if (!threads.has(made) && known !== undefined) threads.set(made, known);
      }
    for (const gone of change.removed) {
      threads.delete(gone);
      m.made.delete(gone);
      m.unsent.delete(gone);
    }
    m.threads = threads;
    if (change.all) {
      // a whole list is as good as a read: one in flight is older
      m.ticket += 1;
      m.phase = "ready";
      m.refused = null;
    }
    if (m.phase === "ready") this.#settle(m);
    if (m === this.#current) this.#emit();
  }

  /* ---------- the view ---------- */

  #key(): string {
    this.#n += 1;
    return `conversation-${this.#n}`;
  }

  #stateOf(thread: ThreadId): MatterState | undefined {
    for (const m of this.#by.values()) if (m.threads.has(thread)) return m;
    return undefined;
  }

  /** The agent a new thread for `key` runs: its draft's choice, else agent.ts's order. */
  #agentOf(m: MatterState, key: string | null = m.open): AgentId {
    const chosen = m.draft !== null && m.draft.key === key ? m.draft.chosen : null;
    return agentFor({ chosen, setting: null, threads: [...m.threads.values()] });
  }

  #make(): ConversationsView {
    const m = this.#current;
    const agents = this.#agents;
    const options = planOptions(agents);
    if (m === null)
      return {
        matter: null,
        phase: "loading",
        refused: null,
        rows: [],
        open: null,
        thread: null,
        drafting: false,
        plan: undefined,
        agents,
        options,
        agent: agentFor({ chosen: null, setting: null, threads: [] }),
        focus: null,
        canRetry: this.#next.retry !== null,
        retried: null,
      };
    const listed = [...m.threads.values()].filter((t) => t.status !== "archived").sort(newestFirst);
    const rows: ConversationRow[] = listed.map((t) => ({
      key: m.keys.get(t.thread) ?? t.thread,
      thread: t.thread,
      ...(t.title === undefined ? {} : { title: t.title }),
      agent: t.agent,
      status: t.status,
      ...(t.seen === undefined ? {} : { seen: t.seen }),
      ...(t.last_stop === undefined ? {} : { last_stop: t.last_stop }),
    }));
    const d = m.draft;
    if (d !== null && d.shown)
      rows.unshift({ key: d.key, thread: null, agent: null, status: "idle" });
    const thread = m.open === null ? null : this.#threadOf(m, m.open);
    const origin = thread === null ? undefined : m.retried.get(thread);
    const source = origin === undefined ? undefined : m.threads.get(origin.from);
    return {
      matter: m.matter,
      phase: m.phase,
      refused: m.refused,
      rows,
      open: m.open,
      thread,
      drafting: m.phase !== "loading" && thread === null,
      plan: matterPlan(listed, agents),
      agents,
      options,
      agent: this.#agentOf(m),
      focus: this.#focus !== null && this.#focus.key === m.open ? this.#focus : null,
      canRetry: this.#next.retry !== null,
      retried:
        origin === undefined
          ? null
          : {
              from: m.keys.get(origin.from) ?? origin.from,
              ...(source?.title === undefined ? {} : { title: source.title }),
              at: origin.at,
            },
    };
  }

  #emit(): void {
    this.#view = this.#make();
    for (const listener of this.#listeners) listener();
  }
}
