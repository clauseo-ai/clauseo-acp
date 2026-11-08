// @vitest-environment happy-dom
// The plan card's copy review cells (U12), rendered as the review renders them (in StrictMode): each opens the card
// from the toolbar and shows its string, which needs the plan signed in on the setup wire (hosts.ts's threadHost);
// the Codex model bucket's cell is a ChatGPT plan's conversation, its toolbar naming that plan.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { stubFonts } from "../room/thread/testing.ts";
import { U12 } from "./registry-u12.tsx";

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

const said = () => document.body.textContent ?? "";

const CARDS = U12.filter((e) => e.screen === "limits/plan-card").map((e) => e.id);

test("the model-window cells are among the card cells", () => {
  expect(CARDS).toEqual(expect.arrayContaining(["U12-44", "U12-47"]));
});

test.each(CARDS)("%s opens the plan card with its string", async (id) => {
  const entry = U12.find((e) => e.id === id)!;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  const card = () => document.body.querySelector(".plan-card")?.textContent ?? "";
  const want = patternOf(entry.words.text("en"));
  for (let i = 0; i < 150 && !want.test(card()); i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(card()).toMatch(want);
});

test("U12-47's toolbar names the ChatGPT plan the card shows", async () => {
  const entry = U12.find((e) => e.id === "U12-47")!;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  const plan = () => document.body.querySelector('[data-testid="plan"]')?.textContent ?? "";
  for (let i = 0; i < 150 && !plan().includes("ChatGPT plan"); i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  expect(plan()).toContain("ChatGPT plan");
  expect(said()).not.toContain("Claude plan");
});
