// @vitest-environment happy-dom
// Plans (UD2) for a saved API key the keychain didn't give (decision record 0048): agent.key_unreadable when its read
// had not answered in time, agent.key_denied when the keychain refused it (macOS's Deny). The card says so under
// "Uses your Anthropic API key." (never "Uses your Claude plan."), in each code's own words, with Try again, and offers
// Remove key: desktop deletes the key with no read, the way out when the keychain keeps refusing. Kept apart from
// plans.test.tsx for its 600-line limit.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { stubFonts } from "../../room/thread/testing.ts";
import type { SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  block,
  button,
  install,
  mountPlans,
  play,
  press,
  signIn,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(unmount);

const TIMED_OUT = "Clauseo couldn't check for a saved API key in time.";
const DENIED =
  "Your Mac's keychain didn't let Clauseo read, save or remove the API key. Press Allow on Your keychain, then choose Always Allow.";
const failed = (code: string): SignIn => ({ failed: { code: code as never, at: AT } });
const at = (claude: SignIn, codex: SignIn = { signed_out: {} }) =>
  mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, claude),
      install(CODEX, READY),
      signIn(CODEX, codex),
    ],
  });
const words = (plan: "claude" | "chatgpt" = "claude") => block(plan)?.textContent ?? "";
const line = (plan: "claude" | "chatgpt" = "claude") =>
  block(plan)?.querySelector('[data-part="plan"]')?.textContent ?? "";

test("the words-errors line for agent.key_denied is the app track's provisional line, not the timeout's", () => {
  expect(errorWords("en", "agent.key_denied")).toBe(DENIED);
  expect(errorWords("en", "agent.key_unreadable")).toContain(TIMED_OUT);
});

test.each([
  ["agent.key_unreadable", TIMED_OUT, DENIED],
  ["agent.key_denied", DENIED, TIMED_OUT],
])(
  "%s: Uses your Anthropic API key., that code's own words, Try again and Remove key; never the plan's line",
  async (code, own, other) => {
    const t = await at(failed(code));
    const claude = block("claude")!;
    expect(claude.dataset["state"]).toBe("failed");
    expect(line()).toBe("Uses your Anthropic API key.");
    expect(words()).not.toContain("Uses your Claude plan.");
    expect(words()).toContain(own);
    expect(words()).not.toContain(other);
    expect(button("Try again", claude)).not.toBeNull();
    expect(button("Sign in again", claude)).toBeNull();
    // the key can't be read, so no Change key, and no second key offered over it: Remove key alone
    expect(claude.querySelector('[data-testid="plans-key-remove"]')?.textContent).toBe(
      "Remove key",
    );
    expect(button("Change key", claude)).toBeNull();
    expect(button("Use an API key instead", claude)).toBeNull();
    // Try again reads the key again (desktop refuses the start with the code); no command but that one
    await press(button("Try again", claude));
    expect(t.setups()).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  },
);

test("Remove key under an unreadable key: after its confirm, clear_agent_key, then signed out with Sign in offered", async () => {
  const t = await at(failed("agent.key_denied"));
  await press(block("claude")!.querySelector('[data-testid="plans-key-remove"]'));
  expect(document.body.textContent).toContain("Remove the API key?");
  await press(document.querySelector('[data-testid="plans-key-remove-cancel"]'));
  expect(t.setups()).toEqual([]);
  await press(block("claude")!.querySelector('[data-testid="plans-key-remove"]'));
  await press(document.querySelector('[data-testid="plans-key-remove-confirm"]'));
  expect(t.setups()).toEqual([{ clear_agent_key: { agent: CLAUDE } }]);
  // desktop deletes the key with no read, ends the mark, and the agent is plain "no key"
  await play(t, signIn(CLAUDE, { signed_out: {} }));
  const claude = block("claude")!;
  expect(claude.dataset["state"]).toBe("signed-out");
  expect(line()).toBe("Uses your Claude plan.");
  expect(words()).not.toContain(DENIED);
  expect(claude.querySelector('[data-testid="plans-key-remove"]')).toBeNull();
  expect(button("Use an API key instead", claude)).not.toBeNull();
  const sign = claude.querySelector<HTMLElement>('[data-testid="plans-sign-in"]');
  expect(sign?.textContent).toBe("Sign in");
  await press(sign);
  expect(t.setups()).toEqual([
    { clear_agent_key: { agent: CLAUDE } },
    { start_sign_in: { agent: CLAUDE } },
  ]);
});

test("ChatGPT's card in the same state: Uses your OpenAI API key., the code's words and Remove key", async () => {
  await at({ signed_out: {} }, failed("agent.key_denied"));
  const chatgpt = block("chatgpt")!;
  expect(line("chatgpt")).toBe("Uses your OpenAI API key.");
  expect(words("chatgpt")).not.toContain("Uses your ChatGPT plan.");
  expect(words("chatgpt")).toContain(DENIED);
  expect(chatgpt.querySelector('[data-testid="plans-key-remove"]')).not.toBeNull();
  // Claude's card, signed out, keeps its plan line
  expect(line()).toBe("Uses your Claude plan.");
});

// stage E: while the window's one permissions read lists the keychain row as would_ask, the card points at the
// keychain row's Allow in place of the key's words and Try again; granted, today's words
test("keychain would_ask: the keychain line and Allow, which asks for it, never Try again; granted, today's words", async () => {
  const KEYS = {
    id: "keychain",
    state: "would_ask",
    folder: null,
    settings_path: null,
    note: null,
  } as const;
  const ask = vi.fn(async () => ({ ...KEYS, state: "granted" as const }));
  await mountPlans({
    first: [install(CLAUDE, READY), signIn(CLAUDE, failed("agent.key_unreadable"))],
    permissions: { list: async () => [KEYS], ask },
  });
  expect(words()).toContain("Clauseo needs your Mac's permission to read its saved keys.");
  expect(words()).not.toContain(TIMED_OUT);
  expect(block("claude")?.querySelector('[data-testid="plans-sign-in"]')).toBeNull();
  await press(
    block("claude")?.querySelector<HTMLElement>('[data-testid="keychain-allow"]') ?? null,
  );
  expect(ask).toHaveBeenCalledWith("keychain");
  expect(words()).toContain(TIMED_OUT);
  expect(block("claude")?.querySelector('[data-testid="plans-sign-in"]')?.textContent).toBe(
    "Try again",
  );
});
