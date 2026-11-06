// #494's e2e hang, pinned as product behaviour: a sign-in that turns signed_out takes the composer's Send away even
// after a turn has run and failed (agent.exited), because the gate is for the plan, not the conversation. The
// Windows e2e app hit this when desktop's first setup events came after its first question; it now starts its
// agents managed and signed in by the firm (ui/e2e/app-windows/scratch.ts), the second test's state. Don't "fix"
// the first test by keeping Send under a gate: a signed-out plan sends nothing (composer-gate.test.tsx).
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test } from "vitest";
import type { SignIn } from "../../transport/bindings.ts";
import {
  agentText,
  batch,
  event,
  reset,
  step,
  userMessage,
  uuid,
} from "../../testing/stream-script.ts";
import {
  A,
  AT,
  CLAUDE,
  MADE_THREAD,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  sent,
  signIn,
  typeInto,
} from "./rig.tsx";
import { stubFonts } from "../thread/testing.ts";

stubFonts();

const QUESTION = "Summarise the indemnity clause";
const field = () => byId<HTMLTextAreaElement>("composer") as HTMLTextAreaElement;
const MISSING = { missing: { download_bytes: 120_000_000 } };
const FIRM: SignIn = {
  signed_in: { method: "firm", source: "own", at: AT },
};

/** Asks the question, then ends its turn failed with agent.exited after one chunk, as fake_agent's
 * claude-exit-mid-turn does. */
async function askAndExit(t: Awaited<ReturnType<typeof mountFirstRun>>) {
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread")).toHaveLength(1);
  const header = {
    header: {
      order: 1,
      thread: MADE_THREAD,
      matter: A.matter,
      agent: CLAUDE,
      status: "idle" as const,
      config_options: [],
      commands: [],
      plan: [],
    },
  };
  const entries = [
    userMessage(5, 1, QUESTION),
    agentText(6, 1, "Starting on the answer."),
    step(7, { stop: "failed", failure: "agent.exited" }),
  ];
  const stream = `thread:${MADE_THREAD}` as const;
  await act(async () =>
    t.deliver(JSON.stringify(event(batch(1, [reset(entries, header)], uuid(1, 3), stream)))),
  );
  expect(byId("turn-stopped")?.dataset["failure"]).toBe("agent.exited");
}

test("after a turn fails with agent.exited, a signed_out sign-in takes Send away and puts the gate up", async () => {
  // no setup word yet: nothing held back, so the first question goes
  const t = await mountFirstRun({ first: [] });
  await askAndExit(t);
  expect(byId("send")).not.toBeNull();
  // desktop's first setup events arrive: the install is missing and the plan signed out
  await play(t, install(CLAUDE, MISSING), signIn(CLAUDE, { signed_out: {} }));
  expect(byId("send")).toBeNull();
  expect(byId("signin")).not.toBeNull();
  expect(field().hidden).toBe(true);
});

test("a plan the firm signs in from the first word, with a managed install, keeps Send after a failed turn", async () => {
  const t = await mountFirstRun({ first: [install(CLAUDE, "managed"), signIn(CLAUDE, FIRM)] });
  await askAndExit(t);
  expect(byId("signin")).toBeNull();
  expect(byId("send")).not.toBeNull();
});
