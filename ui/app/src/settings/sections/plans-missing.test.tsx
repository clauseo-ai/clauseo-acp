// @vitest-environment happy-dom
// Plans with Claude not here yet (0033, "When an install starts"): the same card as ChatGPT's, "Add Claude" with its
// plan line and size, which installs Claude and then signs in once it is ready; the line then shows the download. A
// plan the firm's policy locks still shows no control.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import {
  CLAUDE,
  CODEX,
  block,
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
afterEach(async () => {
  await unmount();
});

const MISSING = [
  install(CLAUDE, { missing: { download_bytes: 130_000_000 } }),
  signIn(CLAUDE, { signed_out: {} }),
  install(CODEX, { missing: { download_bytes: 165_000_000 } }),
];

test("Claude not here yet: the same card as ChatGPT's, Add Claude with its plan line and size; Add installs it, then signs in once it is ready", async () => {
  const t = await mountPlans({ first: MISSING });
  const claude = block("claude")!;
  const chatgpt = block("chatgpt")!;
  expect(claude.dataset["state"]).toBe("add");
  expect(chatgpt.dataset["state"]).toBe("add");
  expect(claude.textContent).toContain("Uses your Claude plan.");
  expect(claude.textContent).toContain(
    "Needs a download of about 130 MB. Signing in happens in your browser.",
  );
  expect(chatgpt.textContent).toContain("Uses your ChatGPT plan.");
  // mirrored: the same parts in the same order, the names swapped
  const add = claude.querySelector<HTMLElement>('[data-testid="plans-add"]');
  expect(add?.textContent).toBe("Add Claude");
  expect(chatgpt.querySelector('[data-testid="plans-add"]')?.textContent).toBe("Add ChatGPT");
  expect(add?.className).toBe(chatgpt.querySelector('[data-testid="plans-add"]')?.className);
  expect(claude.textContent).not.toContain("ChatGPT");
  await press(add);
  expect(t.setups()).toEqual([{ install_agent: { agent: CLAUDE } }]);
  await play(t, install(CLAUDE, { downloading: { received: 10_000_000, total: 130_000_000 } }));
  expect(block("claude")!.textContent).toContain("Getting Claude ready… 10 MB of 130 MB");
  expect(block("claude")!.querySelector('[data-testid="plans-add"]')).toBeNull();
  await play(t, install(CLAUDE, { ready: { version: "0.1.0" } }));
  expect(t.setups()).toEqual([
    { install_agent: { agent: CLAUDE } },
    { start_sign_in: { agent: CLAUDE } },
  ]);
});

test("Claude not here yet under the firm's lock: Managed by firm and no control", async () => {
  const t = await mountPlans({ first: MISSING, locked: ["plans.claude-acp"] });
  const claude = block("claude")!;
  expect(claude.dataset["state"]).toBe("locked");
  expect(claude.querySelectorAll("button, input")).toHaveLength(0);
  expect(t.setups()).toEqual([]);
});
