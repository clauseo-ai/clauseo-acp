// The setup store's state as the first run's parts show it: the install as the working line says it, and the sign-in
// as the gate says it. Pure, so each state's screen is tested from the host's words alone.
import type { GateRefusal, GateView } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import type { InstallView } from "@clauseo/components/clauseo/first-launch/install.tsx";
import type { ToolsView } from "@clauseo/components/clauseo/first-launch/tools.tsx";
import type { AgentId } from "../../transport/protocol.ts";
import {
  PLANS,
  isReady,
  isSignedIn,
  mbOf,
  type AgentSetup,
  type SetupState,
} from "../../store/setup.ts";

/** A download's size as the line says it, "about": the nearest 10 MB from 100 MB up,
 * whole MB below. 0 stays 0. */
export const aboutMb = (mb: number): number => (mb >= 100 ? Math.round(mb / 10) * 10 : mb);

/** The install as the working line shows it, from the host's InstallProgress; null with nothing to say. */
export function installView(setup: AgentSetup | undefined): InstallView | null {
  const install = setup?.install;
  if (install === undefined || install === "managed") return null;
  if (install === "installing") return { kind: "installing" };
  // paused: another plan's download goes first; it says where it stands, still
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
  if ("waiting" in install && install.waiting !== undefined) return { kind: "offline" };
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

/** The gate for a plan's sign-in, or null when it is signed in or nothing is known of it (a host before R20). */
export function gateView(
  setup: AgentSetup | undefined,
  state: SetupState,
  clock: (at: number) => string,
): GateView | null {
  const signIn = setup?.signIn;
  if (signIn === undefined || isSignedIn(signIn)) return null;
  if (state.keyOpen) return { kind: "key" };
  if (signIn === "checking") return { kind: "checking" };
  if (signIn === "queued") {
    const install = setup?.install;
    const offline =
      typeof install === "object" && "waiting" in install && install.waiting !== undefined;
    return { kind: "queued", offline };
  }
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

/** The last setup command desktop refused for the plan, as the gate's line says it; null with none. A
 * `command.conflict` while the plan is switching accounts says only that the switch isn't done. */
export function refusalView(setup: AgentSetup | undefined): GateRefusal | null {
  if (setup?.refused === undefined) return null;
  const signIn = setup.signIn;
  const switching = typeof signIn === "object" && signIn.switching !== undefined;
  return { code: setup.refused, switching };
}

/** Whether a plan isn't here yet and says its size: missing, or paused behind the other plan's download. */
const awaited = (setup: AgentSetup | undefined): boolean => {
  const install = setup?.install;
  return install === "paused" || (typeof install === "object" && install.missing !== undefined);
};

/** The document tools' line, from SetupInfo's `python`; null with nothing to say: unavailable, ready, or an older host
 * that says nothing. Their size before the download shows only while a plan still says its own, once for all of them
 * (no plan's size counts it). */
export function toolsView(state: SetupState): ToolsView | null {
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
  if (python.waiting !== undefined) return { kind: "waiting" };
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
