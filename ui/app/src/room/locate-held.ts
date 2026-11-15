// Locate's step for a folder Claude can't start in (FR24, FR25): on macOS and Linux, a folder picked to locate a matter
// whose name has a backslash (folder-backslash.ts) is held here instead of relocated at once (matters.ts's locate),
// and the room's step (locate-warning.tsx) asks: Choose another folder, which opens the picker again, or Locate anyway,
// which relocates the matter there. The step's host tells the store the OS while it is mounted; with none mounted
// (a test's Matters alone) nothing is held, and Locate goes straight through.
import type { Platform } from "@clauseo/components/lib/display.ts";
import type { MatterId } from "../transport/protocol.ts";
import { hasBackslash } from "./folder-backslash.ts";

/** A pick held back for the step: the folder as it shows, and the step's two ways on. */
export type HeldPick = {
  display: string;
  /** Choose another folder: the picker again. */
  again: () => Promise<void>;
  /** The other button: go on with the folder held. */
  anyway: () => Promise<void>;
};

/** A Locate held back: the matter, the folder as it shows, and the step's two ways on: the picker again for the same
 * matter, or relocate_matter with the folder held. */
export type HeldLocate = HeldPick & { matter: MatterId };

export class LocateHeld {
  #platform: Platform | null = null;
  #held: HeldLocate | null = null;
  readonly #listeners = new Set<() => void>();

  current = (): HeldLocate | null => this.#held;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** The OS the step is shown on, from its host while mounted; null with no step to show. */
  shown(platform: Platform | null): void {
    this.#platform = platform;
    if (platform === null) this.drop();
  }

  /** Whether a folder picked with this display waits for the step. */
  warns(display: string): boolean {
    return this.#platform !== null && hasBackslash(display, this.#platform);
  }

  hold(held: HeldLocate): void {
    this.#put(held);
  }

  /** The step closed, by either button, Escape, or a new hold. */
  drop(): void {
    if (this.#held !== null) this.#put(null);
  }

  #put(held: HeldLocate | null): void {
    this.#held = held;
    for (const listener of this.#listeners) listener();
  }
}
