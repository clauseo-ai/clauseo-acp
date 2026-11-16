// The copy review entries for a Claude launch the host refused because Claude's protection (its sandbox) couldn't be
// set up on macOS or Linux (UC-470 to UC-472, #sandbox-off-refuses): each is the question and its stop line, never a
// send's refusal, in place of the "U3-err-<code>" entry (registry-offline.tsx's ownEntry). UC-470 names the matter
// folder's own name with its *; UC-471 the other matter's folder whose * or ? pattern would cover this one's (with
// none to name, the code's own line, which names none); UC-472 is also matter.folder_wildcard's line with no folder
// to name. matter.name_unreadable (U3-err-matter.name_unreadable, provisional) is a launch refusal for a folder name
// Claude's sandbox can't read, which names no folder; it reads as the code's own line, never UC-472's.
import { step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

/** A turn Claude didn't start, failed with `code`, in a matter folder that shows as `folder`, on Linux, beside other
 * matters whose folders show as `others`. */
const stopped =
  (code: ErrorCode, folder: string, others: string[] = []) =>
  () => (
    <RoomOver
      host={() =>
        threadHost({
          entries: [
            userMessage(5, 1, "What does clause nine of the lease say about notice?"),
            step(9, { stop: "failed", failure: code }),
          ],
          platform: "linux",
          matterFolder: folder,
          otherFolders: others,
        })
      }
    />
  );

/** The entries in place of a refusal, by code. */
export const SANDBOX_OFF: Partial<Record<ErrorCode, Entry[]>> = {
  "matter.folder_wildcard": [
    {
      id: "UC-470",
      words: errorWords(
        "Rename the folder '{name}' so it has no * or ? in its name, then try again. Clauseo can't protect conversations in a folder with those characters.",
      ),
      screen: "room/thread",
      render: stopped("matter.folder_wildcard", "Documents/Matters/Acme*"),
    },
  ],
  "agent.sandbox_wildcard": [
    {
      id: "UC-471",
      words: errorWords(
        "Clauseo can't protect conversations here because the folder '{folder}' has * or ? in its name. Rename it so it has no * or ?, then try again.",
      ),
      screen: "room/thread",
      render: stopped("agent.sandbox_wildcard", "Documents/Matters/Acme", [
        "Documents/Matters/Ac?e",
      ]),
    },
  ],
  "agent.sandbox_unavailable": [
    {
      id: "UC-472",
      words: errorWords(
        "Clauseo couldn't set up its protection for conversations on this computer, so Claude didn't start. Restart your computer, then try again.",
      ),
      screen: "room/thread",
      render: stopped("agent.sandbox_unavailable", "Documents/Matters/Acme"),
    },
  ],
  "matter.name_unreadable": [
    {
      id: "U3-err-matter.name_unreadable",
      words: errorWords(
        "Claude can't start in any matter while a matter's folder, or Clauseo's data folder, leads to a folder with a name Clauseo can't read. Rename that folder, then try again.",
      ),
      screen: "room/thread",
      render: stopped("matter.name_unreadable", "Documents/Matters/Acme"),
    },
  ],
};
