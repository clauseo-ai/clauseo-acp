// Versions' hold on one conversation's stream while the lawyer looks at a turn's version, and the reading back of that
// turn's step when the stream's window doesn't hold it (controller.ts): Undo needs the step's files. A step sought to
// the stream's start in vain is missing, and the toolbar's Undo walks past its turn until the step comes after all.
import type { ViewStore } from "../../store/store.ts";
import { streamOf } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StepEntry, ThreadId, TurnId } from "../../transport/protocol.ts";
import { pageBack, stepsSig } from "./reading.ts";
import type { Timeline, VersionMeta } from "./timeline.ts";

export class StepSeeker {
  readonly #transport: HostTransport;
  readonly #store: ViewStore;
  /** Called once a page read back has come in. */
  readonly #paged: () => void;
  #held: { thread: ThreadId; release: () => void } | null = null;
  #seeking: TurnId | null = null;
  readonly #steps = new Map<TurnId, StepEntry>();
  /** The turns whose step was sought and isn't there. */
  readonly missing = new Set<TurnId>();

  constructor(transport: HostTransport, store: ViewStore, paged: () => void) {
    this.#transport = transport;
    this.#store = store;
    this.#paged = paged;
  }

  /** A turn's step: from its conversation's stream, where the room holds it, or read back. */
  stepOf = (thread: ThreadId, turn: TurnId): StepEntry | undefined => {
    const e = this.#store.view(streamOf(thread))?.entries.get(`step:${turn}`);
    return e?.step ?? this.#steps.get(turn);
  };

  /** What a change of the seeker shows in: the held stream's steps and how many were read back. */
  sig(): string {
    const held = this.#held === null ? undefined : this.#store.view(streamOf(this.#held.thread));
    return `${stepsSig(held)}|${this.#steps.size}`;
  }

  /** Holds the conversation of the version looked at (none for another kind), and seeks its step if it isn't in. */
  hold(m: VersionMeta | undefined): void {
    const thread = m?.kind === "after_turn" ? m.thread : undefined;
    if (this.#held?.thread !== thread) {
      this.#held?.release();
      this.#held =
        thread === undefined ? null : { thread, release: this.#store.watch(streamOf(thread)) };
    }
    this.#seeking = m?.kind === "after_turn" && m.step === null ? (m.turn ?? null) : null;
    this.seek();
  }

  /** Pages the held conversation back until the sought step is in; paged to its start without it, the turn is
   * missing. */
  seek(): void {
    const held = this.#held;
    const turn = this.#seeking;
    if (held === null || turn === null) return;
    if (this.stepOf(held.thread, turn) !== undefined) {
      this.#seeking = null;
      return;
    }
    const stream = streamOf(held.thread);
    const view = this.#store.view(stream);
    if (view === undefined || view.epoch === null) return;
    this.#seeking = null;
    if (!view.hasOlder) {
      this.missing.add(turn);
      return;
    }
    void pageBack(this.#transport, stream, view, (page) => {
      for (const e of page) if (e.step !== undefined) this.#steps.set(e.step.turn, e.step);
      return this.#steps.has(turn);
    }).then(() => {
      if (!this.#steps.has(turn)) this.missing.add(turn);
      this.#paged();
    });
  }

  /** Whether a turn sought in vain has its step after all (its version came a flush before it); it is missing no
   * more. */
  found(timeline: Timeline): boolean {
    let any = false;
    for (const m of timeline.meta.values())
      if (m.turn !== undefined && m.step !== null && this.missing.delete(m.turn)) any = true;
    return any;
  }

  release(): void {
    this.#held?.release();
    this.#held = null;
  }
}
