// @vitest-environment happy-dom
// The unsafe-site list's copy review cells (UBL), rendered as the review renders them (in
// StrictMode): each shows its string, its slots filled, in the state it names, after the presses
// that lead there; the notice cells show the notice's state.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { loadThread } from "../room/thread/chunk.ts";
import { stubFonts } from "../room/thread/testing.ts";
import { UBL } from "./registry-blocklist.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
  document.body.replaceChildren();
});

/** The string as a pattern: each {slot} any run of text. */
const patternOf = (text: string) =>
  new RegExp(
    text
      .split(/\{\w+\}/)
      .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .join(".+"),
  );

/** What the page says: its text and its accessible names. */
const said = () =>
  [
    document.body.textContent ?? "",
    ...[...document.body.querySelectorAll("[aria-label]")].map((e) => e.getAttribute("aria-label")),
  ].join("\n");

const STATES: Record<string, string> = {
  "UBL-10": "allowed-matter",
  "UBL-11": "allowed-always",
  "UBL-12": "expired",
  "UBL-28": "no-override",
  "UBL-29": "override-off",
};

test.each(UBL.map((e) => e.id))("%s shows its string", async (id) => {
  const entry = UBL.find((e) => e.id === id)!;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  await act(async () => void (await loadThread()));
  const want = patternOf(entry.words.text("en"));
  for (let i = 0; i < 30 && !want.test(said()); i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  expect(said()).toMatch(want);
  const state = STATES[id];
  if (state !== undefined)
    expect(document.querySelector('[data-testid="site-blocked"]')?.getAttribute("data-state")).toBe(
      state,
    );
});
