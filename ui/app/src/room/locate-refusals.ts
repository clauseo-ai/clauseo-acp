// What the lawyer's last Locate folder on each matter was refused with (relocate_matter's refusal): its code, the
// matter on the list it names (HostError's `matter`), and the folder picked as it reads. The open matter whose folder is
// away says it under the moved folder's words (moved.tsx's MovedMatter), Locate folder staying as the way to pick
// another; any other matter says it on its rail row's note line, with Locate folder beside it (rail.tsx). It clears on
// the next Locate of that matter, and as a folder that was away is found.
import { locateErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { MatterInfo } from "./host-next.ts";

/** A refused Locate folder: the code, the matter it names, the folder picked as it reads, and whether the matter's
 * folder was away when it was refused. */
export type LocateRefusal = {
  code: string;
  matter?: MatterId;
  path: string;
  missing: boolean;
};

export class LocateRefusals {
  #by: ReadonlyMap<MatterId, LocateRefusal> = new Map();
  readonly #listeners = new Set<() => void>();

  view = (): ReadonlyMap<MatterId, LocateRefusal> => this.#by;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** The matter's Locate folder was refused: it stands until the next Locate, or the folder is found. */
  refuse(matter: MatterId, refusal: LocateRefusal): void {
    this.#put(matter, refusal);
  }

  /** Takes back what the matter's last Locate said. */
  clear(matter: MatterId): void {
    if (this.#by.has(matter)) this.#put(matter, null);
  }

  /** The list as it reads now: a refusal of a matter off the list goes, and one taken while the folder was away goes
   * once it's found. */
  settle(list: readonly MatterInfo[]): void {
    if (this.#by.size === 0) return;
    const rows = new Map(list.map((m) => [m.matter, m]));
    for (const [matter, refusal] of this.#by) {
      const row = rows.get(matter);
      if (row === undefined || (refusal.missing && row.folder_state !== "missing"))
        this.clear(matter);
    }
  }

  #put(matter: MatterId, refusal: LocateRefusal | null): void {
    const by = new Map(this.#by);
    if (refusal === null) by.delete(matter);
    else by.set(matter, refusal);
    this.#by = by;
    for (const listener of this.#listeners) listener();
  }
}

/** A refused Locate's line for the matter `located`, in the language, naming the matter on `list` the refusal names. */
export function locateLine(
  lang: Lang,
  located: MatterId,
  refusal: LocateRefusal,
  list: readonly MatterInfo[],
): string {
  const named =
    refusal.matter === undefined ? undefined : list.find((m) => m.matter === refusal.matter)?.name;
  return locateErrorLine(lang, refusal.code, refusal.path, named, refusal.matter === located);
}
