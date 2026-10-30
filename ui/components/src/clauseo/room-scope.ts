// One room's own reach. Every part of a room finds the elements it measures or moves through
// this scope, by refs the parts register as they mount, never by a lookup on the page; and every room keeps its own
// hand, its own motion timers and its own ids. Two rooms mounted on one page never touch each other. A part
// mounted outside any room takes a shared default scope, whose ids are the bare names.
import { createContext, use } from "react";
import { createFlourish, type Flourish } from "./flourish.ts";
import { createHand, type HandStore } from "./hand.ts";
import { RoomNodes } from "./room-nodes.ts";

export type { NodeName, KeyedKind } from "./room-nodes.ts";
export { RoomNodes };

export type RoomScope = {
  /** An id unique on the page: the room's prefix and the name. */
  id: (name: string) => string;
  nodes: RoomNodes;
  hand: HandStore;
  motion: Flourish;
  /** Whether this room takes the page's keys when nothing in particular is focused: the room last used. */
  takesKeys: () => boolean;
};

export function createScope(prefix: string): RoomScope {
  const nodes = new RoomNodes();
  const hand = createHand();
  const scope: RoomScope = {
    id: (name) => `${prefix}${name}`,
    nodes,
    hand,
    motion: createFlourish(nodes, hand),
    takesKeys: () => lastUsed === null || lastUsed === scope,
  };
  return scope;
}

/** The room the lawyer last pressed in or focused into; the first room to mount until then. */
let lastUsed: RoomScope | null = null;
/** Every room on the page. */
const mounted = new Set<RoomScope>();

/** The room a node stands in: inside its window, or inside its open card. */
export function roomOf(node: EventTarget | null): RoomScope | null {
  if (!(node instanceof Node)) return null;
  for (const r of mounted) {
    if (
      r.nodes.get("window")?.contains(node) === true ||
      r.nodes.get("card")?.contains(node) === true
    )
      return r;
  }
  return null;
}

/** Whether a key pressed where the focus stands is this room's: the focus is inside it, or in no room at all and
 * this is the room last used. */
export function keyIsMine(scope: RoomScope, target: EventTarget | null): boolean {
  const owner = roomOf(target);
  return owner === scope || (owner === null && scope.takesKeys());
}
const fallback = createScope("");
export const RoomContext = createContext<RoomScope>(fallback);

/** A room arriving on the page, and leaving it. */
export function enterRoom(scope: RoomScope): void {
  mounted.add(scope);
  lastUsed ??= scope;
}
export function leaveRoom(scope: RoomScope): void {
  mounted.delete(scope);
  if (lastUsed === scope) lastUsed = null;
}
/** The lawyer pressed in this room or focused into it. */
export function markUsed(scope: RoomScope): void {
  lastUsed = scope;
}

/** The room this part stands in. */
export function useRoom(): RoomScope {
  return use(RoomContext);
}
