// The set-up view in the composer (first-run.tsx's viewFor, gate.tsx's "set-up"): the plan a conversation runs, not
// set up on this computer while the other is, says so in one line with one act where the question goes. On a new
// conversation the other plan is a quiet way round it, which starts no download it doesn't need; a sign-in the agent
// refused as expired says "Sign in again"; a conversation fixed to its plan offers no other. While the gate stands the
// model and effort wait with it.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import type { ConfigOption } from "../../transport/protocol.ts";
import {
  A,
  AT,
  CLAUDE,
  CODEX,
  MADE_THREAD,
  READY,
  SIGNED_IN,
  button,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  signIn,
  text,
} from "./rig.tsx";

const planButton = (name: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;

const MODEL = {
  id: "model",
  name: "Model",
  category: "model",
  kind: {
    select: {
      current_value: "default",
      options: [
        { value: "default", name: "Default model" },
        { value: "other", name: "Other model" },
      ],
    },
  },
} as ConfigOption;

const CLAUDE_ONLY = [install(CLAUDE, READY), install(CODEX, READY), signIn(CLAUDE, SIGNED_IN)];

test("ChatGPT chosen but not set up: one line, Set up ChatGPT, and Use Claude instead, which downloads nothing", async () => {
  const t = await mountFirstRun({
    options: [MODEL],
    first: [...CLAUDE_ONLY, signIn(CODEX, { signed_out: {} })],
  });
  // Claude signed in: the field, the model menu
  expect(byId("signin")).toBeNull();
  expect(byId("pick-model")).not.toBeNull();
  await press(planButton("ChatGPT"));
  expect(byId("signin-state")?.dataset["state"]).toBe("set-up");
  expect(text()).toContain("ChatGPT isn't set up on this computer.");
  expect(byId("signin-set-up")?.textContent).toBe("Set up ChatGPT");
  // the gate stands where the question goes: the model and effort wait with it
  expect(byId("pick-model")).toBeNull();
  await press(button("Use Claude instead"));
  expect(planButton("Claude")?.getAttribute("aria-pressed")).toBe("true");
  expect(byId("signin")).toBeNull();
  expect(byId("pick-model")).not.toBeNull();
  // Claude is ready: no install_agent for it
  expect(t.setups().filter((c) => "install_agent" in c)).toEqual([]);
});

test("an expired ChatGPT sign-in says so, and its one act is Sign in again", async () => {
  const t = await mountFirstRun({
    first: [...CLAUDE_ONLY, signIn(CODEX, { failed: { code: "agent.auth_required", at: AT } })],
  });
  await press(planButton("ChatGPT"));
  expect(text()).toContain("Your ChatGPT sign-in has expired.");
  expect(byId("signin-set-up")?.textContent).toBe("Sign in again");
  await press(byId("signin-set-up"));
  expect(t.setups()).toContainEqual({ start_sign_in: { agent: CODEX } });
  // signed in again: the field is back, on ChatGPT
  await play(t, signIn(CODEX, SIGNED_IN));
  expect(byId("signin-set-up")).toBeNull();
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
});

test("a conversation fixed to ChatGPT, ChatGPT gone from this computer: Set up ChatGPT, and no other plan offered", async () => {
  await mountFirstRun({
    threads: [{ thread: MADE_THREAD, matter: A.matter, agent: CODEX, status: "idle" }],
    first: [...CLAUDE_ONLY, signIn(CODEX, { signed_out: {} })],
  });
  expect(planButton("Claude")).toBeNull();
  expect(byId("signin-set-up")?.textContent).toBe("Set up ChatGPT");
  expect(button("Use Claude instead")).toBeNull();
});

test("neither plan signed in on a new conversation: both stand side by side, as on the first run", async () => {
  await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      install(CODEX, READY),
      signIn(CLAUDE, { signed_out: {} }),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  expect(byId("signin-set-up")).toBeNull();
  expect(text()).toContain("Sign in to Claude or ChatGPT to ask about this matter");
});
