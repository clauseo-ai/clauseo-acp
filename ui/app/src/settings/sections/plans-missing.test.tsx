// @vitest-environment happy-dom
// Plans with Claude not here yet (0033, "When an install starts"): its install line offers the room gate's Sign in,
// which sends the gate's start_sign_in, so desktop downloads Claude and signs in once it is ready; the line then shows
// the download. A plan the firm's policy locks still shows no control.
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

test("Claude not here yet: the size and Sign in, which sends the room's start_sign_in; the line then shows the download", async () => {
  const t = await mountPlans({ first: MISSING });
  const claude = block("claude")!;
  expect(claude.dataset["state"]).toBe("install");
  expect(claude.textContent).toContain("Needs a download of about 130 MB");
  const sign = claude.querySelector<HTMLElement>('[data-testid="plans-sign-in"]');
  expect(sign?.textContent).toBe("Sign in");
  await press(sign);
  expect(t.setups()).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  await play(
    t,
    signIn(CLAUDE, "queued"),
    install(CLAUDE, { downloading: { received: 10_000_000, total: 130_000_000 } }),
  );
  expect(block("claude")!.textContent).toContain("Getting Claude ready… 10 MB of 130 MB");
  expect(block("claude")!.querySelector('[data-testid="plans-sign-in"]')).toBeNull();
  // ChatGPT keeps its own card
  expect(block("chatgpt")!.querySelector('[data-testid="plans-add"]')).not.toBeNull();
});

test("Claude not here yet under the firm's lock: Managed by firm and no control", async () => {
  const t = await mountPlans({ first: MISSING, locked: ["plans.claude-acp"] });
  const claude = block("claude")!;
  expect(claude.dataset["state"]).toBe("locked");
  expect(claude.querySelectorAll("button, input")).toHaveLength(0);
  expect(t.setups()).toEqual([]);
});
