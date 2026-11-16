// The copy review entries for a Claude launch the host refused, or a Claude it stopped, over Claude's own settings
// (U3-err-agent.firm_hooks_off, firm_policy_weakens, user_policy and settings_changed, #launch-never-weaker): the firm's managed settings turn hooks off (agent.firm_hooks_off),
// a managed setting outranks Clauseo's and would weaken the launch (agent.firm_policy_weakens), a policy any program can write changed its security settings
// (agent.user_policy), or its settings file changed while it ran (agent.settings_changed). Each is the question and its stop line, never a send's refusal,
// in place of the "U3-err-<code>" entry (registry-offline.tsx's ownEntry). The two policy codes share one line with a {setting} slot
// (U3-err-<code>, shown with a setting from the host's fixed list), and the line with no setting named (U3-err-<code>.unnamed); all
// four stand together, in agent.user_policy's place, so the named and the unnamed lines read side by side.
import { step, userMessage } from "../testing/stream-script.ts";
import type { ErrorCode, PolicySetting } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { errorWords } from "./words.ts";

/** A turn of Claude's that failed with `code`, naming the policy `setting` when given, on Windows, where the hooks and
 * the policy key matter most. */
const stopped = (code: ErrorCode, setting?: PolicySetting) => () => (
  <RoomOver
    host={() =>
      threadHost({
        entries: [
          userMessage(5, 1, "What does clause nine of the lease say about notice?"),
          step(9, {
            stop: "failed",
            failure: code,
            ...(setting === undefined ? {} : { failure_setting: setting }),
          }),
        ],
        platform: "windows",
      })
    }
  />
);

/** A policy code's line, the one naming `setting` (U3-err-<code>, its {setting} slot filled as the room shows it), or
 * with none, the unnamed line (U3-err-<code>.unnamed). */
const policyEntry = (code: ErrorCode, setting?: PolicySetting): Entry => ({
  id: setting === undefined ? `U3-err-${code}.unnamed` : `U3-err-${code}`,
  words: errorWords(
    setting === undefined
      ? "Claude can't start: a setting on this computer turns off a protection Clauseo needs, and only an administrator can change it. Whoever looks after this computer can change or remove it; until then, you can use ChatGPT."
      : "Claude can't start: a setting on this computer ({setting}) turns off a protection Clauseo needs, and only an administrator can change it. Whoever looks after this computer can change or remove it; until then, you can use ChatGPT.",
  ),
  screen: "room/thread",
  render: stopped(code, setting),
});

/** The entries in place of a refusal, by code. */
export const CLAUDE_SETTINGS: Partial<Record<ErrorCode, Entry[]>> = {
  "agent.firm_hooks_off": [
    {
      id: "U3-err-agent.firm_hooks_off",
      words: errorWords(
        "Claude can't start here: your firm's Claude settings on this computer switch off a safety check Clauseo needs (disableAllHooks or allowManagedHooksOnly). Start this conversation with ChatGPT instead.",
      ),
      screen: "room/thread",
      render: stopped("agent.firm_hooks_off"),
    },
  ],
  // shown with agent.user_policy's, below
  "agent.firm_policy_weakens": [],
  "agent.user_policy": [
    policyEntry("agent.user_policy", "disableAllHooks"),
    policyEntry("agent.user_policy"),
    policyEntry("agent.firm_policy_weakens", "sandbox.enabled"),
    policyEntry("agent.firm_policy_weakens"),
  ],
  "agent.settings_changed": [
    {
      id: "U3-err-agent.settings_changed",
      words: errorWords(
        "Claude's settings changed while it was working, so Clauseo stopped it. Send your message again to restart Claude with Clauseo's own settings.",
      ),
      screen: "room/thread",
      render: stopped("agent.settings_changed"),
    },
  ],
};
