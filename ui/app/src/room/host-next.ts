// The host calls the room needs that the host doesn't have yet: H8's list of a matter's papers, H7's display
// settings, and the matters' rows: H5's details and statuses, H3's moved folders, H4's app events, H6's thread marks and
// H20's default location. Their wire shapes as the briefs give them, and one small interface, NextHost, that a transport may
// implement. FakeTransport does, from scripted answers; TauriTransport doesn't and never sends these. The room asks for
// it only through `nextHostOf`, so when those rows land, the swap to the generated bindings touches this file alone.
// Without a NextHost the wall shows its surface only, and the display settings live in localStorage (display-store.ts).
import type { TextSize } from "@clauseo/components/lib/display.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  ErrorCode,
  FileRefId,
  FolderRefId,
  HostError,
  MatterId,
  MatterPath,
  MatterSource,
  MatterSummary,
  SkippedFile,
  StopReason,
  ThreadId,
  ThreadSummary,
  Timestamp,
} from "../transport/protocol.ts";

/* ---------- H8: a matter's papers ---------- */

/** One file in a matter's folder, as `list_papers` lists it. */
export type PaperSummary = {
  /** The host's handle for the file. */
  file: FileRefId;
  /** The file, relative to the matter folder. */
  path: MatterPath;
  /** Its size in bytes. */
  size: number;
  /** When it last changed. */
  modified: Timestamp;
};

/** `list_papers`'s answer: the files, the files left out and why, and whether the list stops short. */
export type Papers = { papers: PaperSummary[]; skipped: SkippedFile[]; truncated: boolean };

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
  /** H20: its folder as the lawyer would write it, home-relative: "~/Documents/Acme". */
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
  | "get_default_location";
/** Their answers, one per read, in the order above. */
export type NextQueryResult =
  | { papers: Papers }
  | { settings: Settings }
  | { matters: MatterInfo[] }
  | { threads: ThreadInfo[] }
  | { default_location: { display: string } };

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

/** The commands: `{"set_ui_settings": {"patch": ...}}`, only the keys that change, and the matters' commands. */
export type NextCommand = { set_ui_settings: { patch: Partial<UiSettings> } } | MattersCommand;
/** What a command did: `"done"`, or create_matter's `{"matter_created": {"matter": ...}}`. */
export type NextOutcome = "done" | { matter_created: { matter: MatterId } };

/** The event: `{"settings_changed": ...}`, the settings after any change, whoever made it. */
export type NextEvent = { settings_changed: Settings };

/** H4: the app's events, to every window: the matters as they now are, a matter's threads as they now are, and matters
 * taken off the list (an addition the root didn't list). */
export type AppEvent =
  | { matters_changed: { matters: MatterInfo[] } }
  | { threads_changed: { matter: MatterId; threads: ThreadInfo[] } }
  | { matters_removed: { matters: MatterId[] } };

/** The calls a transport makes for the room until H3 to H8 and H20 land. Each rejects with a HostError. */
export interface NextHost {
  listPapers(matter: MatterId): Promise<Papers>;
  getSettings(): Promise<Settings>;
  setUiSettings(patch: Partial<UiSettings>): Promise<void>;
  /** Hears each `settings_changed`. Returns the unsubscribe. */
  onSettingsChanged(listener: (settings: Settings) => void): () => void;
  listMatters(): Promise<MatterInfo[]>;
  listThreads(matter: MatterId): Promise<ThreadInfo[]>;
  /** H20's default location, as the lawyer would write it. */
  getDefaultLocation(): Promise<string>;
  /** One of the matters' commands; create_matter gives `matter_created`. */
  matters(command: MattersCommand): Promise<NextOutcome>;
  /** Hears each AppEvent (H4). Returns the unsubscribe. */
  subscribeApp(listener: (event: AppEvent) => void): () => void;
}

/** A transport that may carry the calls above: `nextHost()` gives them, or null where its host has none. */
export interface NextHostSource {
  nextHost(): NextHost | null;
}

const isSource = (transport: object): transport is NextHostSource =>
  typeof (transport as Partial<NextHostSource>).nextHost === "function";

/** The calls the room needs beyond today's host, or null where the transport's host has none (the desktop one, for
 * now). The one place the room asks for them. */
export function nextHostOf(transport: HostTransport): NextHost | null {
  return isSource(transport) ? transport.nextHost() : null;
}
