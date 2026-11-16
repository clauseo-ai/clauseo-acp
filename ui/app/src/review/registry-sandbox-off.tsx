// The copy review entries for a Claude launch the host refused because Claude's protection (its sandbox) couldn't be
// set up on macOS or Linux (UC-470 to UC-472, #sandbox-off-refuses): each is the question and its stop line, never a
// send's refusal, in place of the "U3-err-<code>" entry (registry-offline.tsx's ownEntry). UC-470 names the matter
// folder's own name with its *; UC-472 is also matter.folder_wildcard's line with no folder to name.
import { step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

/** A turn Claude didn't start, failed with `code`, in a matter folder that shows as `folder`, on Linux. */
const stopped = (code: ErrorCode, folder: string) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [
          userMessage(5, 1, "What does clause nine of the lease say about notice?"),
          step(9, { stop: "failed", failure: code }),
        ],
        platform: "linux",
        matterFolder: folder,
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
        "Clauseo can't protect conversations here because another matter's folder, or a folder Clauseo keeps its own files in, has * or ? in its name. Rename that folder so it has no * or ?, then try again. If you can't find it, ask your firm's IT.",
      ),
      screen: "room/thread",
      render: stopped("agent.sandbox_wildcard", "Documents/Matters/Acme"),
    },
  ],
  "agent.sandbox_unavailable": [
    {
      id: "UC-472",
      words: errorWords(
        "Clauseo couldn't set up its protection for conversations on this computer, so Claude didn't start. Restart your computer, then try again. If this keeps happening, ask your firm's IT.",
      ),
      screen: "room/thread",
      render: stopped("agent.sandbox_unavailable", "Documents/Matters/Acme"),
    },
  ],
};
