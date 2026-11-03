// @vitest-environment happy-dom
// The web sources' copy review cells, rendered as the review renders them (in StrictMode): every cell but WEB-12 shows
// its rows without a click, WEB-12 the shut summary alone; and the tool calls above the answer carry the titles the
// real adapters send, never a made-up "Web".
import { readFileSync } from "node:fs";
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { searched, titleOf, visited, webCallsFor } from "../testing/web-sources.ts";
import { loadThread } from "../room/thread/chunk.ts";
import { stubFonts } from "../room/thread/testing.ts";
import { WEB_SOURCES } from "./registry-web-sources.tsx";

stubFonts();
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
});

/** One cell rendered, its thread's chunk in and its frames run, as the review page shows it. */
async function cell(id: string): Promise<HTMLElement> {
  const entry = WEB_SOURCES.find((e) => e.id === id);
  if (entry === undefined) throw new Error(`no cell ${id}`);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<StrictMode>{entry.render("en")}</StrictMode>));
  await act(async () => void (await loadThread()));
  for (let i = 0; i < 10 && host.querySelector('[data-testid="web-sources"]') === null; i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
  unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  return host;
}

const rows = (host: HTMLElement) => host.querySelectorAll('[data-testid="web-source"]');

test.each(WEB_SOURCES.filter((e) => e.id !== "WEB-12").map((e) => e.id))(
  "%s shows its rows without a click",
  async (id) => {
    const host = await cell(id);
    expect(
      host.querySelector('[data-testid="web-sources-toggle"]')?.getAttribute("aria-expanded"),
    ).toBe("true");
    expect(rows(host).length).toBeGreaterThan(0);
  },
);

test("WEB-12 shows the summary shut, counting the five rows", async () => {
  const host = await cell("WEB-12");
  expect(host.querySelector('[data-testid="web-sources-toggle"]')?.textContent).toBe("5 sources");
  expect(
    host.querySelector('[data-testid="web-sources-toggle"]')?.getAttribute("aria-expanded"),
  ).toBe("false");
  expect(rows(host).length).toBe(0);
});

test("the cells' tool calls carry the adapters' titles, never \"Web\"", async () => {
  const host = await cell("WEB-05");
  const titles = [...host.querySelectorAll('[data-testid="tool-call"]')].map((e) => e.textContent);
  expect(titles.length).toBe(5);
  expect(titles).not.toContain("Web");
  expect(titles[0]).toBe('Search "Delhi Rent Control Act notice period"');
  expect(titles[2]).toBe(
    "Fetch https://www.indiacode.nic.in/handle/123456789/1525/simple-search?query=section%2014",
  );
});

test("each adapter's title for each kind of web tool call", () => {
  expect(titleOf(searched(1, "notice"), "claude-acp")).toBe('Search "notice"');
  expect(titleOf(searched(1, "notice"), "codex-acp")).toBe("Searching for: notice");
  expect(titleOf(visited(1, "https://a.example"), "claude-acp")).toBe("Fetch https://a.example");
  expect(titleOf(visited(1, "https://a.example", { via: "codex_open_page" }), "codex-acp")).toBe(
    "Opening: https://a.example",
  );
  expect(titleOf(visited(1, "https://a.example", { via: "browser" }), "claude-acp")).toBe(
    "https://a.example",
  );
  const calls = webCallsFor([searched(1, "q"), visited(2, "https://a.example")], "claude-acp");
  expect(calls.map((c) => c.tool_call?.title)).toEqual(['Search "q"', "Fetch https://a.example"]);
});

test("WEB-05's search rows say the step list's words, the query quoted", async () => {
  const host = await cell("WEB-05");
  expect([...rows(host)].map((r) => r.querySelector("p")?.textContent).slice(0, 2)).toEqual([
    "Searched online for 'Delhi Rent Control Act notice period'.",
    "Searched online for 'section 14 eviction notice thirty days'. No results.",
  ]);
});

test("WEB-08's page rows end their sentence after online, the site muted after a title only", async () => {
  const host = await cell("WEB-08");
  expect([...rows(host)].map((r) => r.textContent)).toEqual([
    "Read 'Section 14 in the Delhi Rent Control Act, 1958' online. indiacode.nic.in",
    "Read https://delhihighcourt.nic.in/judgments/2019/rent-control-notice-period-eviction-petition.pdf online.",
  ]);
});

test("WEB-10's browser row quotes the title and puts the site after the whole sentence", async () => {
  const host = await cell("WEB-10");
  expect(rows(host)[0]?.textContent).toBe(
    "Opened 'Delhi High Court: judgments' in the browser. delhihighcourt.nic.in",
  );
});

test("WEB-14's browser row with no title is its address, unquoted, with no site", async () => {
  const host = await cell("WEB-14");
  expect(rows(host)[0]?.textContent).toBe(
    "Opened https://delhihighcourt.nic.in/judgments/2019/rent-control-notice-period-eviction-petition.pdf in the browser.",
  );
});

// Nothing at launch names the judgment search the lead left out, the copy page included
test("no fixture or cell of the web sources names the left-out judgment search", () => {
  const files = [
    "./registry-web-sources.tsx",
    "./registry-web-sources.test.tsx",
    "../testing/web-sources.ts",
    "../room/thread/sources.test.tsx",
    "../room/thread/sources.test.ts",
  ];
  for (const file of files) {
    const text = readFileSync(new URL(file, import.meta.url), "utf8").toLowerCase();
    // split so this test's own words don't match
    expect(text.includes("indian" + "kanoon"), file).toBe(false);
    expect(text.includes("indian" + " kanoon"), file).toBe(false);
  }
  for (const id of WEB_SOURCES.map((e) => e.id)) expect(id).toMatch(/^WEB-\d\d$/);
});
