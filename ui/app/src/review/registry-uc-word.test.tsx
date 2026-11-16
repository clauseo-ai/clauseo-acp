// @vitest-environment happy-dom
// The tracked changes' copy review cells (UC-120 on), rendered as the review renders them (in StrictMode): each shows
// its string, its slots filled, in the state it names, after the presses that bring it.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, expect, test } from "vitest";
import { stubFonts } from "../room/thread/testing.ts";
import { assertWarm, WARM_MS, warmUp } from "@clauseo/components/lib/warm-up.ts";
import { UC_WORD } from "./registry-uc-word.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** Renders a cell as the tests do, then takes it away: the first render of a cold file is the slow one. */
async function renderAndTake(element: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{element}</StrictMode>));
  await act(async () => root.unmount());
  host.remove();
}

// every cell rendered once before the first test, under its own budget (warm-up.ts), so each test measures itself
let warmFailure: string | null = null;
beforeAll(async () => {
  warmFailure = await warmUp("the review cells' first render", async () => {
    for (const entry of UC_WORD) await renderAndTake(entry.render("en"));
    // what the renders left behind is not the first test's to see
    document.body.replaceChildren();
    localStorage.clear();
  });
}, WARM_MS + 5_000);
beforeEach(() => {
  assertWarm(warmFailure);
});

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

/** Where each string stands once its state is up. */
const WHERE: Record<string, string> = {
  "UC-120": 'aside.room-pane [data-testid="tracked-changes"]',
  "UC-121": '[data-testid="word-changes-empty"]',
  "UC-122": '[data-testid="word-changes"] .rd-change-kind',
  "UC-123": '[data-testid="word-changes"] .rd-change-kind',
  "UC-124": '[data-testid="word-changes-line"]',
  "UC-125": '[data-testid="word-changes-line"]',
  "UC-126": '[data-testid="word-changes-line"]',
  "UC-127": '[data-testid="word-changes-line"]',
  "UC-128": '[data-testid="version"][data-kind="revisions"] .ver-entry-who',
  "UC-260": '[data-testid="word-changes-more"]',
};

const said = (selector: string) =>
  [...document.querySelectorAll(selector)].map((n) => n.textContent ?? "").join("\n");

test.each(UC_WORD.map((e) => e.id))("%s shows its string where it stands", async (id) => {
  const entry = UC_WORD.find((e) => e.id === id)!;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  const want = patternOf(entry.words.text("en"));
  const where = WHERE[id] ?? "body";
  for (let i = 0; i < 60 && !want.test(said(where)); i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 50)));
  expect(said(where)).toMatch(want);
});
