// @vitest-environment happy-dom
// Plans (UD2) and the two cancels (installs-cancel): a plan waiting behind the other plan's sign-in has no "Cancel
// {name}'s sign-in" here, since the plan ahead's block shows its own Cancel, which ends that sign-in; and Cancel on a
// sign-in queued while the plan isn't installed yet sends cancel_agent_install, after which the host's `missing` shows
// the card to add it again.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
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

const words = (plan: "claude" | "chatgpt") => block(plan)?.textContent ?? "";

test("ChatGPT queued behind Claude's sign-in: no second Cancel for Claude on its card; Claude's own Cancel sends cancel_sign_in, and desktop starts ChatGPT's", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { waiting: { since: AT, nudge: false } }),
      install(CODEX, READY),
      signIn(CODEX, "queued"),
    ],
  });
  expect(words("chatgpt")).toContain("Your browser opens for ChatGPT after Claude's sign-in.");
  expect(words("chatgpt")).not.toContain("Cancel Claude's sign-in");
  expect(button("Cancel", block("chatgpt")!)).not.toBeNull();
  await press(button("Cancel", block("claude")!));
  expect(t.setups()).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
  await play(
    t,
    signIn(CLAUDE, { signed_out: {} }),
    signIn(CODEX, { waiting: { since: AT, nudge: false } }),
  );
  expect(words("chatgpt")).toContain("Finish signing in with your browser.");
});

test("ChatGPT queued behind Claude's device code: Claude's block shows its code with its own Cancel, so ChatGPT's card has no second one", async () => {
  await mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { device_code: { url: "https://x", code: "ABCD-1234", expires_at: AT } }),
      install(CODEX, READY),
      signIn(CODEX, "queued"),
    ],
  });
  expect(button("Cancel", block("claude")!)).not.toBeNull();
  expect(words("chatgpt")).not.toContain("Cancel Claude's sign-in");
});

test("Cancel on ChatGPT's sign-in queued behind its paused download sends cancel_agent_install; the host's missing shows Add ChatGPT again", async () => {
  const t = await mountPlans({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, { missing: { download_bytes: 139_000_000 } }),
      install(CODEX, "paused"),
      signIn(CODEX, "queued"),
    ],
  });
  await press(button("Cancel", block("chatgpt")!));
  expect(t.setups()).toEqual([{ cancel_agent_install: { agent: CODEX } }]);
  await play(
    t,
    install(CODEX, { missing: { download_bytes: 139_000_000 } }),
    signIn(CODEX, { signed_out: {} }),
  );
  expect(block("chatgpt")?.dataset["state"]).toBe("add");
  expect(words("chatgpt")).toContain("Needs a download of about 140 MB.");
});
