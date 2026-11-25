// @vitest-environment happy-dom
// Settings › Connectors with the server's words for what credits buy (r7470): under the balance's credits left
// ("Enough for about 3 research questions.") and after each pack's price in Add credits, as written; nothing when the
// server sent none.
import { afterEach, expect, test, vi } from "vitest";
import { stubFonts } from "../../room/thread/testing.ts";
import type { Balance, ConnectorView, PackView } from "../../transport/protocol.ts";
import { mountSettings, PACKS, stateOf, type Rig } from "./connectors-rig.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
stubFonts();
vi.mock("../../transport/window-title.ts", () => ({ setWindowTitle: () => {} }));

let rig: Rig | null = null;
afterEach(async () => {
  await rig?.unmount();
  rig = null;
});

const saying = <T extends PackView | Balance>(value: T, text: string): T =>
  ({ ...value, research_questions_text: text }) as T;

const signed = (balance: Balance): ConnectorView["status"] => ({
  signed_in: { balance, low: false },
});
const questions = (r: Rig) =>
  r.el.querySelector('[data-testid="connectors-questions"]')?.textContent ?? null;

test("with the server's words: under credits left, and after each pack's price", async () => {
  const packs = [saying(PACKS[0]!, "less than one research question"), ...PACKS.slice(1)];
  rig = await mountSettings({
    state: stateOf({
      packs,
      status: signed(saying({ credits: 10_000 }, "about 25 research questions")),
    }),
    hash: "#connectors",
  });
  expect(questions(rig)).toBe("Enough for about 25 research questions.");
  await rig.press("Add credits");
  expect(rig.text()).toContain("200 credits, ₹23.60, for less than one research question");
  // a pack the server said nothing about keeps its plain line
  expect(rig.text()).toContain("1,000 credits, ₹118");
  expect(rig.text()).not.toContain("1,000 credits, ₹118, for");
});

test("without them, nothing new: no line under the balance, the packs as before", async () => {
  rig = await mountSettings({
    state: stateOf({ status: signed({ credits: 10_000 }) }),
    hash: "#connectors",
  });
  expect(questions(rig)).toBeNull();
  expect(rig.text()).not.toContain("Enough for");
  await rig.press("Add credits");
  expect(rig.text()).toContain("200 credits, ₹23.60");
  expect(rig.text()).not.toContain("research question");
});
