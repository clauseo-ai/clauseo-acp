// @vitest-environment happy-dom
// Plans' note on the settings the Safe workspace leaves out (SettingsView.safe_workspace_unused_env): nothing for a
// plan with no names or with the field missing; the plan's line, by its name (ChatGPT, never Codex), and the names
// alone, in code type, for one with names; "Sign in through Clauseo instead." only while the plan isn't signed in
// through Clauseo, and never for a plan the firm's policy holds.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { SignIn } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  block,
  install,
  mountPlans,
  signIn,
  signed,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(unmount);

const both = (claude: SignIn, codex: SignIn = { signed_out: {} }) => [
  install(CLAUDE, READY),
  signIn(CLAUDE, claude),
  install(CODEX, READY),
  signIn(CODEX, codex),
];
const note = (plan: "claude" | "chatgpt") =>
  block(plan)?.querySelector<HTMLElement>('[data-testid="plans-unused-env"]') ?? null;
const SIGN_IN = "Sign in through Clauseo instead.";

test("no note while the host lists no names: the field missing, a plan left out, or an empty list", async () => {
  await mountPlans({ first: both(signed()) });
  expect(note("claude")).toBeNull();
  expect(note("chatgpt")).toBeNull();
  await unmount();
  await mountPlans({ first: both(signed()), unusedEnv: { [CLAUDE]: [] } });
  expect(note("claude")).toBeNull();
  expect(note("chatgpt")).toBeNull();
  expect(document.body.textContent).not.toContain("aren't used");
});

test("a plan with names: its line by the plan's name, and the names alone in code type, joined", async () => {
  await mountPlans({
    first: both(signed(), signed({ plan: "ChatGPT Plus" })),
    unusedEnv: { [CODEX]: ["OPENAI_API_KEY", "OPENAI_BASE_URL"] },
  });
  expect(note("claude")).toBeNull();
  const shown = note("chatgpt");
  expect(shown?.textContent).toContain(
    "Some of ChatGPT's settings aren't used in the Safe workspace.",
  );
  expect(shown?.textContent).not.toContain("Codex");
  const names = shown?.querySelector("code");
  expect(names?.textContent).toBe("OPENAI_API_KEY, OPENAI_BASE_URL");
  expect(names?.dataset["testid"]).toBe("plans-unused-names");
  expect(shown?.querySelector(".set-plan-fine")?.textContent).toBe(
    "Not used: OPENAI_API_KEY, OPENAI_BASE_URL",
  );
  // the names, and nothing that stands for a value
  expect(shown?.textContent).not.toMatch(/=|sk-|\*\*\*/);
});

test("Sign in through Clauseo shows while the plan isn't signed in through Clauseo", async () => {
  const unusedEnv = { [CLAUDE]: ["ANTHROPIC_API_KEY"] };
  await mountPlans({ first: both(signed()), unusedEnv });
  expect(note("claude")?.textContent).not.toContain(SIGN_IN);
  await unmount();
  await mountPlans({ first: both(signed({ source: "reused" })), unusedEnv });
  expect(note("claude")?.textContent).toContain(SIGN_IN);
  await unmount();
  await mountPlans({ first: both({ signed_out: {} }), unusedEnv });
  expect(note("claude")?.textContent).toContain(SIGN_IN);
  await unmount();
  await mountPlans({ first: both({ signed_out: {} }), unusedEnv, locked: ["plans.claude-acp"] });
  expect(note("claude")).not.toBeNull();
  expect(note("claude")?.textContent).not.toContain(SIGN_IN);
});
