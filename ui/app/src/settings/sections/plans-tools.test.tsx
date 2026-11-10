// @vitest-environment happy-dom
// Plans (UD2) with Clauseo's document tools: one quiet line under the plan cards, as the first screen has it. The size
// once under the cards while a plan is still to add; a failure's Try again through a ready plan, and none with none
// ready; nothing from an older host. A failure leaves every card as it was.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { PythonProgress, SetupEvent } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  block,
  install,
  mountPlans,
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

const tools = (python: PythonProgress): SetupEvent => ({
  setup_info: { keys_available: true, python },
});
const line = () => document.querySelector<HTMLElement>('[data-testid="plans-tools"]');
const CLAUDE_SIGNED = [install(CLAUDE, READY), signIn(CLAUDE, signed())];

test("an older host: no line under the cards", async () => {
  await mountPlans({
    first: [...CLAUDE_SIGNED, install(CODEX, { missing: { download_bytes: 165_000_000 } })],
  });
  expect(line()).toBeNull();
});

test("missing with ChatGPT still to add: the size once under the cards, ChatGPT's card keeping its own", async () => {
  await mountPlans({
    first: [
      ...CLAUDE_SIGNED,
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
      tools({ missing: { download_bytes: 106_000_000 } }),
    ],
  });
  expect(document.querySelectorAll('[data-testid="plans-tools"]')).toHaveLength(1);
  expect(line()?.textContent).toBe("Also downloads Clauseo's document tools (106 MB).");
  expect(block("chatgpt")!.textContent).toContain("Needs a download of about 170 MB.");
  expect(block("chatgpt")!.textContent).not.toContain("document tools");
});

test("failed: Try again sends RetryAgentInstall with the ready plan's id, and Claude's card stays signed in", async () => {
  const t = await mountPlans({
    first: [
      ...CLAUDE_SIGNED,
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
      tools({ failed: { cause: "disk_full", code: "agent.install_disk_full", log: [] } }),
    ],
  });
  expect(line()?.textContent).toContain("Clauseo couldn't download its document tools.");
  expect(block("claude")!.dataset["state"]).toBe("signed");
  await press(document.querySelector<HTMLElement>('[data-testid="plans-tools-retry"]'));
  expect(t.setups()).toEqual([{ retry_agent_install: { agent: CLAUDE } }]);
});

test("failed with no plan ready: no Try again", async () => {
  await mountPlans({
    first: [
      install(CLAUDE, { missing: { download_bytes: 130_000_000 } }),
      install(CODEX, { missing: { download_bytes: 165_000_000 } }),
      tools({ failed: { cause: "failed", log: [] } }),
    ],
  });
  expect(line()).not.toBeNull();
  expect(document.querySelector('[data-testid="plans-tools-retry"]')).toBeNull();
});
