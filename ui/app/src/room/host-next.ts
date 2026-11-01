// The host calls the room needs that the host doesn't have yet: H8's list of a matter's papers and H7's display
// settings. Their wire shapes as the briefs give them, and one small interface, NextHost, that a transport may
// implement. FakeTransport does, from scripted answers; TauriTransport doesn't and never sends these. The room asks for
// it only through `nextHostOf`, so when H7 and H8 land, the swap to the generated bindings touches this file alone.
// Without a NextHost the wall shows its surface only, and the display settings live in localStorage (display-store.ts).
import type { TextSize } from "@clauseo/components/lib/display.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type {
  FileRefId,
  MatterId,
  MatterPath,
  SkippedFile,
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

/* ---------- the wire shapes ---------- */

/** The reads: `{"list_papers": {"matter": ...}}` and `"get_settings"`. */
export type NextQuery = { list_papers: { matter: MatterId } } | "get_settings";
/** Their answers: `{"papers": ...}` for list_papers, `{"settings": ...}` for get_settings. */
export type NextQueryResult = { papers: Papers } | { settings: Settings };
/** The command: `{"set_ui_settings": {"patch": ...}}`, only the keys that change. */
export type NextCommand = { set_ui_settings: { patch: Partial<UiSettings> } };
/** The event: `{"settings_changed": ...}`, the settings after any change, whoever made it. */
export type NextEvent = { settings_changed: Settings };

/** The calls a transport makes for the room until H7 and H8 land. Each rejects with a HostError. */
export interface NextHost {
  listPapers(matter: MatterId): Promise<Papers>;
  getSettings(): Promise<Settings>;
  setUiSettings(patch: Partial<UiSettings>): Promise<void>;
  /** Hears each `settings_changed`. Returns the unsubscribe. */
  onSettingsChanged(listener: (settings: Settings) => void): () => void;
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
