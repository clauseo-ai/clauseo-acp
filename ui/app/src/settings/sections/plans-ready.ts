// "Add {plan}"'s sign-in once its install is ready. At the install's Ready desktop asks the plan's status check
// itself (crates/desktop/src/setup/backend.rs, install_event), which finds a login already on this computer; a
// start_sign_in sent at Ready raced that check and stopped it, so a lawyer whose login was on disk read "Sign-in didn't
// finish." So the add waits for the status answer heard after Ready: signed out starts the sign-in, once; signed in
// (its own login, a reused one or a key), a login found to offer, a sign-in already going or a failure send nothing;
// checking waits on.
//
// The wait is bounded. Desktop asks that check at most once per plan per run, and not at all when it can't start one,
// so a second Add in the same run, or an older desktop, may never answer: WAIT_MS after Ready (or after the last
// "checking") with no answer, the sign-in starts if the plan still reads signed out (or says nothing), and the watch
// ends either way. It also ends on any sign-in or key press in the plan's block (`dropAdd`), and when the plan leaves
// Ready. So a much later signed out (a failed launch's re-check) never opens a sign-in by itself.
//
// There is no setup command that only asks the status now: find_login answers with one state always, but it is a
// search that turns reuse back on, so it is not sent here.
//
// What desktop must send for the answer to count: a sign-in state AFTER Ready reaches the window (`checking`, then
// `signed_in`, `signed_out` or `login_found`). A state heard before Ready says nothing of the check (desktop shows a
// plan with no sign-in state yet as signed out), so it is ignored; the bound then decides.
import type { AgentId, SignIn } from "../../transport/protocol.ts";
import { isReady, type SetupStore } from "../../store/setup.ts";

/** How long the add waits for the status answer after Ready, or after the last "checking". */
export const WAIT_MS = 10_000;

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

/** The add forgotten, its watch and its timer stopped. */
export function dropAdd(store: SetupStore, agent: AgentId): void {
  addsOf(store).get(agent)?.();
}

/** "Add {plan}" pressed: from now its install is watched, and at Ready its status answer, as the head says. */
export function watchAdd(store: SetupStore, agent: AgentId): void {
  dropAdd(store, agent);
  let readyAt: { signIn: SignIn | undefined } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const signInNow = () => store.state().agents[agent]?.signIn;
  const start = () => {
    end();
    store.command({ start_sign_in: { agent } });
  };
  // no answer in time: signed out (or nothing said) starts it, anything else ends the wait
  const wait = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const now = signInNow();
      if (now === undefined || afterReady(now) === "start") start();
      else end();
    }, WAIT_MS);
  };
  const stop = store.subscribe(() => {
    const setup = store.state().agents[agent];
    if (readyAt === null) {
      if (!isReady(setup?.install)) return;
      readyAt = { signIn: setup?.signIn };
      wait();
      return;
    }
    if (!isReady(setup?.install)) return end();
    // the same state as at Ready (or as last heard): no answer yet
    if (setup?.signIn === readyAt.signIn) return;
    readyAt = { signIn: setup?.signIn };
    const next = afterReady(setup?.signIn);
    if (next === "wait") return wait();
    if (next === "start") return start();
    end();
  });
  const end = () => {
    stop();
    clearTimeout(timer);
    if (addsOf(store).get(agent) === end) addsOf(store).delete(agent);
  };
  addsOf(store).set(agent, end);
}
