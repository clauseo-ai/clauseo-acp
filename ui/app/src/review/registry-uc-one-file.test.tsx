// @vitest-environment happy-dom
// Restoring one document's copy review cells (UC-316 to UC-325, and UC-428), rendered as the review renders them (in StrictMode):
// each shows its string, its slots filled, in the state it names, after the presses that bring it.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { stubFonts } from "../room/thread/testing.ts";
import { UC_ONE_FILE } from "./registry-uc-one-file.tsx";

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

const said = (selector: string) =>
  [...document.querySelectorAll(selector)].map((n) => n.textContent ?? "").join("\n");

test.each(UC_ONE_FILE.map((e) => e.id))("%s shows its string where it stands", async (id) => {
  const entry = UC_ONE_FILE.find((e) => e.id === id)!;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  const want = patternOf(entry.words.text("en"));
  const where = id === "UC-316" ? '[data-testid="version-restore-file"]' : "body";
  // up to 15 s: the first cell also loads Versions' chunk cold
  for (let i = 0; i < 300 && !want.test(said(where)); i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 50)));
  expect(said(where)).toMatch(want);
});
