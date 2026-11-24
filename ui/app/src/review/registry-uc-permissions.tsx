// Issue 5's entries for the first screen's permissions step (UC-1150 to UC-1166, UC-1164 unused): what macOS asks Clauseo for (the
// Documents folder, the keychain), asked at the head of the first screen before the first matter, each with its reason;
// waiting while macOS asks, allowed, refused with where to change it; and Windows' Controlled folder access blocking the
// matters folder, or unreadable; and the open matter's line where Controlled folder access protects its folder. The real room over FakeTransport, on the first screen, with a fake permissions host.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { PermissionRow, PermissionsHost } from "../room/first-run/permissions-host.ts";
import type { SetupEvent } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { render } from "./registry-ud1-scene.tsx";
import { firstLaunchWords as w, matterWords as m, type WordsRef } from "./words.ts";

/** Several lines of one id, joined with " / " as the approved page writes them. */
const joined = (parts: readonly WordsRef[]): WordsRef => ({
  file: [...new Set(parts.map((p) => p.file))].join(", "),
  key: parts.map((p) => p.key).join(" / "),
  text: (lang: Lang) => parts.map((p) => p.text(lang)).join(" / "),
  later: parts.some((p) => p.later),
});

const NONE = { folder: null, settings_path: null, note: null };
export const DOCS: PermissionRow = {
  ...NONE,
  id: "files_documents",
  state: "not_asked",
  folder: "Documents",
};
export const KEYS: PermissionRow = { ...NONE, id: "keychain", state: "would_ask" };
const MAC_PATH = "System Settings › Privacy & Security › Files and Folders › Clauseo";
const WIN_PATH =
  "Windows Security › Virus & threat protection › Ransomware protection › Allow an app through Controlled folder access";
const WIN_NOTE =
  "Claude's and ChatGPT's own programs need allowing too, or choose a matters folder outside Documents.";

/** A fake host listing `rows`; each ask answers `after`'s row, or waits for good when it has none. */
export const fakePermissions = (
  rows: PermissionRow[],
  after: Partial<Record<PermissionRow["id"], PermissionRow>> = {},
): PermissionsHost => ({
  list: () => Promise.resolve(rows),
  ask: (id) => {
    const row = after[id];
    return row === undefined ? new Promise(() => {}) : Promise.resolve(row);
  },
});

/** Claude installed and signed out, ChatGPT offered: the first screen as UD1-12 shows it, so the step is the one
 * difference from that cell. */
const EVENTS: SetupEvent[] = [
  { agent_install_progress: { agent: "claude-acp", state: "installing" } },
  { agent_install_progress: { agent: "claude-acp", state: { ready: { version: "0.1.0" } } } },
  { sign_in_changed: { agent: "claude-acp", state: { signed_out: {} } } },
];

/** The first screen with the step over `host`, its controls pressed in order. */
export const permissionsScene = (
  host: PermissionsHost,
  over: { platform?: "mac" | "windows"; clicks?: string[] } = {},
) =>
  render({
    first: true,
    events: EVENTS,
    permissions: host,
    platform: over.platform ?? "mac",
    ...(over.clicks === undefined ? {} : { clicks: over.clicks }),
  });

const BOTH = permissionsScene(fakePermissions([DOCS, KEYS]));
const WAITING = permissionsScene(fakePermissions([DOCS, KEYS]), {
  clicks: ['[data-testid="permission-allow"]'],
});
const ONE_GRANTED = permissionsScene(fakePermissions([{ ...DOCS, state: "granted" }, KEYS]));
const KEY_WAITING = permissionsScene(fakePermissions([{ ...DOCS, state: "granted" }, KEYS]), {
  clicks: ['[data-testid="permission-allow"]'],
});
export const DENIED = permissionsScene(
  fakePermissions([{ ...DOCS, state: "denied", settings_path: MAC_PATH }, KEYS]),
);
const KEY_DENIED = permissionsScene(
  fakePermissions([
    { ...DOCS, state: "granted" },
    { ...KEYS, state: "denied", note: "Clauseo couldn't read: Claude sign-in, Westlaw connector." },
  ]),
);
const FAILED = permissionsScene(
  { list: () => Promise.resolve([DOCS, KEYS]), ask: () => Promise.reject(new Error("no")) },
  { clicks: ['[data-testid="permission-allow"]'] },
);
const winRow = (state: "blocking" | "unknown"): PermissionRow => ({
  ...NONE,
  id: "controlled_folder_access",
  state,
  folder: "Documents\\Clauseo",
});
const windows = (state: "blocking" | "unknown") =>
  permissionsScene(
    fakePermissions([winRow(state)], {
      controlled_folder_access: { ...winRow(state), settings_path: WIN_PATH, note: WIN_NOTE },
    }),
    { platform: "windows" },
  );
export const WIN_BLOCKING = windows("blocking");

const SCREEN = "first-launch/welcome";

/** Matter A open on Windows, its folder in Documents, Controlled folder access listed; Claude signed in. */
const blockedMatter = (over: Partial<PermissionsHost> = {}, clicks?: string[]) =>
  render({
    events: [
      { agent_install_progress: { agent: "claude-acp", state: { ready: { version: "0.1.0" } } } },
      {
        sign_in_changed: {
          agent: "claude-acp",
          state: {
            signed_in: {
              email: "asha@rathi-law.in",
              plan: "Claude Max",
              method: "plan",
              source: "own",
              at: Date.UTC(2026, 10, 24, 9, 0),
            },
          },
        },
      },
    ],
    platform: "windows",
    folder: "Documents/Clauseo/Acme Traders supply agreement",
    permissions: { ...fakePermissions([winRow("blocking")]), ...over },
    ...(clicks === undefined ? {} : { clicks }),
  });
export const MATTER_BLOCKED = blockedMatter();
export const MATTER_CHOSEN = blockedMatter(
  { chooseMattersFolder: () => Promise.resolve({ done: "D:\\Matters" }) },
  ['[data-testid="folder-choice"]'],
);
const MATTER_SCREEN = "room/thread";

export const UC_PERMISSIONS: Entry[] = [
  { id: "UC-1150", words: w("Before your first matter"), screen: SCREEN, render: BOTH },
  {
    id: "UC-1151",
    words: w("Answer here first, so macOS doesn't stop you in the middle of a matter."),
    screen: SCREEN,
    render: BOTH,
  },
  {
    id: "UC-1152",
    words: joined([
      w("Your Documents folder"),
      w(
        "Your matters' papers live here. Clauseo, Claude and ChatGPT read them and save their work beside them.",
      ),
    ]),
    screen: SCREEN,
    render: BOTH,
  },
  {
    id: "UC-1153",
    words: joined([
      w("Your keychain"),
      w(
        "Clauseo keeps your sign-ins and connector passwords in your Mac's keychain. macOS asks once for each and wants your Mac's login password. Choose Always Allow. Plain Allow means macOS asks again next time.",
      ),
    ]),
    screen: SCREEN,
    render: BOTH,
  },
  { id: "UC-1154", words: w("Allow"), screen: SCREEN, render: BOTH },
  {
    id: "UC-1155",
    words: w("macOS is asking. Choose Allow in its window."),
    screen: SCREEN,
    render: WAITING,
  },
  {
    id: "UC-1156",
    words: w("macOS is asking. Type your Mac's login password, then choose Always Allow."),
    screen: SCREEN,
    render: KEY_WAITING,
  },
  { id: "UC-1157", words: w("Allowed"), screen: SCREEN, render: ONE_GRANTED },
  {
    id: "UC-1158",
    words: joined([w("Not allowed."), w("You can allow it in {path}.")]),
    screen: SCREEN,
    render: DENIED,
  },
  { id: "UC-1159", words: w("Open System Settings"), screen: SCREEN, render: DENIED },
  {
    id: "UC-1160",
    words: w("macOS asks again the next time Clauseo opens. Choose Always Allow then."),
    screen: SCREEN,
    render: KEY_DENIED,
  },
  {
    id: "UC-1161",
    words: w("Clauseo couldn't ask macOS. Try again."),
    screen: SCREEN,
    render: FAILED,
  },
  {
    id: "UC-1162",
    words: joined([
      w("Windows is blocking your matters folder"),
      w(
        "Windows blocks the assistants' tools, such as PowerShell, from saving in protected folders. Choose a matters folder outside Documents, Desktop and Pictures, or ask your IT team.",
      ),
    ]),
    screen: SCREEN,
    render: WIN_BLOCKING,
  },
  {
    id: "UC-1163",
    words: w("Windows may be blocking your matters folder"),
    screen: SCREEN,
    render: windows("unknown"),
  },
  {
    id: "UC-1165",
    words: w("Choose a matters folder outside Documents"),
    screen: SCREEN,
    render: WIN_BLOCKING,
  },
  { id: "UC-1166", words: w("Check again"), screen: SCREEN, render: WIN_BLOCKING },
  // the open matter's line (matter-blocked.tsx); its second sentence before these is UC-1162's
  {
    id: "UC-1167",
    words: m("Windows' Controlled folder access blocks saving in this folder"),
    screen: MATTER_SCREEN,
    render: MATTER_BLOCKED,
  },
  {
    id: "UC-1168",
    words: m("Move this matter's folder out of Documents in File Explorer, then choose it here."),
    screen: MATTER_SCREEN,
    render: MATTER_BLOCKED,
  },
  {
    id: "UC-1164",
    words: m("Choose the moved folder"),
    screen: MATTER_SCREEN,
    render: MATTER_BLOCKED,
  },
  {
    id: "UC-1169",
    words: joined([
      m("New matters now go in {folder}. Matters you already have stay where they are."),
      m("Your firm sets where new matters go, so it can't be changed here."),
      m("Clauseo couldn't use that folder. Choose it again."),
      m("Clauseo couldn't save the change. Try again."),
    ]),
    screen: MATTER_SCREEN,
    render: MATTER_CHOSEN,
  },
];
