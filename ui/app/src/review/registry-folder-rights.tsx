// Folder rights' copy review entries (FR-01 to FR-15, FR-20): the room's notice for a matter folder Clauseo can't
// fence or couldn't check, the text it opens for IT, the toolbar's Not protected, the reason on an Editing files card,
// in the real room over FakeTransport, the host's folder_support set on the one matter; and New matter's warning for
// a folder picked, the host's FolderSupport answering. Apart from registry.tsx to keep that file under the size
// limit. The mode picker's off options (FR16, FR17) wait on a mode picker.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { TURN, uuid } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { FolderSupportKind, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER, NEW_MATTER, roomHost, threadHost, type Step } from "./hosts.ts";
import { RoomAfter, RoomFocusing, RoomOver, RoomPressing } from "./states.tsx";
import { folderCardWords, folderWords as w, uiWords, type WordsRef } from "./words.ts";

/** Several lines of one id, joined with " / " as the approved page writes them. */
const joined = (parts: readonly WordsRef[]): WordsRef => ({
  file: [...new Set(parts.map((p) => p.file))].join(", "),
  key: parts.map((p) => p.key).join(" / "),
  text: (lang: Lang) => parts.map((p) => p.text(lang)).join(" / "),
  later: parts.some((p) => p.later),
});
const notProtected: WordsRef = {
  file: "lib/words-toolbar.ts",
  key: "Not protected",
  text: (lang) => tbt(lang, "Not protected"),
  later: tbt.hi["Not protected"] === null,
};

/** The one matter, its folder where the approved page shows it, with the host's reading. */
const room = (kind: FolderSupportKind) => () =>
  roomHost([{ ...MATTER, folder_display: "D:\\Matters\\Acme Traders", folder_support: kind }]);
const INFO = '[data-testid="folder-notice"] .room-info';

/** New matter over a host whose picker gives a folder on the approved page's drive and whose check reads `kind`. */
const picking = (kind: FolderSupportKind) => () =>
  new FakeTransport({
    query: (query) =>
      typeof query === "object" && "folder_support" in query
        ? { folder_support: { support: kind } }
        : { matters: [MATTER] },
    pickFolder: () => ({
      folder: "review-folder",
      display:
        kind === "network" ? "\\\\fileserver\\matters\\Rathi Steels" : "E:\\Matters\\Rathi Steels",
    }),
  });
/** New matter, the name typed, and Change: the folder picked, its warning under the folder line. */
const PICKED: readonly Step[] = [...NEW_MATTER, { press: "matter-existing" }];
const warned = (kind: FolderSupportKind) => () => <RoomAfter host={picking(kind)} steps={PICKED} />;
const entry = (id: string, words: WordsRef, render: () => ReactNode): Entry => ({
  id,
  words,
  screen: "room/folder-rights",
  render,
});

/** A pending Editing files card in a folder whose permissions can't change. */
const EDITING: ViewEntry[] = [
  {
    tool_call: {
      order: 2,
      tool_call: uuid(1, 7),
      turn: TURN,
      started_at: 2000,
      title: "Editing files",
      kind: "edit",
      status: "pending",
      locations: [],
      content: [],
    },
  },
  {
    permission: {
      order: 3,
      request: uuid(1, 8),
      tool_call: uuid(1, 7),
      options: [
        { option_id: "once", kind: "allow_once", agent_label: "Allow" },
        { option_id: "deny", kind: "reject_once", agent_label: "Deny" },
      ],
      deliverable: "live",
      state: "pending",
    },
  },
];
const card = () =>
  threadHost({ entries: EDITING, status: "needs_you", folder: "cant_change_permissions" });

export const FOLDER_RIGHTS: Entry[] = [
  // the notice, its title, and its name under the pointer and for a screen reader
  entry(
    "FR-01",
    joined([w("Clauseo asks before each edit"), w("Clauseo asks before each edit in this folder")]),
    () => <RoomOver host={room("cant_change_permissions")} />,
  ),
  // behind its info icon
  entry(
    "FR-02",
    w(
      "You can change files here, but not this folder's permissions. Clauseo needs them to keep Claude and Codex inside the matter. Some edits won't go through.",
    ),
    () => <RoomFocusing host={room("cant_change_permissions")} focus={INFO} />,
  ),
  entry("FR-03", w("What to tell IT"), () => <RoomOver host={room("cant_change_permissions")} />),
  // Close, the cross's tooltip and name: the existing word
  entry("FR-04", uiWords("Close"), () => (
    <RoomFocusing host={room("cant_change_permissions")} focus='[data-testid="folder-close"]' />
  )),
  // What to tell IT open: the text for IT, the matter's folder in it, selected, and Copy
  entry(
    "FR-05",
    joined([
      w(
        'Please give my account "Change permissions" on this folder: {path}. Clauseo, the app I use for this matter, adds one entry to the folder\'s permissions so Claude and Codex, which it runs, can write only inside it.',
      ),
      w("Copy"),
    ]),
    () => <RoomPressing host={room("cant_change_permissions")} press="folder-it" />,
  ),
  // the reason under "Editing files" on the card, and behind its info icon
  entry(
    "FR-06",
    joined([
      folderCardWords("Can't set folder permissions"),
      folderCardWords(
        "Clauseo can't change this folder's permissions, so it asks before each edit.",
      ),
    ]),
    () => <RoomFocusing host={card} focus='[data-testid="folder-reason"] .room-info' />,
  ),
  // New matter's warning for a folder on a network drive: its title, its name, and its body with why behind the info
  // icon (FR8's why provisional, "an agent" reworded)
  entry(
    "FR-07",
    joined([w("On a network drive"), w("This folder is on a network drive")]),
    warned("network"),
  ),
  entry(
    "FR-08",
    joined([
      w("Claude asks before every change. Codex can't work here."),
      w("Clauseo can't keep Claude and Codex inside a folder on a network drive."),
    ]),
    warned("network"),
  ),
  // the warning's buttons: another folder through the picker, and the matter made there anyway
  entry("FR-09", w("Choose another folder"), warned("network")),
  entry("FR-10", w("Create anyway"), warned("network")),
  // a network drive's matter in the toolbar: the existing word and its open lock
  entry("FR-11", notProtected, () => <RoomOver host={room("network")} />),
  // a network drive's notice: its words, its name, and behind its info icon
  entry(
    "FR-12",
    joined([
      w("On a network drive."),
      w("Claude asks before every change. Codex can't work here."),
      w("This matter's folder is on a network drive"),
      w(
        "Clauseo can't keep Claude and Codex inside this folder. For full protection, move the matter to a folder on this computer.",
      ),
    ]),
    () => <RoomFocusing host={room("network")} focus={INFO} />,
  ),
  // a drive without file permissions, as FR-12
  entry(
    "FR-15",
    joined([
      w("No file permissions."),
      w("Claude asks before every change. Codex can't work here."),
      w("This matter's folder is on a drive that doesn't keep file permissions"),
      w(
        "Clauseo can't keep Claude and Codex inside this folder. USB drives often don't keep permissions. For full protection, move the matter to a folder on this computer.",
      ),
    ]),
    () => <RoomFocusing host={room("no_permissions")} focus={INFO} />,
  ),
  // New matter's warning for a drive without file permissions, as FR-07 and FR-08 (FR14's why provisional)
  entry(
    "FR-13",
    joined([w("No file permissions"), w("This drive doesn't keep file permissions")]),
    warned("no_permissions"),
  ),
  entry(
    "FR-14",
    joined([
      w("Claude asks before every change. Codex can't work here."),
      w(
        "Clauseo can't keep Claude and Codex inside a folder on this drive. USB drives often don't keep permissions.",
      ),
    ]),
    warned("no_permissions"),
  ),
  // the room's notice for a folder the host couldn't check (provisional): one line, its name too
  entry(
    "FR-20",
    w("Clauseo couldn't check this folder, so Claude asks before every change."),
    () => <RoomOver host={room("check_failed")} />,
  ),
];
