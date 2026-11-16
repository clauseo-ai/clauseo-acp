// What a refused Locate says (matters.ts's locate): the host refused relocate_matter, and the moved folder's line
// (moved.tsx) says why, in New matter's words for that code (matter-form-check.ts's createErrorLine), and any other
// code's words from words-errors.ts, the generic one for a code this version doesn't know. The refusal stands until the
// lawyer picks again (matters.ts's locate), so Locate folder is never stranded.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { ERROR_WORDS, errorWords } from "@clauseo/components/lib/words-errors.ts";
import type { MatterId } from "../transport/protocol.ts";

/** A Locate the host refused: its code, the matter it names when the list has it, and the folder as it shows. */
export type LocateRefusal = {
  code: string;
  matter?: MatterId | undefined;
  path?: string | undefined;
};

/** The line a refused Locate says. A folder code the words have (New matter's folder refusals) takes the line
 * createErrorLine gives it, with the matter's name when the list has it; any other code takes its words in
 * words-errors.ts, which an unknown code reads as the generic line. */
export function locateLine(lang: Lang, refusal: LocateRefusal, name?: string): string {
  const { code, path } = refusal;
  return code.startsWith("matter.folder_") && Object.hasOwn(ERROR_WORDS, code)
    ? createErrorLine(lang, code, path, name)
    : errorWords(lang, code);
}

/** The refused Locate's line for the open matter, naming the matter the refusal names when `matters` has it. */
export function locateRefusalLine(
  lang: Lang,
  refusal: LocateRefusal,
  matters: readonly { matter: MatterId; name: string }[],
): string {
  const named =
    refusal.matter === undefined
      ? undefined
      : matters.find((m) => m.matter === refusal.matter)?.name;
  return locateLine(lang, refusal, named);
}

/** What each matter's last Locate was refused with, read by the room (useSyncExternalStore). */
export class LocateRefusals {
  #by: ReadonlyMap<MatterId, LocateRefusal> = new Map();
  readonly #listeners = new Set<() => void>();

  view = (): ReadonlyMap<MatterId, LocateRefusal> => this.#by;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  refuse(matter: MatterId, refusal: LocateRefusal): void {
    this.#put(matter, refusal);
  }

  /** Takes back what the matter's last Locate said, as the lawyer picks again. */
  clear(matter: MatterId): void {
    if (this.#by.has(matter)) this.#put(matter, null);
  }

  #put(matter: MatterId, refusal: LocateRefusal | null): void {
    const by = new Map(this.#by);
    if (refusal === null) by.delete(matter);
    else by.set(matter, refusal);
    this.#by = by;
    for (const listener of this.#listeners) listener();
  }
}
