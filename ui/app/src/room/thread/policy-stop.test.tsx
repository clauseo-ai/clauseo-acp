// @vitest-environment happy-dom
// A Claude launch a policy refused (agent.user_policy, agent.firm_policy_weakens): the stop line names the setting the
// step carries (StepEntry.failure_setting, from the host's fixed list), isolated in the line's {setting} slot; with
// none, or one this version doesn't know ("unknown"), the unnamed line. The host's detail never reaches the line.
import { POLICY_NO_SETTING } from "@clauseo/components/lib/words-errors-policy.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset, step, upsert, userMessage } from "../../testing/stream-script.ts";
import type { ErrorCode, PolicySetting } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "What notice period does clause five set?");
const NAMED = (setting: string) =>
  `Claude can't start: a setting on this computer (\u2068${setting}\u2069) turns off a protection Clauseo needs, and only an administrator can change it. Whoever looks after this computer can change or remove it; until then, you can use ChatGPT.`;

/** The stop line of a turn that failed with `code`, its step naming `setting`. */
async function line(code: ErrorCode, setting?: PolicySetting): Promise<string | null | undefined> {
  h = await mountRoom();
  const failed = step(30, {
    stop: "failed",
    failure: code,
    ...(setting === undefined ? {} : { failure_setting: setting }),
  });
  h.deliver([reset([ASKED], header("idle")), upsert(failed)]);
  await h.settle();
  const stopped = h.one("turn-stopped");
  expect(stopped?.dataset["failure"]).toBe(code);
  const text = stopped?.textContent;
  await h.unmount();
  h = null;
  document.body.replaceChildren();
  return text;
}

test.each([
  ["agent.user_policy", "disableAllHooks"],
  ["agent.firm_policy_weakens", "sandbox.enabled"],
  ["agent.firm_policy_weakens", "sandbox.network.allowLocalBinding"],
] as const)("%s names the step's setting, %s", async (code, setting) => {
  expect(await line(code, setting)).toBe(NAMED(setting));
});

test.each(["agent.user_policy", "agent.firm_policy_weakens"] as const)(
  "%s with no setting, or one this version doesn't know, says the unnamed line",
  async (code) => {
    expect(await line(code)).toBe(POLICY_NO_SETTING);
    expect(await line(code, "unknown")).toBe(POLICY_NO_SETTING);
  },
);
