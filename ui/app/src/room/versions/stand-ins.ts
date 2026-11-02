// What Versions does through the host, each an optional method of one seam. The room passes the transport's own
// openFile and reveal (H11) unless it is given stand-ins: tests and the copy review page pass theirs to the room
// explicitly (stand-ins.test.tsx fails if their types drift from HostTransport's). versionsFolder and diskUse wait for
// H12: until then nothing passes them in the desktop app, so their affordances are absent, never a button that fails.
import type { FileRefId, MatterId } from "../../transport/protocol.ts";

/** What H11's reveal shows: a matter's folder, or Clauseo's own data folder, where versions are kept. */
export type RevealTarget = { matter: MatterId } | "app_data";

export type StandIns = {
  /** H11: opens a file of the matter in the program the computer opens it with. Rejects with a HostError. */
  openFile?: (file: FileRefId) => Promise<void>;
  /** H11: shows a folder in Finder or File Explorer. Rejects with a HostError. */
  reveal?: (target: RevealTarget) => Promise<void>;
  /** H12: Clauseo's own folder for the matter's versions, as the computer writes it. */
  versionsFolder?: (matter: MatterId) => Promise<string>;
  /** H12: how much room the matter's versions take, as the computer says it: "412 MB". */
  diskUse?: (matter: MatterId) => Promise<string>;
};

/** No stand-ins at all: Versions' default when the room passes none. The room itself always passes the transport's
 * openFile and reveal, or the stand-ins it was given. */
export const NO_STAND_INS: StandIns = {};
