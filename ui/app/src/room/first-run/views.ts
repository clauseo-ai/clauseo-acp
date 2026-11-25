// The setup store's state as the first run's parts show it: the install as the working line says it, and the sign-in
// as the gate says it. Pure, so each state's screen is tested from the host's words alone.
import type { GateRefusal, GateView } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import type { InstallView } from "@clauseo/components/clauseo/first-launch/install.tsx";
import type { ToolsView } from "@clauseo/components/clauseo/first-launch/tools.tsx";
import type { AgentId, InstallProgress, SignIn } from "../../transport/protocol.ts";
import {
  PLANS,
  isReady,
  isSignedIn,
  mbOf,
  signInAhead,
  type AgentSetup,
  type SetupState,
} from "../../store/setup.ts";
import { NAME, planOf } from "./acts.ts";

/** A download's size as the line says it, "about": the nearest 10 MB from 100 MB up,
 * whole MB below. 0 stays 0. */
export const aboutMb = (mb: number): number => (mb >= 100 ? Math.round(mb / 10) * 10 : mb);

/** The install as the working line shows it, from the host's InstallProgress; null with nothing to say. `online`: what
 * the window says of the network (online.ts), so a waiting install says it can't reach the download, not "offline". */
export function installView(setup: AgentSetup | undefined, online: boolean): InstallView | null {
  const install = setup?.install;
  if (install === undefined || install === "managed") return null;
  const update = updateView(setup, install);
  if (update !== null) return update;
  if (install === "installing") return { kind: "installing" };
  // paused, which only the lawyer does (the other plan's download never pauses it): it says where it stands, still
  if (install === "paused")
    return setup?.mb === undefined ? null : { kind: "missing", mb: aboutMb(setup.mb) };
  if ("missing" in install && install.missing !== undefined) {
    const mb = aboutMb(mbOf(install.missing.download_bytes));
    // Clauseo installed it here before and its folder went (#859): the line offers it again
    return install.missing.installed_before === true
      ? { kind: "missing", mb, reinstall: true }
      : { kind: "missing", mb };
  }
  if ("downloading" in install && install.downloading !== undefined) {
    const { received, total } = install.downloading;
    // done never above total: the bar and the words agree
    const totalMb = mbOf(total);
    return {
      kind: "downloading",
      received,
      total,
      doneMb: Math.min(mbOf(received), totalMb),
      totalMb,
    };
  }
  if ("waiting" in install && install.waiting !== undefined)
    return online ? { kind: "offline", online } : { kind: "offline" };
  if ("failed" in install && install.failed !== undefined) {
    const f = install.failed;
    if (f.code === "agent.install_blocked") return { kind: "blocked", hosts: f.hosts };
    if (f.code === "agent.install_disk_full")
      return { kind: "disk", need: f.need_mb ?? 0, free: f.free_mb ?? 0 };
    // every file downloaded, and setting them up failed: words that say the download worked
    if (f.code === "agent.install_setup_failed") return { kind: "failed", log: f.log, setup: true };
    return { kind: "failed", log: f.log };
  }
  return null;
}

/** A plan the lawyer has, its install redone after an update (store/agent-update.ts): stopped (Missing), paused,
 * downloading, setting up or failed, each in the update's words; null for no update, or a state the normal line says
 * with its reason and Try again (offline, which retries by itself; blocked; a full disk; setting up failed). */
function updateView(setup: AgentSetup | undefined, install: InstallProgress): InstallView | null {
  if (setup?.update !== true) return null;
  if (install === "paused") return { kind: "update-stopped", paused: true };
  if (install === "installing") return { kind: "installing", update: true };
  if (typeof install !== "object") return null;
  if (install.missing !== undefined) return { kind: "update-stopped", paused: false };
  if (install.downloading !== undefined) {
    const { received, total } = install.downloading;
    const totalMb = mbOf(total);
    const doneMb = Math.min(mbOf(received), totalMb);
    return { kind: "downloading", received, total, doneMb, totalMb, update: true };
  }
  if (install.failed?.code === "agent.install_failed")
    return { kind: "failed", log: install.failed.log, update: true };
  return null;
}

/** A waiting sign-in as the gate and the plan's line say it: its install waiting, it opens when the download reconnects
 * (`online`, the window online meanwhile: when setting up can continue); with the plan installed while the other
 * plan's sign-in is under way, it opens after that one (one sign-in at a time); else it opens when the plan is ready.
 * `agent` left out (a caller that doesn't know it), never the second. The sign-in ahead in the browser shows its own
 * line with its Cancel beside this one; with a code or switching accounts it shows no line on the first run's screens,
 * so this one offers to cancel it (`ahead`). */
export function queuedView(
  setup: AgentSetup | undefined,
  state: SetupState,
  agent: AgentId | undefined,
  online: boolean,
): Extract<GateView, { kind: "queued" }> {
  const install = setup?.install;
  const offline =
    typeof install === "object" && "waiting" in install && install.waiting !== undefined;
  if (offline) return online ? { kind: "queued", offline, online } : { kind: "queued", offline };
  const ahead = agent === undefined ? null : signInAhead(state, agent);
  const installed = install === "managed" || isReady(install);
  if (ahead === null || !installed) return { kind: "queued", offline };
  const other = state.agents[ahead]?.signIn;
  const hidden =
    typeof other === "object" && (other.device_code !== undefined || other.switching !== undefined);
  return hidden
    ? { kind: "queued", offline, after: NAME[planOf(ahead)], ahead: true }
    : { kind: "queued", offline, after: NAME[planOf(ahead)] };
}

/** The gate for a plan's sign-in, or null when it is signed in or nothing is known of it (a host before R20). `online`:
 * what the window says of the network, for a queued sign-in whose install waits (queuedView). */
export function gateView(
  setup: AgentSetup | undefined,
  state: SetupState,
  clock: (at: number) => string,
  agent: AgentId | undefined,
  online: boolean,
): GateView | null {
  const signIn = setup?.signIn;
  if (signIn === undefined || isSignedIn(signIn)) return null;
  // a plan being updated asks for no sign-in until it is ready: the install line stands alone (agent-update.ts)
  if (setup?.update === true && !isReady(setup.install) && signInIdle(signIn)) return null;
  if (state.keyOpen) return { kind: "key" };
  if (signIn === "checking") return { kind: "checking" };
  if (signIn === "queued") return queuedView(setup, state, agent, online);
  if ("signed_out" in signIn && signIn.signed_out !== undefined) {
    // none found and reuse off are the plain sign-in screen here; Plans (UD2) says them
    const found = signIn.signed_out.found;
    const plain = found === "none_found" || found === "reuse_off";
    return { kind: "signed-out", found: plain ? undefined : found };
  }
  if ("waiting" in signIn && signIn.waiting !== undefined)
    return { kind: "waiting", nudge: signIn.waiting.nudge, pasting: state.pasting };
  if ("device_code" in signIn && signIn.device_code !== undefined)
    return {
      kind: "device",
      code: signIn.device_code.code,
      until: clock(signIn.device_code.expires_at),
    };
  if ("login_found" in signIn && signIn.login_found !== undefined)
    return { kind: "login-found", email: signIn.login_found.email };
  if ("switching" in signIn && signIn.switching !== undefined)
    return { kind: "switching", stuck: signIn.switching.stuck };
  if ("failed" in signIn && signIn.failed !== undefined) {
    // the agent's own refusal for no sign-in is the plan screen again
    if (signIn.failed.code === "agent.auth_required") return { kind: "signed-out" };
    return { kind: "failed", code: signIn.failed.code, at: clock(signIn.failed.at) };
  }
  return { kind: "signed-out" };
}

/** A sign-in state that would offer a sign-in: signed out, a login found, or the agent's refusal for none. */
const signInIdle = (signIn: SignIn): boolean =>
  typeof signIn === "object" &&
  (signIn.signed_out !== undefined ||
    signIn.login_found !== undefined ||
    signIn.failed?.code === "agent.auth_required");

/** The last setup command desktop refused for the plan, as the gate's line says it; null with none. A
 * `command.conflict` while the plan is switching accounts says only that the switch isn't done. */
export function refusalView(setup: AgentSetup | undefined): GateRefusal | null {
  if (setup?.refused === undefined) return null;
  const signIn = setup.signIn;
  const switching = typeof signIn === "object" && signIn.switching !== undefined;
  return { code: setup.refused, switching };
}

/** Whether a plan isn't here yet and says its size: missing, or its download paused by the lawyer. */
const awaited = (setup: AgentSetup | undefined): boolean => {
  const install = setup?.install;
  return install === "paused" || (typeof install === "object" && install.missing !== undefined);
};

/** The document tools' line, from SetupInfo's `python`; null with nothing to say: unavailable, ready, or an older host
 * that says nothing. Their size before the download shows only while a plan still says its own, once for all of them
 * (no plan's size counts it). Waiting with the window `online`, it says it can't reach the download. */
export function toolsView(state: SetupState, online: boolean): ToolsView | null {
  const python = state.python;
  if (python === null || python === "unavailable") return null;
  if (python === "installing")
    return state.pythonMb === undefined ? null : { kind: "installing", mb: state.pythonMb };
  if (python.missing !== undefined) {
    const mb = mbOf(python.missing.download_bytes);
    return mb > 0 && PLANS.some((id) => awaited(state.agents[id])) ? { kind: "missing", mb } : null;
  }
  if (python.downloading !== undefined) {
    const { received, total } = python.downloading;
    const totalMb = mbOf(total);
    return {
      kind: "downloading",
      received,
      total,
      doneMb: Math.min(mbOf(received), totalMb),
      totalMb,
    };
  }
  if (python.waiting !== undefined)
    return online ? { kind: "waiting", online } : { kind: "waiting" };
  if (python.failed !== undefined) {
    const { cause, blocked_by } = python.failed;
    // Windows' app control refused it: a retry can't help
    if (cause === "run_blocked" && (blocked_by === "app_control" || blocked_by === "applocker"))
      return { kind: "blocked" };
    return { kind: "failed" };
  }
  return null;
}

/** A plan whose install is ready, the one the document tools' retry goes through (any ready plan retries them alone,
 * and stays ready); null with none. */
export const readyPlan = (state: SetupState): AgentId | null =>
  PLANS.find((id) => isReady(state.agents[id]?.install)) ?? null;
