// The host calls the room needs beyond the generated commands: H8's list of a matter's papers, H7's display
// settings, the matters' rows: H5's details and statuses, H3's moved folders, H4's app events, H6's thread marks and
// H20's default location. Their wire shapes as the briefs give them (names in WIRE), and NextHost: one optional part
// per host row, which a transport may give. FakeTransport does, from scripted answers; TauriTransport gives settings
// and papers, over the desktop host's wire (host-wire.ts). The room asks for them only through `nextHostOf`, so as
// each row lands, the swap to the generated bindings touches this file and host-wire.ts alone. Without papers the wall shows
// its surface only; without settings they live in localStorage (display-store.ts); without matters the rail lists
// today's matters with no details.
import type { TextSize } from "@clauseo/components/lib/display.ts";
import type { AppFeedSource, HostTransport } from "../transport/host-transport.ts";
import type {
  AgentId,
  ErrorCode,
  FolderRefId,
  HostError,
  MatterId,
  MatterSource,
  MatterSummary,
  MessageId,
  QueryResult,
  StopReason,
  ThreadId,
  ThreadSummary,
  Timestamp,
} from "../transport/protocol.ts";
import { wirePapersHost, wireSettingsHost } from "./host-wire.ts";

/* ---------- H8: a matter's papers ---------- */

// H8 has landed: its types are the generated ones (bindings.ts), and the desktop gives the part (host-wire.ts).

/** One file in a matter's folder, as `list_papers` lists it. */
export type { PaperSummary } from "../transport/protocol.ts";

/** `list_papers`'s answer: the files, the files left out and why, the folders the host couldn't read (sorted, counted
 * inside the same 2,000 cap), and whether the list stops short. */
export type Papers = NonNullable<Extract<QueryResult, { papers: unknown }>["papers"]>;

/* ---------- H7: display settings ---------- */

/** The light: the computer's, or set. The settings crate's Theme. */
export type UiTheme = "system" | "light" | "dark";
/** Lite: automatic follows the host's display report (GetDisplayReport); off until there is one. */
export type UiLite = "automatic" | "full" | "lite";
/** Motion: the computer's reduced-motion preference, or set. */
export type UiMotion = "system" | "full" | "reduced";
export type UiLanguage = "en" | "hi";

/** The settings' `ui` table. */
export type UiSettings = {
  theme: UiTheme;
  font_size: TextSize;
  language: UiLanguage;
  lite: UiLite;
  motion: UiMotion;
};

/** `get_settings`'s answer, and what `settings_changed` carries: the settings, and the keys a policy locks. */
export type Settings = { ui: UiSettings; locked: string[] };

/* ---------- H3, H5, H20: a matter's details ---------- */

/** H5: what a matter is doing, over all its threads. */
export type MatterStatus =
  | "idle"
  | "working"
  | "needs_permission"
  | "needs_answer"
  | "ready"
  | { stopped: { at: Timestamp } }
  | { paused_at_limit: { resets_at?: Timestamp } };

/** H5, H3 and H20: what the rail shows of a matter beyond its name. */
export type MatterDetails = {
  last_activity?: Timestamp;
  client?: string;
  /** The next hearing, a calendar date: ISO "YYYY-MM-DD". */
  next_date?: string;
  pinned: boolean;
  last_opened?: Timestamp;
  /** H3: whether its folder is where it was. */
  folder_state: "found" | "missing";
  status: MatterStatus;
  /** H20: its folder as the lawyer would write it: under home, the parts below it ("Documents/Acme"); else in full. */
  folder_display?: string;
};

/** A matter in `list_matters`'s answer and in `matters_changed`. */
export type MatterInfo = MatterSummary & MatterDetails;

/** H6: a thread in `list_threads`'s answer, with whether its last answer was seen and how its last turn ended. */
export type ThreadInfo = ThreadSummary & {
  seen: boolean;
  last_stop?: { reason: StopReason; at: Timestamp };
  last_activity?: Timestamp;
};

/* ---------- R15: the agents ---------- */

/** An agent the settings set up, as R15's `list_agents` lists it (its shape until R15's record lands: the settings key
 * alone). */
export type AgentInfo = { agent: AgentId };

/** H20: where a matter goes, with the default location the host makes `<location>/<name>` in. */
export type NextMatterSource = MatterSource | "default_location";

/** The refusals H5 adds to today's `matter.folder_refused` and `matter.folder_missing`. */
export type NextErrorCode =
  | ErrorCode
  | "matter.folder_is_drive"
  | "matter.folder_is_home"
  | "matter.folder_is_system"
  | "matter.folder_unreadable";

/** A HostError whose code may be one of H5's. */
export type NextHostError = Omit<HostError, "code"> & { code: NextErrorCode };

/* ---------- the wire shapes ---------- */

/** The reads: `{"list_papers": {"matter": ...}}`, `"get_settings"`, `"list_matters"`, `{"list_threads": {"matter":
 * ...}}` and `"get_default_location"` (an addition the root didn't list: the form's folder line before a pick). */
export type NextQuery =
  | { list_papers: { matter: MatterId } }
  | "get_settings"
  | "list_matters"
  | { list_threads: { matter: MatterId } }
  | "get_default_location"
  | "list_agents";
/** Their answers, one per read, in the order above. */
export type NextQueryResult =
  | { papers: Papers }
  | { settings: Settings }
  | { matters: MatterInfo[] }
  | { threads: ThreadInfo[] }
  | { default_location: { display: string } }
  | { agents: AgentInfo[] };

/** The commands of H5, H3, H6 and H20 for matters. */
export type MattersCommand =
  | {
      create_matter: {
        name: string;
        source: NextMatterSource;
        client?: string;
        next_date?: string;
      };
    }
  | { update_matter: { matter: MatterId; client?: string; next_date?: string; pinned?: boolean } }
  | { open_matter: { matter: MatterId } }
  | { forget_matter: { matter: MatterId } }
  | { relocate_matter: { matter: MatterId; folder: FolderRefId } }
  | { mark_seen: { thread: ThreadId } };

/** H13: a new thread from one of the lawyer's questions in `thread`, asked again there; nothing is deleted. */
export type RetryCommand = { retry_from: { thread: ThreadId; message: MessageId } };

/** The commands: `{"set_ui_settings": {"patch": ...}}`, only the keys that change, the matters' commands and H13's
 * retry. */
export type NextCommand =
  | { set_ui_settings: { patch: Partial<UiSettings> } }
  | MattersCommand
  | RetryCommand;
/** What a command did: `"done"`, create_matter's `{"matter_created": {"matter": ...}}`, or retry_from's
 * `{"thread_created": {"thread": ...}}`, as create_thread answers. */
export type NextOutcome =
  | "done"
  | { matter_created: { matter: MatterId } }
  | { thread_created: { thread: ThreadId } };

/** The event: `{"settings_changed": ...}`, the settings after any change, whoever made it. */
export type NextEvent = { settings_changed: Settings };

/** H4 and H6: a matter's threads that changed, those taken off its list, and whether `threads` is its whole list (the
 * first frames after the app subscription are each matter's whole list; later ones carry only what changed). */
export type ThreadsChanged = {
  matter: MatterId;
  threads: ThreadInfo[];
  removed: ThreadId[];
  all: boolean;
};

/** H4: the app's events, to every window: the matters as they now are, a matter's threads that changed, and matters
 * taken off the list (an addition the root didn't list). */
export type AppEvent =
  | { matters_changed: { matters: MatterInfo[] } }
  | { threads_changed: ThreadsChanged }
  | { matters_removed: { matters: MatterId[] } };

/** The wire names, one home each: the reads, the commands and the events as the host will spell them. */
export const WIRE = {
  listPapers: "list_papers",
  getSettings: "get_settings",
  setUiSettings: "set_ui_settings",
  settingsChanged: "settings_changed",
  listMatters: "list_matters",
  listThreads: "list_threads",
  getDefaultLocation: "get_default_location",
  listAgents: "list_agents",
  createMatter: "create_matter",
  updateMatter: "update_matter",
  openMatter: "open_matter",
  forgetMatter: "forget_matter",
  relocateMatter: "relocate_matter",
  markSeen: "mark_seen",
  retryFrom: "retry_from",
  matterCreated: "matter_created",
  mattersChanged: "matters_changed",
  threadsChanged: "threads_changed",
  mattersRemoved: "matters_removed",
} as const;

/** H8: a matter's papers. */
export interface PapersHost {
  listPapers(matter: MatterId): Promise<Papers>;
}

/** H7: the display settings. */
export interface SettingsHost {
  getSettings(): Promise<Settings>;
  setUiSettings(patch: Partial<UiSettings>): Promise<void>;
  /** Hears each `settings_changed`. Returns the unsubscribe. */
  onSettingsChanged(listener: (settings: Settings) => void): () => void;
}

/** H5, H3, H6 and H20's matter half: the matters' rows and their commands. */
export interface MattersHost {
  listMatters(): Promise<MatterInfo[]>;
  listThreads(matter: MatterId): Promise<ThreadInfo[]>;
  /** One of the matters' commands; create_matter gives `matter_created`. */
  command(command: MattersCommand): Promise<NextOutcome>;
}

/** H20: the default location, as the lawyer would write it. */
export interface LocationHost {
  getDefaultLocation(): Promise<string>;
}

/** H10: restart_agent, which the host refuses today. Built, it relaunches the thread's agent and asks the last
 * question again; refused with thread.turn_running during a turn. The command is on the wire already, so the part
 * says only that the host has built it. */
export interface RestartHost {
  /** Rejects with the HostError when the host refuses. */
  restartAgent(thread: ThreadId): Promise<void>;
}

/** R15: the agents the settings set up, for the composer's plan choice. */
export interface AgentsHost {
  listAgents(): Promise<AgentInfo[]>;
}

/** H13: Retry from here. */
export interface RetryHost {
  /** Makes the new thread from the question `message` of `thread`, and gives its id. */
  retryFrom(thread: ThreadId, message: MessageId): Promise<ThreadId>;
}

/** H4: the app's events. */
export interface AppHost {
  /** Hears each AppEvent. Returns the unsubscribe. */
  subscribeApp(listener: (event: AppEvent) => void): () => void;
}

/** The calls a transport makes for the room until H3 to H8 and H20 land, one part per host row, each null where the
 * host lacks it: H8 may land before H5, so the room works with papers and no matters. Each call rejects with a
 * HostError. */
export type NextHost = {
  papers: PapersHost | null;
  settings: SettingsHost | null;
  matters: MattersHost | null;
  location: LocationHost | null;
  app: AppHost | null;
  restart: RestartHost | null;
  agents: AgentsHost | null;
  retry: RetryHost | null;
};

/** One part of NextHost: a host row. */
export type NextPart = keyof NextHost;

/** No part at all: today's desktop host. */
export const NO_NEXT: NextHost = {
  papers: null,
  settings: null,
  matters: null,
  location: null,
  app: null,
  restart: null,
  agents: null,
  retry: null,
};

/** A transport that may carry the calls above: `nextHost()` gives the parts its host has. */
export interface NextHostSource {
  nextHost(): NextHost;
}

const isSource = (transport: object): transport is NextHostSource =>
  typeof (transport as Partial<NextHostSource>).nextHost === "function";

const hasAppFeed = (transport: object): transport is AppFeedSource =>
  typeof (transport as Partial<AppFeedSource>).onAppEvent === "function";

/** The calls the room needs beyond today's host, each part null where the transport's host lacks it. A NextHostSource
 * (FakeTransport) gives its scripted parts; a transport with the app feed (TauriTransport) gives settings and papers
 * over the wire and nothing else, since the desktop host has no H4, H5, H6 or H20 yet. The one place the room asks for
 * them. */
export function nextHostOf(transport: HostTransport): NextHost {
  if (isSource(transport)) return transport.nextHost();
  if (hasAppFeed(transport))
    return { ...NO_NEXT, settings: wireSettingsHost(transport), papers: wirePapersHost(transport) };
  return NO_NEXT;
}
