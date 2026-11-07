// What became of the lawyer's open and show actions on a matter (H11's open_file and reveal) where
// the place they came from has no line of its own: a rail row's Show in Finder, a paper named in an
// answer, and a paper Find opens. The rail row says a refused Show in Finder on its note line; the
// composer's line says a refused open of the open matter. A declined confirm
// (`transport.unauthorized`) is the lawyer's own answer and says nothing (open.ts's refusalOf). A
// refusal clears as the action is tried again, or as the matter's folder is located. A folder that
// is gone or isn't the matter's (matter.folder_missing, matter.folder_not_a_matter) offers Locate
// folder (decision 0020) wherever its refusal is said; LocateFolderContext carries the open
// matter's Locate from the room down to those lines.
import { createContext, useContext } from "react";
import type { MatterId } from "../transport/protocol.ts";
import { refusalOf } from "./open.ts";

/** A refusal whose folder the lawyer can find again: Locate folder stands beside its words. */
export const locatable = (code: string | null): boolean =>
  code === "matter.folder_missing" || code === "matter.folder_not_a_matter";

/** Locate folder for the open matter: the picker, then relocate_matter (matters.ts's locate);
 * null with none open. */
export const LocateFolderContext = createContext<(() => void) | null>(null);

/** The open matter's Locate folder, or null. */
export const useLocateFolder = (): (() => void) | null => useContext(LocateFolderContext);

/** Which action was refused, and its code. */
export type ActRefusal = { act: "open" | "reveal"; code: string };

export class ActRefusals {
  #by: ReadonlyMap<MatterId, ActRefusal> = new Map();
  readonly #listeners = new Set<() => void>();

  view = (): ReadonlyMap<MatterId, ActRefusal> => this.#by;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Runs an open or show call on the matter: what it last said clears, and a refusal other than
   * a decline stands. */
  async track(act: ActRefusal["act"], matter: MatterId, call: Promise<unknown>): Promise<void> {
    this.clear(matter);
    const code = await refusalOf(act === "open" ? "open_file" : "reveal", call);
    if (code !== null) this.#put(matter, { act, code });
  }

  /** Takes back what the matter's last action said. */
  clear(matter: MatterId): void {
    if (this.#by.has(matter)) this.#put(matter, null);
  }

  #put(matter: MatterId, refusal: ActRefusal | null): void {
    const by = new Map(this.#by);
    if (refusal === null) by.delete(matter);
    else by.set(matter, refusal);
    this.#by = by;
    for (const listener of this.#listeners) listener();
  }
}
