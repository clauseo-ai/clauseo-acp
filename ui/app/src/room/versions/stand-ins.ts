// What Versions can do only once a host change lands, each an optional method of one seam. The desktop host supplies
// none of them, so each affordance is absent there, never a button that fails; FakeTransport-backed tests and the copy
// review page supply them. H11's open_file and reveal take the shapes H11's lead gave: when H11 adds them to
// HostTransport, the room passes them here (`standInsOf` below) and nothing in Versions changes.
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

const KEYS = [
  "openFile",
  "reveal",
  "versionsFolder",
  "diskUse",
] as const satisfies readonly (keyof StandIns)[];

/** The stand-ins a transport carries, by duck type: a transport with H11's methods (or a test's or the review page's
 * fake with any of these) brings them, bound to it. The desktop transport has none today. */
export function standInsOf(transport: object): StandIns {
  const t = transport as Partial<Record<keyof StandIns, unknown>>;
  const out: Record<string, unknown> = {};
  for (const key of KEYS) {
    const method = t[key];
    if (typeof method === "function")
      out[key] = (method as (...a: never[]) => unknown).bind(transport);
  }
  return out as StandIns;
}
