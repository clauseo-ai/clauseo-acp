// @vitest-environment happy-dom
// Plans on the API key's path, from the Windows key test (keypath-proof-vm/results.md): the key is offered whether or
// not the plan is added yet, and a key saved for a plan not here starts its download (install_agent); a key the
// provider refused (the host's signed_in.refused, after a question failed) says so in place of "✓ Using an API key";
// and a cancelled sign-in gives way to the key as soon as the host says the key is in use.
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
  typeInto,
  unmount,
} from "./plans-rig.tsx";

vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();

beforeEach(() => localStorage.clear());
afterEach(async () => {
  await unmount();
});

const KEY: SignIn = { signed_in: { method: "api_key", source: "own", at: AT } };
/** The host's state after the provider answered 401 to the key (#984's `refused`). */
const REFUSED: SignIn = {
  signed_in: { method: "api_key", source: "own", at: AT, refused: AT + 60_000 },
};

test("defect 2: ChatGPT not added: its card offers the key, and a key saved there starts ChatGPT's download", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { signed_in: { method: "plan", source: "own", at: AT } }),
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
    ],
  });
  const chatgpt = block("chatgpt")!;
  expect(chatgpt.textContent).toContain("Add ChatGPT");
  await press(button("Use an API key instead", chatgpt));
  const field = chatgpt.querySelector<HTMLInputElement>('input[type="password"]');
  expect(field?.getAttribute("aria-label")).toBe("OpenAI API key");
  await typeInto(field!, "sk-proj-clauseo-test-invalid");
  await press(button("Use this key", chatgpt));
  expect(t.setups()).toEqual([
    { set_agent_key: { agent: CODEX, key: "sk-proj-clauseo-test-invalid" } },
    { install_agent: { agent: CODEX } },
  ]);
});

test("defect 2: a key saved after a cancelled sign-in: the card leaves Sign-in was cancelled once the host says the key is in use", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { failed: { code: "signin.cancelled", at: AT } }),
    ],
  });
  const chatgpt = () => block("chatgpt")!;
  expect(chatgpt().textContent).toContain("Sign-in was cancelled.");
  await press(button("Use an API key instead", chatgpt()));
  const field = chatgpt().querySelector<HTMLInputElement>('input[type="password"]');
  await typeInto(field!, "sk-proj-clauseo-test-invalid");
  await press(button("Use this key", chatgpt()));
  // ChatGPT is installed: the key alone, no download
  expect(t.setups()).toEqual([
    { set_agent_key: { agent: CODEX, key: "sk-proj-clauseo-test-invalid" } },
  ]);
  // the host's part (#984): the key's line replaces the cancelled sign-in at once
  await play(t, signIn(CODEX, KEY));
  expect(chatgpt().textContent).not.toContain("Sign-in was cancelled.");
  expect(chatgpt().textContent).toContain("Using an API key, billed to its own account");
});

test("defect 4: after a 401 the plan says the key wasn't accepted, never ✓ Using an API key; a new key clears it", async () => {
  const t = await mountPlans({
    first: [install(CLAUDE, READY), signIn(CLAUDE, KEY), install(CODEX, READY), signIn(CODEX, KEY)],
  });
  expect(block("chatgpt")!.textContent).toContain("Using an API key, billed to its own account");
  await play(t, signIn(CODEX, REFUSED), signIn(CLAUDE, REFUSED));
  for (const [plan, name] of [
    ["chatgpt", "ChatGPT"],
    ["claude", "Claude"],
  ] as const) {
    const el = block(plan)!;
    expect(el.querySelector('[data-testid="plans-key-refused"]')?.textContent).toBe(
      `${name} didn't accept the API key.`,
    );
    expect(el.textContent).not.toContain("Using an API key");
    // the key's own controls stay: change it or remove it
    expect(button("Change key", el)).not.toBeNull();
    expect(button("Remove key", el)).not.toBeNull();
  }
  await play(t, signIn(CODEX, KEY));
  expect(block("chatgpt")!.querySelector('[data-testid="plans-key-refused"]')).toBeNull();
  expect(block("chatgpt")!.textContent).toContain("Using an API key, billed to its own account");
});
