// @vitest-environment happy-dom
// The refused step's copy review cells (UC-600 to UC-612), rendered as the review renders them (in StrictMode): each
// shows its line under the refused row, and none shows Claude Code's raw words. In English only, as the other
// registry tests: the room takes its language from the page's display, which the review sets per frame; refusals.test.ts
// checks the Hindi lines.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { stubFonts } from "../room/thread/testing.ts";
import { PLAIN_REFUSAL } from "./registry-plain-refusal.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
  document.body.replaceChildren();
  localStorage.clear();
});

const said = () =>
  [...document.querySelectorAll('[data-testid="tool-call-why"]')]
    .map((n) => n.textContent ?? "")
    .join("\n");

test("the entries are UC-600 to UC-612, one for each refusal line", () => {
  expect(PLAIN_REFUSAL.map((e) => e.id)).toEqual(
    Array.from({ length: 13 }, (_, i) => `UC-${600 + i}`),
  );
  expect(new Set(PLAIN_REFUSAL.map((e) => e.words.key)).size).toBe(13);
});
test.each(PLAIN_REFUSAL.map((e) => e.id))(
  "%s shows its line under the refused row",
  async (id) => {
    const entry = PLAIN_REFUSAL.find((e) => e.id === id)!;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
    unmount = async () => {
      await act(async () => root.unmount());
      host.remove();
    };
    const want = entry.words.text("en");
    // the first room in a run loads the thread's chunks: up to 10 s
    for (let i = 0; i < 200 && said() !== want; i += 1)
      await act(async () => new Promise((resolve) => setTimeout(resolve, 50)));
    expect(said()).toBe(want);
    expect(document.body.textContent).not.toMatch(/classifier|auto mode|has been denied/i);
  },
  20_000,
);
