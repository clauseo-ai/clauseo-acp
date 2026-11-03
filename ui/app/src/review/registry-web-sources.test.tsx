// @vitest-environment happy-dom
// The web sources' copy review cells, rendered as the review renders them (in StrictMode): every cell but WEB-12 and
// WEB-13 shows its rows without a click, those two the folded line alone; WEB-09's info tip and WEB-10's tooltip show
// their words; and the tool calls above the answer carry the titles the real adapters send, never a made-up "Web".
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

const FOLDED = ["WEB-12", "WEB-13"];
/** What a reader reads: the text without the tiles' letters. */
const words = (el: Element | null | undefined): string => {
  if (el === null || el === undefined) return "";
  const copy = el.cloneNode(true) as Element;
  for (const hidden of copy.querySelectorAll("[aria-hidden=true]")) hidden.remove();
  return copy.textContent ?? "";
};
/** Frames run until `ready` holds, as the review page waits for a state's presses. */
async function until(ready: () => boolean) {
  for (let i = 0; i < 20 && !ready(); i += 1)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 20)));
}

test.each(WEB_SOURCES.filter((e) => !FOLDED.includes(e.id)).map((e) => e.id))(
  "%s shows its rows without a click",
  async (id) => {
    const host = await cell(id);
    expect(
      host.querySelector('[data-testid="web-sources-toggle"]')?.getAttribute("aria-expanded"),
    ).toBe("true");
    expect(rows(host).length).toBeGreaterThan(0);
  },
);

test("WEB-12 shows the line folded: two sites' tiles and the search glyph, counting the five rows", async () => {
  const host = await cell("WEB-12");
  const toggle = host.querySelector('[data-testid="web-sources-toggle"]');
  expect(words(toggle)).toBe("5 sources");
  const marks = [...host.querySelector('[data-testid="web-sources-marks"]')!.children];
  expect(marks.map((m) => (m.classList.contains("site-tile") ? m.textContent : "search"))).toEqual([
    "I",
    "L",
    "search",
  ]);
  expect(toggle?.getAttribute("aria-expanded")).toBe("false");
  expect(rows(host).length).toBe(0);
});

test("WEB-13 shows one tile and 1 source, folded", async () => {
  const host = await cell("WEB-13");
  expect(words(host.querySelector('[data-testid="web-sources-toggle"]'))).toBe("1 source");
  expect(host.querySelectorAll('[data-testid="web-sources-marks"] .site-tile').length).toBe(1);
  expect(rows(host).length).toBe(0);
});

test("WEB-09 shows its info tip open, saying where the page was opened", async () => {
  await cell("WEB-09");
  const said = () =>
    [...document.querySelectorAll("[role=dialog]")].some(
      (d) => d.textContent === "Opened on OpenAI's side",
    );
  await until(said);
  expect(said()).toBe(true);
});

test("WEB-10 shows the browser glyph focused, its tooltip saying how the page was opened", async () => {
  const host = await cell("WEB-10");
  const glyph = () => host.querySelector('[data-testid="web-source-browser"]');
  await until(() => document.activeElement === glyph());
  expect(document.activeElement).toBe(glyph());
  expect(glyph()?.getAttribute("aria-label")).toBe("Opened in the browser");
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

test("WEB-05's rows lead with icons: a quoted query, nested results, pages as link and site", async () => {
  const host = await cell("WEB-05");
  expect(
    [...host.querySelectorAll('[data-testid="web-source-query"]')].map((q) => q.textContent),
  ).toEqual(["'Delhi Rent Control Act notice period'", "'section 14 eviction notice thirty days'"]);
  expect(
    [...host.querySelectorAll('[data-testid="web-source-result"]')].map((r) => words(r)),
  ).toEqual([
    "Section 14 in the Delhi Rent Control Act, 1958 indiacode.nic.in",
    "Notice to quit under the Rent Control Act livelaw.in",
  ]);
  expect([...rows(host)].slice(2).map((r) => words(r))).toEqual([
    "Section 14 in the Delhi Rent Control Act, 1958 indiacode.nic.in",
    "https://delhihighcourt.nic.in/judgments/2019/rent-control-notice-period-eviction-petition.pdf",
    "Notice to quit under the Rent Control Act livelaw.in",
  ]);
});

test("the retired ids stay retired", () => {
  const ids = WEB_SOURCES.map((e) => e.id);
  for (const gone of ["WEB-01", "WEB-02", "WEB-03", "WEB-04", "WEB-08", "WEB-14", "WEB-15"])
    expect(ids).not.toContain(gone);
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
