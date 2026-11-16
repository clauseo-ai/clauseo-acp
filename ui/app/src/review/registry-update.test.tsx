// @vitest-environment happy-dom
// The updater's copy review cells (R23), rendered as the review renders them (in StrictMode): each shows its string,
// its slots filled, in the state it names; the room's cells show the notice's state.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { stubFonts } from "../room/thread/testing.ts";
import { R23 } from "./registry-update.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
  document.body.replaceChildren();
  localStorage.clear();
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
  "R23-01": "ready",
  "R23-04": "ready",
  "R23-05": "ready",
  "R23-07": "scheduled",
  "R23-09": "installing",
  "R23-10": "updated",
};

test.each(R23.map((e) => e.id))("%s shows its string", async (id) => {
  const entry = R23.find((e) => e.id === id)!;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
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
    expect(document.querySelector(".update-notice")?.getAttribute("data-state")).toBe(state);
});
