// The line for a Claude launch a policy refused (agent.user_policy, agent.firm_policy_weakens, words-errors.ts): the
// setting the step names, or none. Apart from words-errors.ts to keep it under the size limit; the Hindi is in
// words-errors.hi.ts with the rest.
import type { ErrorKey } from "./words-errors.ts";

/** The Claude Code settings a policy refusal can name: the protocol's PolicySetting (ui/app/src/transport/bindings.ts),
 * the host's fixed list as dotted keys, copied, since the parts can't import the app. ui/app/src/room/thread/
 * error-codes.test.ts fails when the two differ. The line names nothing else, never text from a policy file. */
export const POLICY_SETTINGS = [
  "disableAllHooks",
  "allowManagedHooksOnly",
  "allowManagedPermissionRulesOnly",
  "skipDangerousModePermissionPrompt",
  "disableBundledSkills",
  "permissions.defaultMode",
  "permissions.disableBypassPermissionsMode",
  "sandbox.enabled",
  "sandbox.autoAllowBashIfSandboxed",
  "sandbox.allowUnsandboxedCommands",
  "sandbox.failIfUnavailable",
  "sandbox.allowAppleEvents",
  "sandbox.enableWeakerNestedSandbox",
  "sandbox.enableWeakerNetworkIsolation",
  "sandbox.network.allowLocalBinding",
  "sandbox.network.allowAllUnixSockets",
  "sandbox.excludedCommands",
  "includeGitInstructions",
  "crossSessionInbound",
  "disableRemoteControl",
  "tools",
  "env.PATH",
] as const;

/** The protocol's PolicySetting: a setting of POLICY_SETTINGS, or "unknown", one a newer Clauseo names. */
export type PolicySetting = (typeof POLICY_SETTINGS)[number] | "unknown";

/** Provisional (app words, security review): a managed Claude Code setting on this computer turns off a protection Clauseo
 * needs, for agent.user_policy and agent.firm_policy_weakens. `{setting}` is the setting the step names
 * (StepEntry.failure_setting), isolated (errorWords); the line below names none. */
export const POLICY_SETTING =
  "Claude can't start: a setting on this computer ({setting}) turns off a protection Clauseo needs, and only an administrator can change it. Whoever looks after this computer can change or remove it; until then, you can use ChatGPT." satisfies ErrorKey;

/** POLICY_SETTING's line with no setting named (the step names none, or one this version doesn't know):
 * U3-err-agent.user_policy.unnamed and U3-err-agent.firm_policy_weakens.unnamed. */
export const POLICY_NO_SETTING =
  "Claude can't start: a setting on this computer turns off a protection Clauseo needs, and only an administrator can change it. Whoever looks after this computer can change or remove it; until then, you can use ChatGPT." satisfies ErrorKey;

/** `setting` when it is one of POLICY_SETTINGS, else null: none, "unknown", or anything else. */
export function policySetting(
  setting: string | null | undefined,
): Exclude<PolicySetting, "unknown"> | null {
  return (POLICY_SETTINGS as readonly string[]).includes(setting ?? "")
    ? (setting as Exclude<PolicySetting, "unknown">)
    : null;
}
