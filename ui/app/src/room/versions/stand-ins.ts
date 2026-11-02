// What Versions can do only once a host change lands, each an optional method of one seam. The room passes none by
// default, so each affordance is absent in the desktop app, never a button that fails; tests and the copy review page
// pass theirs to the room explicitly. H11 adds openFile and reveal to HostTransport: then the room passes the
// transport's own (stand-ins.test.ts fails if their types drift from these meanwhile), and nothing in Versions changes.
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

/** No stand-ins: the desktop host's today. */
export const NO_STAND_INS: StandIns = {};
