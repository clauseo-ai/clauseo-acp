// Each plan's name as the gate says it, the other plan, each plan's CLI, and the line for a login found but not
// reusable: shared by gate.tsx, gate-failed.tsx and the settings window's Plans (UD2).
import type { FirstLaunchKey } from "../../lib/words-first-launch.ts";
import type { Found, Plan } from "./gate.tsx";

export const NAME: Record<Plan, string> = { claude: "Claude", chatgpt: "ChatGPT" };
export const other = (p: Plan): Plan => (p === "claude" ? "chatgpt" : "claude");

/** Each plan's CLI, whose login Clauseo may reuse. */
export const APP: Record<Plan, string> = { claude: "Claude Code", chatgpt: "Codex" };

/** The line for a login that can't be shared, or reuse that stopped. */
export const FOUND: Record<Found, FirstLaunchKey> = {
  claude_settings:
    "Claude Code on this computer signs in through your own Claude settings, which Clauseo keeps out of your matters.",
  codex_keyring: "Codex on this computer keeps its sign-in where Clauseo can't share it.",
  claude_windows_sandbox:
    "Clauseo's safe workspace can't share Claude Code's sign-in on Windows yet.",
  reused_signed_out: "{app} on this computer is signed out, so Clauseo stopped using its sign-in.",
  reused_changed:
    "{app} on this computer now uses another account, so Clauseo stopped using its sign-in.",
  codex_link_split: "Codex's sign-in on this computer changed, so Clauseo stopped using it.",
  reused_failed: "{app}'s sign-in stopped working here, so Clauseo stopped using it.",
};
