// @vitest-environment happy-dom
// A case-law step's cost, rendered as the review renders it (UC-488 to UC-490): the room's open step group shows the
// row's done or tried line, then a space, then its cost, and the folded line never sums it.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { loadThread } from "../room/thread/chunk.ts";
import { stubFonts } from "../room/thread/testing.ts";
import { UC_CASE_LAW_COST } from "./registry-uc-case-law-cost.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
});

const SUFFIX: Record<string, string> = {
  "UC-488": "· 40 credits",
  "UC-489": "· 1 credit",
  "UC-490": "· nothing charged",
};

/** The step rows' words once the cell's group is open. */
async function rowWords(id: string): Promise<{ rows: string[]; host: HTMLElement }> {
  const entry = UC_CASE_LAW_COST.find((e) => e.id === id);
  if (entry === undefined) throw new Error(`no cell ${id}`);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  await act(async () => void (await loadThread()));
  const rows = () =>
    [...host.querySelectorAll('[data-testid="tool-call"]')].map((r) => r.textContent ?? "");
  for (let i = 0; i < 40 && rows().length === 0; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  return { rows: rows(), host };
}

test.each(Object.keys(SUFFIX))(
  "%s: the row says its cost after its line and a space",
  async (id) => {
    const { rows } = await rowWords(id);
    expect(rows.length).toBe(1);
    const want = SUFFIX[id]!;
    const row = rows[0]!;
    expect(row).toContain(` ${want}`);
    // the words before it are the row's own line, ending in its full stop
    expect(row.slice(0, row.indexOf(want)).trimEnd()).toMatch(/\.$/);
  },
);

test("the folded line never sums the cost", async () => {
  const { host } = await rowWords("UC-488");
  const group = host.querySelector('[data-testid="step-group"]');
  const folded = group?.querySelector("button")?.textContent ?? "";
  expect(folded).not.toMatch(/credit/);
});
