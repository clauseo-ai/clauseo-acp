// What a new conversation's default plan weighs beyond its matter (agent.ts's agentFor, D-014, D-043): which plans are
// signed in, from the setup store the first run keeps (store/setup.ts, one per transport, started by first-run.tsx), so
// the default follows a sign-in or sign-out as it happens; and the plan the lawyer last asked a question with, in any
// matter, kept in this computer's localStorage beside the display settings (display-store.ts's clauseo.display). The
// per-matter memory waits for the pickers' host prefs (UpdateMatter.agent_prefs); a matter's own threads stand for it.
import type { HostTransport } from "../transport/host-transport.ts";
import type { AgentId, AgentKind } from "../transport/protocol.ts";
import { isSignedIn, setupStoreOf, type SetupSource, type SetupState } from "../store/setup.ts";
import { hasSetup } from "./first-run/hold.ts";
import { kindOf } from "./agent.ts";

/** The plans signed in, by kind; null while no sign-in state has been heard. */
export type Signed = ReadonlySet<AgentKind> | null;

/** The localStorage key of the plan last asked with: an agent id. */
export const LAST_PLAN_KEY = "clauseo.last-plan";

type Store = Pick<Storage, "getItem" | "setItem">;

/** The page's localStorage, or null where it can't be reached (a sandboxed page, a test without it). */
const local = (): Store | null => {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
};

/** The plan the lawyer last asked a question with, anywhere; null before the first. */
export function lastPlan(storage: Store | null = local()): AgentId | null {
  try {
    const id = storage?.getItem(LAST_PLAN_KEY) ?? null;
    return id === null || id === "" ? null : id;
  } catch {
    return null;
  }
}

/** Keeps `agent` as the plan last asked with. */
export function rememberPlan(agent: AgentId, storage: Store | null = local()): void {
  try {
    if (lastPlan(storage) !== agent) storage?.setItem(LAST_PLAN_KEY, agent);
  } catch {
    // only the next default reads it
  }
}

/** The plans signed in, by kind; null while no sign-in state has been heard (a host without the setup wire, or
 * before its first word), when the default doesn't weigh sign-in. */
export function signedKinds(state: SetupState): Signed {
  const heard = Object.entries(state.agents).filter(([, a]) => a.signIn !== undefined);
  if (heard.length === 0) return null;
  const kinds = new Set<AgentKind>();
  for (const [id, a] of heard) {
    const kind = kindOf(id, []);
    if (kind !== undefined && isSignedIn(a.signIn)) kinds.add(kind);
  }
  return kinds;
}

const same = (a: Signed, b: Signed) =>
  a === b || (a !== null && b !== null && a.size === b.size && [...a].every((k) => b.has(k)));

/** The signed-in plans, as last heard: `signed`, null until a sign-in state is heard. */
export class SignedPlans {
  signed: Signed = null;

  /** Hears them where the transport carries the setup wire, calling `changed` after each new set (the first at once if
   * one is known). Returns the stop; a transport without the wire hears nothing. */
  watch(transport: HostTransport, changed: () => void): () => void {
    if (!hasSetup(transport)) return () => {};
    const store = setupStoreOf(transport as HostTransport & SetupSource);
    const read = () => {
      const next = signedKinds(store.state());
      if (same(next, this.signed)) return;
      this.signed = next;
      changed();
    };
    read();
    return store.subscribe(read);
  }
}
