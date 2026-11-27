// @vitest-environment happy-dom
// A3 in Settings › Plans: a plan installed whose sign-in desktop hasn't said yet (after an update, at startup) reads
// "Checking whether you're signed in to …", with no Sign in, as desktop's own checking does; signed out brings the
// card's Sign in; signed in its signed-in line. With no answer for CHECK_MS (sign-in-check.ts) it reads signed out.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import { CHECK_MS } from "../../store/sign-in-check.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  block,
  install,
  mountPlans,
  play,
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
  vi.useRealTimers();
});

const FIRST = [install(CLAUDE, READY), signIn(CLAUDE, signed()), install(CODEX, READY)];
const signInsIn = (el: HTMLElement) =>
  [...el.querySelectorAll("button")].filter((b) => (b.textContent ?? "").startsWith("Sign in"));

test("absent, then checking: ChatGPT checks with no Sign in; signed out brings it; signed in shows the line", async () => {
  const t = await mountPlans({ first: FIRST });
  const chatgpt = () => block("chatgpt")!;
  expect(chatgpt().dataset["state"]).toBe("gate");
  expect(chatgpt().textContent).toContain("Checking whether you're signed in to ChatGPT…");
  expect(signInsIn(chatgpt())).toEqual([]);
  await play(t, signIn(CODEX, "checking"));
  expect(chatgpt().textContent).toContain("Checking whether you're signed in to ChatGPT…");
  expect(signInsIn(chatgpt())).toEqual([]);
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(chatgpt().dataset["state"]).toBe("signed-out");
  expect(signInsIn(chatgpt()).length).toBeGreaterThan(0);
  await play(t, signIn(CODEX, signed({ plan: "ChatGPT Plus" })));
  expect(chatgpt().dataset["state"]).toBe("signed");
  expect(signInsIn(chatgpt())).toEqual([]);
  expect(t.setups()).toEqual([]);
});

test("no answer at all: checking until CHECK_MS, then the signed-out card with its Sign in, nothing sent", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const t = await mountPlans({ first: FIRST });
  await act(async () => {
    vi.advanceTimersByTime(CHECK_MS - 2_000);
  });
  await play(t);
  expect(block("chatgpt")!.dataset["state"]).toBe("gate");
  await act(async () => {
    vi.advanceTimersByTime(3_000);
  });
  await play(t);
  expect(block("chatgpt")!.dataset["state"]).toBe("signed-out");
  expect(signInsIn(block("chatgpt")!).length).toBeGreaterThan(0);
  expect(t.setups()).toEqual([]);
});
