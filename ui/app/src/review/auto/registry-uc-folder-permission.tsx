// UC's entries for a guarded folder picked in New matter (UC-1250 to UC-1255, issue 5): each kind's reason line beside
// Allow (UC-1256), and the line after macOS's dialog is turned down. The real room over FakeTransport (registry-nm.tsx's host),
// the pick carrying stage E's permission, and a fake ask_permission that answers denied with the System Settings path.
import type { Entry } from "../entry.ts";
import { EXISTING_PICKED, type Step } from "../hosts.ts";
import { formHost } from "../registry-nm.tsx";
import { RoomAfter } from "../states.tsx";
import { matterFormWords } from "../words.ts";
import { REASON } from "../../room/folder-permission.ts";
import {
  provideFolderPermission,
  type FolderPermissionId,
} from "../../transport/folder-permission.ts";

const PATH = "System Settings › Privacy & Security › Files and Folders";

/** A lawyer's folder of kind `id`, picked; Allow answered denied. */
const host = (id: FolderPermissionId) => () => {
  const transport = formHost("Desktop/Rathi Steels")();
  const pick = transport.host.pickFolder;
  transport.host.pickFolder = async () => {
    const picked = await pick?.();
    return picked === null || picked === undefined ? null : { ...picked, permission: id };
  };
  provideFolderPermission(transport, async () => ({
    id,
    state: "denied",
    folder: null,
    settings_path: PATH,
    note: null,
  }));
  return transport;
};

const DENIED: readonly Step[] = [...EXISTING_PICKED, { press: "folder-permission-allow" }];

const KINDS: readonly FolderPermissionId[] = [
  "files_desktop",
  "files_downloads",
  "files_removable",
  "files_network",
  "files_cloud",
];

const UC_FOLDER_PERMISSION: Entry[] = [
  ...KINDS.map((id, i): Entry => ({
    id: `UC-${1250 + i}`,
    words: matterFormWords(REASON[id]),
    screen: "matters/new-matter",
    render: () => <RoomAfter host={host(id)} steps={EXISTING_PICKED} />,
  })),
  {
    id: "UC-1255",
    words: matterFormWords(
      "Without it, Clauseo can't open or save papers in this folder. You can allow it later in {path}.",
    ),
    screen: "matters/new-matter",
    render: () => <RoomAfter host={host("files_desktop")} steps={DENIED} />,
  },
  {
    id: "UC-1256",
    words: matterFormWords("Allow"),
    screen: "matters/new-matter",
    render: () => <RoomAfter host={host("files_desktop")} steps={EXISTING_PICKED} />,
  },
];

export default UC_FOLDER_PERMISSION;
