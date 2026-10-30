// The elements one room measures or moves, registered by the parts as they mount. Nothing here
// looks anything up on the page: a part hands its element over through a stable ref, and takes it back as it leaves.
import type { Target } from "./hand.ts";

/** The single elements a room measures or moves. */
export type NodeName =
  | "window"
  | "centre"
  | "thread"
  | "threadInner"
  | "pin"
  | "pane"
  | "facade"
  | "composer"
  | "field"
  | "questionStep"
  | "asked"
  | "askedWho"
  | "answerStep"
  | "meta"
  | "workDone"
  | "answer"
  | "acceptRow"
  | "otherside"
  | "ready"
  | "turnScroll"
  | "ghost"
  | "card"
  | "citeMore";

/** The elements a room keeps by a paper's or a judgment's id. tile: a paper's place on the wall. face: its button.
 * cite: a citation row's place in the list. row: the row itself, which moves in hand. link: its name. check: its
 * Good law chip. head: a wall section's heading row, by section. */
export type KeyedKind = "tile" | "face" | "cite" | "row" | "link" | "check" | "head";

export type Attach<T extends Element> = (node: T | null) => void | (() => void);

export class RoomNodes {
  private named = new Map<NodeName, HTMLElement>();
  private keyed = new Map<string, HTMLElement>();
  private namedRefs = new Map<NodeName, Attach<HTMLElement>>();
  private keyedRefs = new Map<string, Attach<HTMLElement>>();
  private refsByKey = new Map<string, Set<HTMLElement>>();
  private refRefs = new Map<string, Attach<HTMLElement>>();
  private blockSet = new Set<HTMLElement>();
  private triggerSet = new Set<HTMLElement>();
  private readonly saysRef: Attach<HTMLElement> = (node) => {
    if (node === null) return;
    this.blockSet.add(node);
    return () => this.blockSet.delete(node);
  };
  /** A stable ref for one of the answer's blocks: the words that take the lawyer's weight. */
  says<T extends HTMLElement>(): Attach<T> {
    return this.saysRef as Attach<T>;
  }

  /** A stable ref for one named element. */
  slot<T extends HTMLElement>(name: NodeName): Attach<T> {
    let ref = this.namedRefs.get(name);
    if (ref === undefined) {
      ref = (node) => {
        if (node === null) return;
        this.named.set(name, node);
        return () => {
          if (this.named.get(name) === node) this.named.delete(name);
        };
      };
      this.namedRefs.set(name, ref);
    }
    return ref as Attach<T>;
  }
  get<T extends HTMLElement = HTMLElement>(name: NodeName): T | null {
    return (this.named.get(name) as T | undefined) ?? null;
  }

  /** A stable ref for one element kept by id. trigger: it opens a card, so a press on it is its own, never a press
   * outside the card. */
  keep<T extends HTMLElement>(kind: KeyedKind, id: string, trigger = false): Attach<T> {
    const key = `${kind}:${id}`;
    const refKey = `${key}:${trigger}`;
    let ref = this.keyedRefs.get(refKey);
    if (ref === undefined) {
      ref = (node) => {
        if (node === null) return;
        this.keyed.set(key, node);
        if (trigger) this.triggerSet.add(node);
        return () => {
          if (this.keyed.get(key) === node) this.keyed.delete(key);
          this.triggerSet.delete(node);
        };
      };
      this.keyedRefs.set(refKey, ref);
    }
    return ref as Attach<T>;
  }
  find<T extends HTMLElement = HTMLElement>(kind: KeyedKind, id: string): T | null {
    return (this.keyed.get(`${kind}:${id}`) as T | undefined) ?? null;
  }
  /** Every element of a kind, in the order they joined. */
  all(kind: KeyedKind): HTMLElement[] {
    const out: HTMLElement[] = [];
    for (const [k, n] of this.keyed) if (k.startsWith(`${kind}:`)) out.push(n);
    return out;
  }

  /** A stable ref for a reference in the answer's words, kept with every other reference to the same card. */
  ref<T extends HTMLElement>(card: string): Attach<T> {
    let ref = this.refRefs.get(card);
    if (ref === undefined) {
      ref = (node) => {
        if (node === null) return;
        let set = this.refsByKey.get(card);
        if (set === undefined) {
          set = new Set();
          this.refsByKey.set(card, set);
        }
        set.add(node);
        this.triggerSet.add(node);
        return () => {
          set.delete(node);
          this.triggerSet.delete(node);
        };
      };
      this.refRefs.set(card, ref);
    }
    return ref as Attach<T>;
  }
  /** Every reference in the answer to one card, "paper:minutes", in the page's order. */
  refsTo(card: string): HTMLElement[] {
    return inOrder([...(this.refsByKey.get(card) ?? [])]);
  }
  /** The answer's blocks inside an element, in the page's order: the words that take the lawyer's weight. */
  blocksIn(root: HTMLElement): HTMLElement[] {
    return inOrder([...this.blockSet].filter((b) => root.contains(b)));
  }
  /** The block a reference stands in. */
  blockOf(node: Element): HTMLElement | null {
    for (let e = node.parentElement; e !== null; e = e.parentElement)
      if (this.blockSet.has(e)) return e;
    return null;
  }
  /** Whether a press landed on something that has a card of its own. */
  isTrigger(node: EventTarget | null): boolean {
    if (!(node instanceof Node)) return false;
    for (const t of this.triggerSet) if (t.contains(node)) return true;
    return false;
  }

  /** The room's window: floating cards stay inside it. */
  windowBox(): DOMRect {
    return (this.get("window") ?? document.documentElement).getBoundingClientRect();
  }
  /** The element a card speaks for: a tile's place on the wall or a citation row's place in the list. */
  target(t: Target): HTMLElement | null {
    return this.find(t.kind === "cite" ? "cite" : "tile", t.id);
  }
}

/** Elements in the order they stand in the page. */
function inOrder(list: HTMLElement[]): HTMLElement[] {
  return list.sort((a, b) =>
    a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1,
  );
}
