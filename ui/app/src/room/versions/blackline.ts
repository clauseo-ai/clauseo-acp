// A Word document's changes, marked up by the host's Word tool (word_blackline): for the Word document in view in
// Compare with now, the version looked at against the folder now (or against a later version). One small store per
// open Versions, beside its controller. A marking up is kept once done, by its key (keyOf), so moving around Versions
// and back asks nothing again; one that failed is not kept, so coming back, or Try again, asks again. Only the answer
// for the key in view is shown: an answer for another key that comes late is kept when it marked the document up, and
// dropped when it failed.
import type { HostTransport } from "../../transport/host-transport.ts";
import { toHostError } from "../../transport/host-transport.ts";
import type {
  BlacklineParagraph,
  CheckpointId,
  FileChange,
  MatterId,
} from "../../transport/protocol.ts";

/** What the sheet asks to have marked up: the file, the version looked at, and the later version (to) or, for the
 * folder now, its content (now: the compare answer's `after`, else its size), so a change to the file asks again. */
export type MarkAsk = { path: string; from: CheckpointId; to?: CheckpointId; now: string };

/** How a marking up stands: running, done (its paragraphs, and whether the host cut the document short), or failed
 * with the host's code and whether another try may mend it. */
export type Mark =
  | { kind: "running" }
  | { kind: "shown"; paragraphs: readonly BlacklineParagraph[]; truncated: boolean }
  | { kind: "failed"; code: string; retryable: boolean };

/** One key per marking up: the file, the version, and the later version or the folder's content now. */
export const keyOf = (a: MarkAsk): string =>
  JSON.stringify([a.path, a.from, a.to === undefined ? `now:${a.now}` : `to:${a.to}`]);

/** A compared file the host's Word tool can mark up: a Word document (.docx) with both sides, then and now. */
export const markable = (f: FileChange): boolean =>
  /\.docx$/i.test(f.path) && f.change === "modified";

/** The ask for a compared file, against the folder now. */
export const markAskOf = (f: FileChange, from: CheckpointId): MarkAsk => ({
  path: f.path,
  from,
  now: f.after ?? `size:${f.size_bytes}`,
});

export class Blacklines {
  readonly #transport: HostTransport;
  readonly #matter: MatterId;
  readonly #marks = new Map<string, Mark>();
  readonly #listeners = new Set<() => void>();
  /** The key in view: the one whose answer the sheet shows. */
  #current: string | null = null;

  constructor(transport: HostTransport, matter: MatterId) {
    this.#transport = transport;
    this.#matter = matter;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** How the marking up for a key stands; undefined before it is asked. */
  markOf = (key: string): Mark | undefined => this.#marks.get(key);

  #set(key: string, mark: Mark | null): void {
    if (mark === null) this.#marks.delete(key);
    else this.#marks.set(key, mark);
    for (const fn of this.#listeners) fn();
  }

  /** The sheet shows this ask: it is marked up unless it is done or running. A failure shown for another key is
   * dropped, so coming back to it asks again; and Try again asks again for a key that failed. */
  ask(a: MarkAsk): void {
    const key = keyOf(a);
    this.#current = key;
    for (const [k, m] of this.#marks) if (k !== key && m.kind === "failed") this.#marks.delete(k);
    const mark = this.#marks.get(key);
    if (mark !== undefined && mark.kind !== "failed") return;
    this.#set(key, { kind: "running" });
    void this.#run(key, a);
  }

  /** The sheet no longer shows a marked-up document: an answer that comes now is not shown. */
  leave(): void {
    this.#current = null;
  }

  async #run(key: string, a: MarkAsk): Promise<void> {
    try {
      const r = await this.#transport.query({
        word_blackline: {
          matter: this.#matter,
          path: a.path,
          from: a.from,
          ...(a.to === undefined ? {} : { to: a.to }),
        },
      });
      if ("word_blackline" in r) {
        const { paragraphs, truncated } = r.word_blackline;
        this.#set(key, { kind: "shown", paragraphs, truncated });
      } else this.#failed(key, "internal", false);
    } catch (e) {
      const { code, retryable } = toHostError(e);
      this.#failed(key, code, retryable);
    }
  }

  /** A failure is shown only for the key in view, and kept for none. */
  #failed(key: string, code: string, retryable: boolean): void {
    this.#set(key, this.#current === key ? { kind: "failed", code, retryable } : null);
  }
}
