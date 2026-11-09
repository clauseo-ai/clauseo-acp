// UC's copy review entries (the protection screens): the room's line and marks, the Not protected
// note for each cause, the Add room and remove confirms, the firm's notice, the first run's Safe
// workspace step and design B's blocks and repair, in the real room over a FakeTransport that
// carries the protection wire; and Settings' Safe workspace, in the real settings window. A note or
// a confirm shows after the presses that open it (PressInOrder). Notes are marked explained first,
// so none opens by itself over a line's cell. The shipped and the design's own lines are approved
// "by": "design"; the new ones are provisional, English only (hi: later). R18's screens are UC2's.
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
import { SafeWorkspaceLine } from "@clauseo/components/clauseo/safe-workspace-line.tsx";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { PressInOrder } from "./press-words.tsx";
import { UC_BLACKLINE } from "./registry-uc-blackline.tsx";
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
export const P = {
  b: { protected: { tier: "b" } } as Protection,
  a: { protected: { tier: "a" } } as Protection,
  needs: np("needs_set_up"),
  refused: np("set_up_refused"),
  failed: np("set_up_failed"),
  cant: np("unsupported"),
  pool: np({ pool_full: { size: 8, open: 5, held: 0, owed: 2, set_aside: 1 } }),
  off: np({ switched_off: { at: Date.UTC(2026, 10, 6, 10, 0) } }),
  // design B (R17 PR-E's causes, not in bindings.ts yet)
  missing: np({ blocked: { reason: "admin_copy_missing" } }),
  logon: np({ blocked: { reason: "logon_right" } }),
  policy: np({ blocked: { reason: "policy" } }),
  tool: np({ blocked: { reason: "tool_missing" } }),
  changed: np({ blocked: { reason: "admin_copy_changed" } }),
  unsafe: np({ blocked: { reason: "admin_copy_unsafe" } }),
  newer: np({ blocked: { reason: "a_reason_from_a_newer_clauseo" } }),
  other: np("other_account"),
};
/** A view whose set-up brings no Windows prompt, so the app offers it. */
const NO_PROMPT = { retry_asks_admin: false } as Partial<ProtectionView>;

const viewOf = (computer: Protection, over: Partial<ProtectionView> = {}): ProtectionView => ({
  tier: "protected" in computer ? (computer.protected?.tier ?? "c") : "c",
  computer,
  required: false,
  unprotected_auto_allowed: true,
  ...over,
});

/** A protection host whose commands never answer, so a wait stays in view. */
export const held = (
  computer: Protection,
  over: Partial<ProtectionView> = {},
): FakeProtectionHost => ({
  view: () => viewOf(computer, over),
  command: () => new Promise<void>(() => {}),
});

/** Every note explained, so none opens by itself over a cell (the frames share one localStorage). */
const quiet = () =>
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER.matter] }),
  );

export type RoomScene = {
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

export const LINE = '[data-testid="workspace-line"]';
const SETUP = '[data-testid="protection-setup"]';

export const room = (id: string, words: WordsRef, screen: string, s: RoomScene): Entry => ({
  id,
  words,
  screen,
  render: () => pressing(s.press, <RoomOver host={roomHost(s)} />),
});

/** The first run, signed in here, with the computer needing its set-up (by default from an older
 * host, whose set-up would bring Windows' prompt: the repair), then `press`. */
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
const firstHost = (over: Partial<ProtectionView>, computer: Protection) => () =>
  new FakeTransport({
    platform: "windows",
    openSettings: () => {},
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && "list_threads" in query) return { threads: [] };
      throw new Error(`not scripted: ${JSON.stringify(query)}`);
    },
    setup: { first: () => SIGN },
    protection: held(computer, over),
  });
const first = (
  id: string,
  words: WordsRef,
  press?: string[],
  over: Partial<ProtectionView> = {},
  computer: Protection = P.needs,
): Entry => ({
  id,
  words,
  screen: "room/workspace-step",
  render: () => pressing(press, <RoomOver host={firstHost(over, computer)} />),
});

/** The settings window at the Safe workspace. */
export const settings = (
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
export const note = (protection: Protection): RoomScene => ({ protection, press: [LINE] });
const REFUSED_INVALID: FakeProtectionHost = {
  view: () => {
    throw { code: "command.invalid", message: "not admitted yet", retryable: false };
  },
};

/** UC's entries, UC-01 to UC-116 (UC-113 to UC-116 in registry-uc-blackline.tsx), each id written as it is; ids
 * missing are unused. */
export const UC: Entry[] = [
  room(
    "UC-01",
    roomWords(
      "Safe workspace. Clauseo works only in this matter's folder. It can't open your other files unless you allow it.",
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
      "Clauseo can read any file you can open, and changes files in this matter without asking. It asks before changing files outside it.",
    ),
    "room/protection-line",
    { protection: P.refused, agent: "codex-acp" },
  ),
  room("UC-06", toolbarWords("Not protected"), "room/protection-mark", { protection: P.refused }),
  first("UC-10", fw("Windows asks once for permission to set up the Safe workspace.")),
  first(
    "UC-11",
    pw(
      "Windows will ask to let Clauseo set up the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
    ),
  ),
  room("UC-12", pw("Set up the Safe workspace"), "room/np-note", {
    protection: P.needs,
    over: NO_PROMPT,
    press: [LINE],
  }),
  first("UC-13", fw("Not now")),
  room("UC-14", pw("Choose Yes in the window Windows shows."), "room/np-note-pool", {
    protection: P.pool,
    press: [LINE, SETUP, '[data-testid="protection-confirm-ok"]'],
  }),
  first("UC-15", pw("Setting up the Safe workspace."), undefined, NO_PROMPT),
  first("UC-16", fw("Settings"), ['[data-testid="workspace-not-now"]']),
  room("UC-20", versionWords("This matter is not protected"), "room/np-note", note(P.needs)),
  room(
    "UC-21",
    versionWords(
      "The Safe workspace is off for {matter}. Claude asks before each change. ChatGPT can read any file you can open, and asks before changing files outside this matter's folder.",
    ),
    "room/np-note",
    note(P.off),
  ),
  room(
    "UC-22",
    pw(
      "The Safe workspace isn't set up on this computer yet. Until it is, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.needs),
  ),
  room(
    "UC-23",
    pw(
      "The Safe workspace couldn't be set up on this computer, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.refused),
  ),
  room(
    "UC-24",
    versionWords("Windows didn't allow the Safe workspace on this computer."),
    "room/np-note",
    note(P.refused),
  ),
  room(
    "UC-25",
    pw(
      "This computer can't run the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
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
      "All {size} places in this computer's Safe workspace are in use, so this matter works without it for now. It goes back in once a place is free.",
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
      "Windows isn't letting the Safe workspace's accounts sign in. Setting up the Safe workspace again usually fixes this.",
    ),
    "room/np-note",
    note(P.logon),
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
      "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo can read any file you can open on this computer, and asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(np({ unsupported: { why: "not_built" } })),
  ),
  settings(
    "UC-36",
    "The Safe workspace isn't available on Windows on ARM yet, so Clauseo asks before changing anything outside a matter's folder.",
    held(np({ unsupported: { why: "windows_arm" } })),
  ),
  settings(
    "UC-37",
    "This version of Clauseo doesn't have the Safe workspace yet. It comes in an update. Until then, Clauseo asks before changing anything outside a matter's folder.",
    held(np({ unsupported: { why: "not_built" } })),
  ),
  room("UC-40", pw("Set up the Safe workspace?"), "room/protection-confirm", {
    protection: P.needs,
    press: [LINE, SETUP],
  }),
  settings("UC-41", pw("Remove the Safe workspace from this computer?"), held(P.b), [
    '[data-testid="protection-remove"]',
  ]),
  settings(
    "UC-42",
    pw(
      "Windows will ask to let Clauseo remove the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
    ),
    held(P.b),
    ['[data-testid="protection-remove"]'],
  ),
  settings(
    "UC-43",
    pw(
      "Afterwards, Clauseo works on this computer without the Safe workspace until it's set up again.",
    ),
    held(P.b),
    ['[data-testid="protection-remove"]'],
  ),
  settings(
    "UC-45",
    pw(
      "Your firm requires the Safe workspace, so afterwards Clauseo won't answer on this computer until it's set up again.",
    ),
    held(P.b, { required: true }),
    ['[data-testid="protection-remove"]'],
  ),
  settings("UC-44", pw("Remove set-up"), held(P.b)),
  room(
    "UC-50",
    pw("Your firm requires the Safe workspace on this computer. Set it up once, then ask here."),
    "room/protection-required",
    { protection: P.needs, over: { required: true, ...NO_PROMPT } },
  ),
  room(
    "UC-51",
    pw(
      "Your firm requires the Safe workspace, and all its places on this computer are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
    ),
    "room/protection-required",
    { protection: P.pool, computer: P.b, over: { required: true } },
  ),
  // the note while the firm requires the Safe workspace: the firm's line in the cause's place
  room(
    "UC-52",
    pw(
      "Your firm requires the Safe workspace, and all its places on this computer are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
    ),
    "room/np-note-pool",
    { ...note(P.pool), over: { required: true } },
  ),
  room(
    "UC-53",
    pw("Your firm requires the Safe workspace on this computer. Set it up once, then ask here."),
    "room/np-note",
    { ...note(P.needs), over: { required: true, ...NO_PROMPT } },
  ),
  settings("UC-60", "Safe workspace", held(P.a)),
  settings("UC-61", "Clauseo can't show this computer's Safe workspace here yet.", REFUSED_INVALID),
  settings("UC-62", "On for every matter", held(P.a)),
  settings("UC-63", "macOS keeps Clauseo inside each matter's folder.", held(P.a)),
  settings("UC-64", "Windows keeps Clauseo inside each matter's folder.", held(P.a)),
  settings("UC-65", "Set up with Windows' permission.", held(P.b)),
  settings("UC-66", "Not protected", held(P.needs)),
  settings(
    "UC-67",
    "The Safe workspace isn't set up on this computer yet, so Clauseo asks before changing anything outside a matter's folder.",
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
    "The Safe workspace couldn't be set up on this computer, so Clauseo asks before changing anything outside a matter's folder.",
    held(P.failed),
  ),
  settings(
    "UC-71",
    "This computer can't run the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.",
    held(P.cant),
  ),
  settings(
    "UC-72",
    "The Safe workspace isn't available on Linux yet, so Clauseo asks before changing anything outside a matter's folder.",
    held(np({ unsupported: { why: "os" } })),
  ),
  settings("UC-73", "Set up again", held(P.refused, NO_PROMPT)),
  settings(
    "UC-74",
    "Your firm requires the Safe workspace, so Clauseo doesn't answer on this computer until it's set up.",
    held(P.needs, { required: true }),
  ),
  settings(
    "UC-76",
    "Your firm doesn't allow Clauseo to work without asking on this computer.",
    held(P.refused, { unprotected_auto_allowed: false }),
  ),
  settings(
    "UC-77",
    pw("Checking the Safe workspace."),
    held(P.b, { in_progress: "check" } as Partial<ProtectionView>),
  ),
  room(
    "UC-78",
    pw(
      "Your firm requires the Safe workspace, and this version of Clauseo doesn't have it yet, so Clauseo can't answer here. The Safe workspace comes in an update.",
    ),
    "room/protection-required",
    { protection: np({ unsupported: { why: "not_built" } }), over: { required: true } },
  ),
  // design B: what stops the Safe workspace, and the repair
  room(
    "UC-80",
    pw(
      "Something on this computer stops the Safe workspace, so Clauseo can read any file you can open on it. It asks before changing anything outside this matter's folder.",
    ),
    "room/np-note",
    note(P.policy),
  ),
  settings(
    "UC-81",
    pw(
      "Something on this computer stops the Safe workspace, so Clauseo asks before changing anything outside a matter's folder.",
    ),
    held(P.policy),
  ),
  settings(
    "UC-82",
    pw(
      "A Windows policy on this computer blocks the program that sets up the Safe workspace. Your firm manages this policy on this computer.",
    ),
    held(P.policy),
  ),
  room(
    "UC-83",
    pw(
      "The program that sets up the Safe workspace is missing, or security software blocked it. Your firm manages the security software on this computer.",
    ),
    "room/np-note",
    note(P.tool),
  ),
  first(
    "UC-84",
    pw("The Safe workspace wasn't set up when Clauseo was installed."),
    undefined,
    {},
    P.missing,
  ),
  first(
    "UC-85",
    pw("Run the Clauseo installer again to set up the Safe workspace."),
    undefined,
    {},
    P.missing,
  ),
  first("UC-86", pw("Get the installer"), undefined, {}, P.missing),
  settings(
    "UC-87",
    pw("Your firm manages how Clauseo is installed on this computer."),
    held(P.missing, { required: true }),
  ),
  settings(
    "UC-88",
    pw(
      "The program that sets up the Safe workspace has changed since Clauseo was installed. Install Clauseo again to restore it.",
    ),
    held(P.changed),
  ),
  settings(
    "UC-89",
    pw(
      "The folder that holds the Safe workspace's set-up program isn't limited to administrators, or holds files it shouldn't. Your firm manages this folder on this computer.",
    ),
    held(P.unsafe),
  ),
  room(
    "UC-90",
    pw(
      "The Safe workspace on this computer belongs to another Windows account. Your firm manages this computer's Windows accounts.",
    ),
    "room/np-note",
    note(P.other),
  ),
  settings(
    "UC-91",
    pw(
      "Something on this computer stops the Safe workspace. Your firm manages this computer's settings.",
    ),
    held(P.newer),
  ),
  room(
    "UC-92",
    pw(
      "Your firm requires the Safe workspace on this computer, so Clauseo can't answer here until it's set up.",
    ),
    "room/protection-required",
    { protection: P.missing, over: { required: true } },
  ),
  settings(
    "UC-93",
    pw(
      "Something on this computer stops the Safe workspace, so Clauseo can't answer here. Your firm manages this computer's settings.",
    ),
    {
      view: () => viewOf(P.failed, NO_PROMPT),
      command: () => Promise.reject({ code: "sandbox.blocked", message: "", retryable: false }),
    },
    [SETUP],
  ),
  room("UC-94", pw("Add room for 4 more?"), "room/protection-confirm", {
    protection: P.pool,
    press: [LINE, SETUP],
  }),
  room(
    "UC-95",
    pw(
      "Windows will ask to let Clauseo add room to the Safe workspace. It will say the publisher is unknown. That's expected for Clauseo. Choose Yes. If Windows asks for an administrator's password, whoever looks after this computer can enter it.",
    ),
    "room/protection-confirm",
    { protection: P.pool, press: [LINE, SETUP] },
  ),
  first(
    "UC-96",
    pw(
      "Clauseo will close so the installer can set up the Safe workspace. Run the installer you download, then open Clauseo again.",
    ),
    undefined,
    {},
    P.missing,
  ),
  // UC-01 once the host runs Claude on its own in the Safe workspace (D2d): not shown in the app until then
  {
    id: "UC-106",
    words: roomWords(
      "Safe workspace. Clauseo changes files in this matter without asking. It can't open your other files unless you allow it.",
    ),
    screen: "room/protection-line",
    render: () => (
      <p className="room-line">
        <SafeWorkspaceLine worksOnItsOwn />
      </p>
    ),
  },
  ...UC_BLACKLINE,
];
