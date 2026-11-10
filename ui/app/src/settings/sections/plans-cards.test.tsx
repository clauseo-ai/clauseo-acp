// @vitest-environment happy-dom
// Plans' cards (UD2, D-002 and D-005): Claude's and ChatGPT's cards have the same parts in the same order, whatever
// holds the plan back (not added, waiting for the other download, signed out, a cancelled sign-in), the main button
// first in the acts row with the same kind. A cancelled sign-in has no key link above the key panel, offers the other
// plan only when it can be used here, and each key panel's help names its own plan.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
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
  signed,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await unmount();
});

const CANCELLED: SignIn = { failed: { code: "signin.cancelled", at: AT } };
const both = (claude: SignIn, codex: SignIn) => [
  install(CLAUDE, READY),
  signIn(CLAUDE, claude),
  install(CODEX, READY),
  signIn(CODEX, codex),
];

/** A block's parts in order: each child's tag and part (or class), and the acts row's buttons by kind. */
const shape = (plan: "claude" | "chatgpt") => {
  const el = block(plan)!;
  const parts = [...el.children].map(
    (c) => `${c.tagName}.${(c as HTMLElement).dataset["part"] ?? c.className}`,
  );
  const acts = [...el.querySelectorAll('[data-part="acts"] > button')].map(
    (b) => `${(b as HTMLElement).dataset["part"] ?? "other"}:${b.className}`,
  );
  return { parts, acts };
};

const KEY_LINK = "Use an API key or my firm's account";

test.each([
  [
    "both not added",
    [
      install(CLAUDE, { missing: { download_bytes: 130_000_000 } }),
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
    ],
  ],
  [
    "Claude waiting for ChatGPT's download, ChatGPT not added",
    [install(CLAUDE, "paused"), install(CODEX, { missing: { download_bytes: 165_000_000 } })],
  ],
  ["both signed out", both({ signed_out: {} }, { signed_out: {} })],
  ["both cancelled", both(CANCELLED, CANCELLED)],
] as const)(
  "%s: the same card for each plan, the same parts in the same order",
  async (_, first) => {
    await mountPlans({ first: [...first] });
    const claude = shape("claude");
    expect(claude.parts).toContain("P.plan");
    expect(claude.parts).toContain("DIV.acts");
    expect(claude.acts[0]).toMatch(/^main:/);
    expect(shape("chatgpt")).toEqual(claude);
    expect(block("claude")!.textContent).toContain("Uses your Claude plan.");
    expect(block("chatgpt")!.textContent).toContain("Uses your ChatGPT plan.");
  },
);

test("Claude waiting for the other download: the card's Add Claude, with its size, which queues the sign-in", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, "paused"),
      install(CODEX, { downloading: { received: 1, total: 165_000_000 } }),
    ],
  });
  const add = block("claude")!.querySelector<HTMLElement>('[data-testid="plans-add"]');
  expect(add?.textContent).toBe("Add Claude");
  await press(add);
  expect(t.setups()).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("a cancelled sign-in: no key link above the key panel, which shows once, under Other ways in", async () => {
  await mountPlans({ first: both({ signed_out: {} }, CANCELLED) });
  const chatgpt = block("chatgpt")!;
  expect(chatgpt.textContent).toContain("Sign-in was cancelled.");
  expect(chatgpt.textContent).not.toContain(KEY_LINK);
  expect(chatgpt.querySelector('[aria-label="Other ways in"]')).not.toBeNull();
  expect(chatgpt.querySelectorAll('[data-testid="plans-key"]')).toHaveLength(1);
});

test("a cancelled sign-in offers Use Claude instead only where Claude is here: not added, or held by the firm, no", async () => {
  await mountPlans({
    first: [
      install(CLAUDE, { missing: { download_bytes: 130_000_000 } }),
      install(CODEX, READY),
      signIn(CODEX, CANCELLED),
    ],
  });
  expect(block("chatgpt")!.textContent).not.toContain("Use Claude instead");
  expect(block("chatgpt")!.querySelector('[data-testid="plans-instead"]')).toBeNull();
  await unmount();
  await mountPlans({ first: both({ signed_out: {} }, CANCELLED), locked: ["plans.claude-acp"] });
  expect(block("chatgpt")!.textContent).not.toContain("Use Claude instead");
});

test("a cancelled sign-in with the other plan here: Use Claude instead moves to Claude's card and sends nothing", async () => {
  const t = await mountPlans({ first: both({ signed_out: {} }, CANCELLED) });
  const instead = button("Use Claude instead", block("chatgpt")!);
  expect(instead).not.toBeNull();
  await press(instead);
  expect(block("claude")!.contains(document.activeElement)).toBe(true);
  expect(t.setups()).toEqual([]);
  await press(button("Sign in", block("chatgpt")!));
  expect(t.setups()).toEqual([{ start_sign_in: { agent: CODEX } }]);
});

test("each key panel's help names its own plan", async () => {
  await mountPlans({ first: both({ signed_out: {} }, CANCELLED) });
  expect(block("claude")!.textContent).toContain(
    "A key from Claude, yours or your firm's. Billed to that key's account.",
  );
  expect(block("claude")!.textContent).not.toContain("ChatGPT");
  expect(block("chatgpt")!.textContent).toContain(
    "A key from ChatGPT, yours or your firm's. Billed to that key's account.",
  );
  expect(block("chatgpt")!.textContent).not.toContain("Claude or ChatGPT");
});

test("D-066: never signed in, each card's Sign in, never again; after a real sign-in, Sign in again", async () => {
  // a search that found nothing, on a computer neither plan was signed in on
  const t = await mountPlans({
    first: both({ signed_out: { found: "none_found" } }, { signed_out: { found: "none_found" } }),
  });
  for (const plan of ["claude", "chatgpt"] as const) {
    expect(button("Sign in", block(plan)!)).not.toBeNull();
    expect(block(plan)!.textContent).not.toContain("again");
  }
  // signed in, then signed out (here or elsewhere): again
  await play(t, signIn(CLAUDE, signed()), signIn(CODEX, signed({ plan: "ChatGPT Plus" })));
  await play(t, signIn(CLAUDE, { signed_out: {} }), signIn(CODEX, { signed_out: {} }));
  for (const plan of ["claude", "chatgpt"] as const) {
    expect(button("Sign in again", block(plan)!)).not.toBeNull();
    expect(button("Sign in", block(plan)!)).toBeNull();
  }
});

test("D-066: the agent's refusal for a sign-in that lapsed (agent.auth_required) says Sign in again", async () => {
  const lapsed: SignIn = { failed: { code: "agent.auth_required", at: AT } };
  await mountPlans({ first: both(lapsed, lapsed) });
  expect(button("Sign in again", block("claude")!)).not.toBeNull();
  expect(button("Sign in again", block("chatgpt")!)).not.toBeNull();
});
