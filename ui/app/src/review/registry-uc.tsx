// UC's copy review entries (the protection screens): the room's line and marks, the Not protected note for each cause,
// the set-up confirm, the firm's notice and the first run's Safe workspace step, in the real room over a FakeTransport
// that carries the protection wire; and Settings' Safe workspace, in the real settings window. A note or a confirm
// shows after the presses that open it (PressInOrder). Notes are marked explained first, so none opens by itself over
// a line's cell. The shipped and the design's own lines are approved "by": "design"; the new ones are provisional,
// English only (hi: later). R18's screens are UC2's.
import type { FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import type { ProtectionKey } from "@clauseo/components/lib/words-protection.ts";
import type { SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { FakeTransport, type FakeProtectionHost } from "../transport/fake-transport.ts";
import type {
  AgentId,
  MatterSummary,
  Protection,
  ProtectionView,
  SetupEvent,
  SignIn,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { PressInOrder } from "./press-words.tsx";
import { RoomOver } from "./states.tsx";
import { SettingsOver } from "./states-settings.tsx";
import {
  firstLaunchWords,
  protectionWords,
  roomWords,
  settingsWords,
  toolbarWords,
  versionWords,
  type WordsRef,
} from "./words.ts";

const CLAUDE: AgentId = "claude-acp";
const THREAD = "00000000-0000-4000-8000-0000000000c1";
const np = (cause: unknown): Protection => ({ not_protected: { cause } }) as Protection;
const P = {
  b: { protected: { tier: "b" } } as Protection,
  a: { protected: { tier: "a" } } as Protection,
  needs: np("needs_set_up"),
  refused: np("set_up_refused"),
  failed: np("set_up_failed"),
  failed1385: np({ set_up_failed: { why: "logon_right_denied_1385" } }),
  cant: np("unsupported"),
  pool: np({ pool_full: { size: 8, open: 5, held: 0, owed: 2, set_aside: 1 } }),
  off: np({ switched_off: { at: Date.UTC(2026, 10, 6, 10, 0) } }),
};

const viewOf = (computer: Protection, over: Partial<ProtectionView> = {}): ProtectionView => ({
  tier: "protected" in computer ? (computer.protected?.tier ?? "c") : "c",
  computer,
  required: false,
  unprotected_auto_allowed: true,
  ...over,
});

/** A protection host whose commands never answer, so a wait stays in view. */
const held = (computer: Protection, over: Partial<ProtectionView> = {}): FakeProtectionHost => ({
  view: () => viewOf(computer, over),
  command: () => new Promise<void>(() => {}),
});

/** Every note explained, so none opens by itself over a cell (the frames share one localStorage). */
const quiet = () =>
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER.matter] }),
  );

type RoomScene = {
  protection: Protection;
  computer?: Protection;
  over?: Partial<ProtectionView>;
  agent?: AgentId;
  press?: string[];
};

const roomHost = (s: RoomScene) => () => {
  quiet();
  const matter: MatterSummary = { ...MATTER, protection: s.protection };
  return new FakeTransport({
    platform: "windows",
    query: (query) => {
      if (query === "list_matters") return { matters: [matter] };
      if (typeof query === "object" && "list_threads" in query)
        return {
          threads: [
            { thread: THREAD, matter: MATTER.matter, agent: s.agent ?? CLAUDE, status: "idle" },
          ],
        };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    protection: held(s.computer ?? s.protection, s.over),
  });
};

const pressing = (press: string[] | undefined, children: React.ReactNode) =>
  press === undefined ? children : <PressInOrder press={press}>{children}</PressInOrder>;

const LINE = '[data-testid="workspace-line"]';
const SETUP = '[data-testid="protection-setup"]';

const room = (id: string, words: WordsRef, screen: string, s: RoomScene): Entry => ({
  id,
  words,
  screen,
  render: () => pressing(s.press, <RoomOver host={roomHost(s)} />),
});

/** The first run, signed in here, with the computer needing its set-up: the Windows screen, then `press`. */
const SIGN: SetupEvent[] = [
  { setup_info: { keys_available: true } },
  { agent_install_progress: { agent: CLAUDE, state: { ready: { version: "0.1.0" } } } },
  { sign_in_changed: { agent: CLAUDE, state: { signed_out: {} } } },
  {
    sign_in_changed: {
      agent: CLAUDE,
      state: {
        signed_in: { method: "plan", source: "own", at: Date.UTC(2026, 10, 6, 10, 0) },
      } as SignIn,
    },
  },
];
const firstHost = (over: Partial<ProtectionView>) => () =>
  new FakeTransport({
    platform: "windows",
    openSettings: () => {},
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && "list_threads" in query) return { threads: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    setup: { first: () => SIGN },
    protection: held(P.needs, over),
  });
const first = (
  id: string,
  words: WordsRef,
  press?: string[],
  over: Partial<ProtectionView> = {},
): Entry => ({
  id,
  words,
  screen: "room/workspace-step",
  render: () => pressing(press, <RoomOver host={firstHost(over)} />),
});

/** The settings window at the Safe workspace. */
const settings = (
  id: string,
  key: SettingsKey | WordsRef,
  protection: FakeProtectionHost,
  press?: string[],
): Entry => ({
  id,
  words: typeof key === "string" ? settingsWords(key) : key,
  screen: "settings/workspace",
  width: 960,
  height: 720,
  render: (lang) =>
    pressing(press, <SettingsOver section="workspace" lang={lang} protection={protection} />),
});

const pw = (key: ProtectionKey) => protectionWords(key);
const fw = (key: FirstLaunchKey) => firstLaunchWords(key);
const note = (protection: Protection): RoomScene => ({ protection, press: [LINE] });
const REFUSED_INVALID: FakeProtectionHost = {
  view: () => {
    throw { code: "command.invalid", message: "not admitted yet", retryable: false };
  },
};

/** UC's entries, UC-01 to UC-76, each id written as it is; ids missing are unused. */
export const UC: Entry[] = [
  room(
    "UC-01",
    roomWords(
      "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.",
    ),
    "room/protection-line",
    { protection: P.b },
  ),
  room("UC-02", toolbarWords("Not protected."), "room/protection-line", { protection: P.refused }),
  room(
    "UC-03",
    toolbarWords("Clauseo asks before each change in this matter."),
    "room/protection-line",
    {
      protection: P.refused,
    },
  ),
  room(
    "UC-04",
    toolbarWords(
      "Clauseo can change files in this matter without asking. It asks before going beyond it.",
    ),
    "room/protection-line",
    { protection: P.refused, agent: "codex-acp" },
  ),
  room(
    "UC-05",
    toolbarWords("Clauseo can reach beyond this matter's folder without asking."),
    "room/protection-line",
    { protection: P.off },
  ),
  room("UC-06", toolbarWords("Not protected"), "room/protection-mark", { protection: P.refused }),
  first("UC-10", fw("Windows asks once for permission to set up the Safe workspace.")),
  first(
    "UC-11",
    pw(
      "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to set up protection.",
    ),
  ),
  first("UC-12", pw("Set up protection")),
  first("UC-13", fw("Not now")),
  first("UC-14", pw("Choose Yes in the window Windows shows."), [SETUP]),
  first("UC-15", pw("Setting up the Safe workspace."), undefined, {
    retry_asks_admin: false,
  } as Partial<ProtectionView>),
  first("UC-16", fw("Settings"), ['[data-testid="workspace-not-now"]']),
  room("UC-20", versionWords("This matter is not protected"), "room/np-note", note(P.needs)),
  room(
    "UC-21",
    versionWords(
      "Protection is off for {matter}. Clauseo can read and change files anywhere on this computer while it works here, without asking first.",
    ),
    "room/np-note",
    note(P.off),
  ),
  room(
    "UC-22",
    pw(
      "Protection isn't set up on this computer yet. Until it is, Clauseo can read any file on this computer, and asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.needs),
  ),
  room(
    "UC-23",
    pw(
      "Protection couldn't be set up on this computer, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.refused),
  ),
  room(
    "UC-24",
    versionWords("Windows didn't allow the protected space on this computer."),
    "room/np-note",
    note(P.refused),
  ),
  room(
    "UC-25",
    pw(
      "This computer can't give Clauseo's protection, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.cant),
  ),
  room(
    "UC-26",
    versionWords(
      "Every change inside the matter's folder is still kept, and Versions can undo it.",
    ),
    "room/np-note",
    note(P.needs),
  ),
  room(
    "UC-27",
    pw(
      "All {size} of this computer's protected spaces are in use, so this matter works without protection for now. It's protected again once one is free.",
    ),
    "room/np-note-pool",
    note(P.pool),
  ),
  room("UC-28", pw("In use by other open matters: {n}"), "room/np-note-pool", note(P.pool)),
  room(
    "UC-29",
    pw("Kept by matters with nothing open: {n}"),
    "room/np-note-pool",
    note(np({ pool_full: { size: 32, open: 0, held: 30, owed: 0, set_aside: 2 } })),
  ),
  room(
    "UC-30",
    pw("Freed the next time Clauseo starts work: {n}"),
    "room/np-note-pool",
    note(P.pool),
  ),
  room("UC-31", pw("Still holding files set aside: {n}"), "room/np-note-pool", note(P.pool)),
  room(
    "UC-32",
    pw("To free one now, end a conversation in another matter."),
    "room/np-note-pool",
    note(P.pool),
  ),
  room(
    "UC-33",
    pw(
      "Windows policy on this computer doesn't let Clauseo's protection accounts sign in. Ask your IT team to add them to the Allow log on locally policy.",
    ),
    "room/np-note",
    note(P.failed1385),
  ),
  room(
    "UC-34",
    pw("Clauseo can add room for 4 more. Windows asks for permission first."),
    "room/np-note-pool",
    note(P.pool),
  ),
  room("UC-35", pw("Add room"), "room/np-note-pool", note(P.pool)),
  room(
    "UC-38",
    pw(
      "This build of Clauseo comes without protection, so Clauseo can read any file on this computer. It asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(np({ unsupported: { why: "not_built" } })),
  ),
  settings(
    "UC-36",
    "Clauseo's protection isn't available on Windows on ARM yet, so Clauseo asks before changing anything outside a matter's folder.",
    held(np({ unsupported: { why: "windows_arm" } })),
  ),
  settings(
    "UC-37",
    "This build of Clauseo comes without protection, so Clauseo asks before changing anything outside a matter's folder.",
    held(np({ unsupported: { why: "not_built" } })),
  ),
  room("UC-40", pw("Set up protection?"), "room/protection-confirm", {
    protection: P.needs,
    press: [LINE, SETUP],
  }),
  settings("UC-41", pw("Remove protection's set-up?"), held(P.b), [
    '[data-testid="protection-remove"]',
  ]),
  settings(
    "UC-42",
    pw(
      "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to remove protection's set-up.",
    ),
    held(P.b),
    ['[data-testid="protection-remove"]'],
  ),
  settings(
    "UC-43",
    pw("Afterwards, Clauseo works on this computer without protection until you set it up again."),
    held(P.b),
    ['[data-testid="protection-remove"]'],
  ),
  settings("UC-44", pw("Remove set-up"), held(P.b)),
  room(
    "UC-50",
    pw("Your firm requires protection on this computer. Set it up once, then ask here."),
    "room/protection-required",
    { protection: P.needs, over: { required: true } },
  ),
  room(
    "UC-51",
    pw(
      "Your firm requires protection, and all of this computer's protected spaces are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
    ),
    "room/protection-required",
    { protection: P.pool, computer: P.b, over: { required: true } },
  ),
  settings("UC-60", "Safe workspace", held(P.a)),
  settings("UC-61", "Clauseo can't show this computer's protection here yet.", REFUSED_INVALID),
  settings("UC-62", "On for every matter", held(P.a)),
  settings("UC-63", "macOS keeps Clauseo inside each matter's folder.", held(P.a)),
  settings("UC-64", "Windows keeps Clauseo inside each matter's folder.", held(P.a)),
  settings("UC-65", "Set up with Windows' permission.", held(P.b)),
  settings("UC-66", "Not protected", held(P.needs)),
  settings(
    "UC-67",
    "Protection isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.",
    held(P.needs),
  ),
  settings(
    "UC-68",
    "Windows didn't allow the set-up, so Clauseo asks before changing anything outside a matter's folder.",
    held(P.refused),
  ),
  settings(
    "UC-69",
    "Set up on {date}, with Windows' permission.",
    held(P.b, { set_up_at: Date.UTC(2026, 10, 3, 6, 0) } as Partial<ProtectionView>),
  ),
  settings(
    "UC-70",
    "Protection couldn't be set up on this computer, so Clauseo asks before changing anything outside a matter's folder.",
    held(P.failed),
  ),
  settings(
    "UC-71",
    "This computer can't give Clauseo's protection, so Clauseo asks before changing anything outside a matter's folder.",
    held(P.cant),
  ),
  settings(
    "UC-72",
    "Clauseo's protection isn't available on Linux yet, so Clauseo asks before changing anything outside a matter's folder.",
    held(np({ unsupported: { why: "os" } })),
  ),
  settings("UC-73", "Set up again", held(P.refused)),
  settings(
    "UC-74",
    "Your firm requires protection, so Clauseo doesn't answer on this computer until it's protected.",
    held(P.needs, { required: true }),
  ),
  settings(
    "UC-76",
    "Your firm doesn't allow Clauseo to work without asking on this computer.",
    held(P.refused, { unprotected_auto_allowed: false }),
  ),
];
