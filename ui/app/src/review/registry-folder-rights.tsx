// Folder rights' copy review entries (FR-01 to FR-06, FR-11, FR-12, FR-15): the room's notice for a matter folder
// Clauseo can't fence, the text it opens for IT, the toolbar's Not protected, and the reason on an Editing files card,
// in the real room over FakeTransport, the host's folder_support set on the one matter. Apart from registry.tsx to
// keep that file under the size limit. New matter's warning (FR7 to FR10, FR13, FR14) and the mode picker's off
// options (FR16, FR17) wait on the host and on a mode picker.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { TURN, uuid } from "../testing/stream-script.ts";
import type { FolderSupportKind, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER, roomHost, threadHost } from "./hosts.ts";
import { RoomFocusing, RoomOver, RoomPressing } from "./states.tsx";
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
];
