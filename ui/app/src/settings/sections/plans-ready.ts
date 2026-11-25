// "Add {plan}"'s sign-in once its install is ready. At the install's Ready desktop asks the plan's status check
// itself (crates/desktop/src/setup/backend.rs, install_event), which finds a login already on this computer; a
// start_sign_in sent at Ready raced that check and stopped it, so a lawyer whose login was on disk read "Sign-in didn't
// finish." So the add waits for the status answer heard after Ready: signed out starts the sign-in, once; signed in
// (its own login, a reused one or a key), a login found to offer, a sign-in already going or a failure send nothing;
// checking waits on. With no answer the block shows the signed-out card's own Sign in.
//
// ASSUMPTION (stage D): desktop answers each Ready's status check with a sign-in state after Ready reaches the
// window (`checking`, then `signed_in`, `signed_out` or `login_found`). A state heard before Ready says nothing of the
// check (desktop shows a plan with no sign-in state yet as signed out), so only a later one counts.
import type { AgentId, SignIn } from "../../transport/protocol.ts";
import { isReady, type SetupStore } from "../../store/setup.ts";

/** What the status answer means for the add's sign-in: start it, wait on, or drop it. */
export function afterReady(signIn: SignIn | undefined): "start" | "wait" | "drop" {
  if (signIn === undefined || signIn === "checking") return "wait";
  if (typeof signIn === "object" && signIn.signed_out !== undefined) return "start";
  return "drop";
}

/** The adds waiting, by store: their plans, though the section was closed and opened meanwhile. */
const adds = new WeakMap<SetupStore, Map<AgentId, () => void>>();
const addsOf = (store: SetupStore) => {
  let map = adds.get(store);
  if (map === undefined) adds.set(store, (map = new Map()));
  return map;
};

/** Whether "Add {plan}" waits on its install or its status answer. */
export const adding = (store: SetupStore, agent: AgentId): boolean => addsOf(store).has(agent);

/** The add forgotten, its watch stopped. */
export function dropAdd(store: SetupStore, agent: AgentId): void {
  addsOf(store).get(agent)?.();
}

/** "Add {plan}" pressed: from now its install is watched, and at Ready its status answer, as the head says. */
export function watchAdd(store: SetupStore, agent: AgentId): void {
  dropAdd(store, agent);
  let readyAt: { signIn: SignIn | undefined } | null = null;
  const stop = store.subscribe(() => {
    const setup = store.state().agents[agent];
    if (readyAt === null) {
      if (isReady(setup?.install)) readyAt = { signIn: setup?.signIn };
      return;
    }
    // the same state as at Ready: no answer yet
    if (setup?.signIn === readyAt.signIn) return;
    const next = afterReady(setup?.signIn);
    if (next === "wait") return;
    end();
    if (next === "start") store.command({ start_sign_in: { agent } });
  });
  const end = () => {
    stop();
    if (addsOf(store).get(agent) === end) addsOf(store).delete(agent);
  };
  addsOf(store).set(agent, end);
}
