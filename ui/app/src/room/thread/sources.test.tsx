// @vitest-environment happy-dom
// The web sources at the end of an answer, in the real room over FakeTransport, icons first: envelopes on the channel's
// events arm put a folded line under their turn's answer (up to three sites' tiles or the search glyph, the count, a
// chevron), which opens to one row per search or page led by its icon; Codex's search and open page say what Codex
// doesn't tell in a few muted words or an info tip; a browser page shows the browser glyph, named; a link opens through
// open_external; a Hindi title is marked Hindi; no tile fetches anything; and a turn with none draws nothing.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import { eventsOf, headerOf, searched, visited, webCallsFor } from "../../testing/web-sources.ts";
import type { AgentId, Envelope } from "../../transport/protocol.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

/** A turn answered by `agent`, with a tool call entry for each tool call the envelopes name (ToolCallWeb's owner). */
async function answered(envelopes: Envelope[] | null, agent: AgentId = "claude-acp") {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        userMessage(5, 1, "What is the notice period under the Rent Control Act?"),
        ...webCallsFor(envelopes ?? [], agent),
        agentText(40, 1, "Thirty days, in writing."),
        step(50),
      ],
      headerOf(agent),
    ),
  ]);
  if (envelopes !== null) await act(async () => h!.transport.deliver(eventsOf(envelopes)));
  return h;
}

const toggle = () => h!.one("web-sources-toggle") as HTMLButtonElement;
const rows = () => h!.all("web-source");
const open = async () => h!.click(toggle());
/** What a reader reads: an element's text without what is hidden from assistive technology (the tiles' letters). */
const words = (el: Element | null | undefined): string => {
  if (el === null || el === undefined) return "";
  const copy = el.cloneNode(true) as Element;
  for (const hidden of copy.querySelectorAll("[aria-hidden=true]")) hidden.remove();
  return copy.textContent ?? "";
};
const count = () => h!.one("web-sources-count")?.textContent;
/** The folded line's marks: a tile's letter, or "search" for the search glyph. */
const marks = () =>
  [...h!.one("web-sources-marks")!.children].map((m) =>
    m.classList.contains("site-tile")
      ? `${m.textContent}:${m.getAttribute("data-size")}`
      : "search",
  );
/** A row's leading mark: its tile's letter, "search", "browser" or "info". */
const lead = (row: Element) => {
  const first = row.querySelector(":scope > span");
  if (first?.querySelector(".site-tile") != null) return `tile:${first.textContent}`;
  if (first?.querySelector('[role="img"]') != null) return "browser";
  return first?.querySelector("svg") == null ? "none" : "glyph";
};

test("envelopes delivered put the folded line under that turn's answer: tiles, the count, nothing else", async () => {
  await answered([
    searched(1, "notice period rent control", [
      {
        title: "Rent Control Act, s. 14",
        url: "https://www.indiacode.nic.in/handle/123456789/1525",
      },
    ]),
    visited(2, "https://www.indiacode.nic.in/handle/123456789/1525", {
      title: "Rent Control Act, s. 14",
    }),
    visited(3, "https://example.com/a"),
  ]);
  const list = h!.one("web-sources");
  expect(list?.closest("section")?.classList.contains("step-answer")).toBe(true);
  const said = h!.one("message-agent")!;
  expect(said.compareDocumentPosition(list!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  // each site once, in order, at 11 px; the words are the count alone
  expect(marks()).toEqual(["I:11", "E:11"]);
  // two pages: the search's result and the page read are one address
  expect(words(toggle())).toBe("2 sources");
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  // the chevron and no other glyph
  expect(toggle().querySelectorAll(":scope > svg").length).toBe(1);
  expect(rows()).toEqual([]);
});

test("the folded line shows at most three marks, distinct, in order, the search glyph for a search with none", async () => {
  await answered([
    searched(1, "rare query"),
    visited(2, "https://www.alpha.example/1", { title: "A" }),
    visited(3, "https://alpha.example/2", { title: "A2" }),
    searched(4, "second rare query"),
    visited(5, "https://beta.example/1", { title: "B" }),
    visited(6, "https://gamma.example/1", { title: "G" }),
  ]);
  expect(marks()).toEqual(["search", "A:11", "B:11"]);
  // the four pages; the searches are no pages
  expect(count()).toBe("4 sources");
});

test("a search's results lend the folded line their sites", async () => {
  await answered([
    searched(1, "notice", [
      { title: "Z", url: "https://zeta.example/z" },
      { title: "Y", url: "https://yota.example/y" },
    ]),
  ]);
  expect(marks()).toEqual(["Z:11", "Y:11"]);
  expect(count()).toBe("2 sources");
});

test("opening shows one row per source, in order, each led by its icon, and shuts again", async () => {
  await answered([
    searched(1, "notice period", [
      {
        title: "Rent Control Act, s. 14",
        url: "https://www.indiacode.nic.in/handle/123456789/1525",
      },
      { title: "Notice to quit", url: "https://example.com/notice" },
    ]),
    visited(2, "https://example.com/page", { title: "A page" }),
  ]);
  await open();
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  expect(rows().map((r) => r.dataset["kind"])).toEqual(["search", "web_fetch"]);
  expect(rows().map(lead)).toEqual(["glyph", "tile:E"]);
  // the query is a quoted run, no sentence
  expect(h!.one("web-source-query")?.textContent).toBe("'notice period'");
  const results = h!.all("web-source-result");
  expect(results.map(words)).toEqual([
    "Rent Control Act, s. 14 indiacode.nic.in",
    "Notice to quit example.com",
  ]);
  expect(results.map(lead)).toEqual(["tile:I", "tile:E"]);
  expect(results.every((r) => r.querySelector('.site-tile[data-size="14"]') !== null)).toBe(true);
  expect(words(rows()[1])).toBe("A page example.com");
  await open();
  expect(rows()).toEqual([]);
});

test("no row says round 2's sentences", async () => {
  await answered([
    searched(1, "notice period", [{ title: "A", url: "https://example.com/a" }]),
    searched(2, "rare query"),
    visited(3, "https://example.com/b", { title: "B" }),
    visited(4, "https://example.com/c", { via: "browser", title: "C" }),
    visited(5, "https://example.com/d"),
  ]);
  await open();
  const text = words(h!.one("web-sources"));
  for (const gone of ["Searched online", "Read ", "online.", "in the browser", "Opened"])
    expect(text, gone).not.toContain(gone);
});

test("a page opened twice in one tool call shows once", async () => {
  await answered([
    visited(1, "https://example.com/a", { call: "c" }),
    visited(2, "https://example.com/a", { call: "c", title: "A" }),
  ]);
  expect(count()).toBe("1 source");
  await open();
  expect(rows().map(words)).toEqual(["A example.com"]);
});

test("a Claude search with no results says so in two muted words", async () => {
  await answered([searched(1, "rare query")]);
  await open();
  expect(h!.one("web-source-query")?.textContent).toBe("'rare query'");
  expect(h!.one("web-source-note")?.textContent).toBe("No results");
  expect(h!.all("web-source-result")).toEqual([]);
});

test("a Codex search: one muted line behind the info glyph, no links, the search glyph folded", async () => {
  await answered([searched(1, "notice period", [], { agent: "codex-acp" })], "codex-acp");
  expect(marks()).toEqual(["search"]);
  // no page, so no count: the glyph and the chevron, the button named for what it opens to
  expect(count()).toBeUndefined();
  expect(words(toggle())).toBe("");
  expect(toggle().getAttribute("aria-label")).toBe("Searches");
  await open();
  const note = h!.one("web-source-note")!;
  expect(note.textContent).toBe("ChatGPT doesn't list its pages");
  expect(note.querySelector("svg[aria-hidden]")).not.toBeNull();
  expect(rows()[0]!.querySelector("a")).toBeNull();
});

test("a page Codex opened: its tile and link, and an info tip that says it was opened on OpenAI's side", async () => {
  await answered([visited(1, "https://example.com/x", { via: "codex_open_page" })], "codex-acp");
  await open();
  expect(lead(rows()[0]!)).toBe("tile:E");
  expect(words(rows()[0]).trim()).toBe("x example.com");
  const tip = rows()[0]!.querySelector<HTMLButtonElement>(
    'button[aria-label="Opened on OpenAI\'s side"]',
  );
  expect(tip).not.toBeNull();
  await act(async () => tip!.click());
  const popup = [...document.querySelectorAll("[role=dialog]")].find(
    (d) => d.textContent === "Opened on OpenAI's side",
  );
  expect(popup).toBeDefined();
  // it opens inside the room's main landmark, in front of the thread, not at the end of the body behind it
  expect(popup!.closest("main")).not.toBeNull();
});

test("a page the browser opened shows the browser glyph in its tile's place, named; an unknown way reads as a plain page", async () => {
  await answered([
    visited(1, "https://example.com/b", { via: "browser", title: "B" }),
    visited(2, "https://example.com/u", { via: "teleport" as never, title: "U" }),
  ]);
  await open();
  expect(rows().map(lead)).toEqual(["browser", "tile:E"]);
  const glyph = rows()[0]!.querySelector('[role="img"]')!;
  expect(glyph.getAttribute("aria-label")).toBe("Opened in the browser");
  expect(rows()[0]!.querySelector(".site-tile")).toBeNull();
  expect(words(rows()[0])).toBe("B example.com");
  expect(words(rows()[1])).toBe("U example.com");
  // its tooltip's layer is inside the room's main landmark, so it opens in front of the thread
  const layer = h!.one("web-sources")!.parentElement!.querySelector("[data-layer]");
  expect(layer).not.toBeNull();
  expect(layer!.closest("main")).not.toBeNull();
});

test("the count is the distinct pages: a search's results and the pages read, an address in both once, no search counted", async () => {
  await answered([
    searched(1, "notice period", [
      { title: "A", url: "https://example.com/a" },
      { title: "B", url: "https://example.com/b" },
    ]),
    searched(2, "rare query"),
    visited(3, "https://example.com/a", { title: "A" }),
    visited(4, "https://example.com/c", { call: "c" }),
    visited(5, "https://example.com/c", { call: "c", title: "C" }),
  ]);
  expect(count()).toBe("3 sources");
  expect(toggle().getAttribute("aria-label")).toBeNull();
  await open();
  expect(rows().length).toBe(4);
});

test("searches with no pages have no count: the search glyph, the chevron, named Searches", async () => {
  await answered([searched(1, "rare query"), searched(2, "rarer query")]);
  expect(marks()).toEqual(["search"]);
  expect(count()).toBeUndefined();
  expect(words(toggle())).toBe("");
  expect(toggle().getAttribute("aria-label")).toBe("Searches");
});

test("a page with no title never links its whole address: its file name, decoded, then its site; or its site alone", async () => {
  await answered([
    visited(1, "https://example.com/docs/plain%20notice.pdf?x=1#p2"),
    visited(2, "https://example.com/opened/", { via: "browser" }),
    visited(3, "https://www.example.org/"),
    searched(4, "notice period", [{ title: "", url: "https://example.com/untitled" }]),
  ]);
  await open();
  expect(words(rows()[0])).toBe("plain notice.pdf example.com");
  expect(words(rows()[1])).toBe("opened example.com");
  // the link is the site, so nothing after it
  expect(words(rows()[2])).toBe("example.org");
  expect(rows()[2]!.querySelector('[data-testid="web-source-site"]')).toBeNull();
  expect(h!.all("web-source-result").map(words)).toEqual(["untitled example.com"]);
  for (const link of h!.all("web-source-link")) expect(link.textContent).not.toContain("://");
});

test("no tile asks the web for anything: letters only, no image, no favicon address", async () => {
  await answered([
    searched(1, "notice", [{ title: "A", url: "https://example.com/a" }]),
    visited(2, "https://example.org/b", { title: "B" }),
  ]);
  await open();
  const list = h!.one("web-sources")!;
  expect(list.querySelectorAll(".site-tile").length).toBeGreaterThan(0);
  expect(list.querySelectorAll("img, image, picture, source, link").length).toBe(0);
  expect(list.innerHTML).not.toContain("favicon");
  expect(list.innerHTML).not.toMatch(/url\(/);
});

test("a link press opens the address through open_external, never the webview", async () => {
  await answered([visited(1, "https://example.com/very/long/path?q=1", { title: "Long" })]);
  await open();
  const link = h!.one("web-source-link") as HTMLAnchorElement;
  const press = new MouseEvent("click", { bubbles: true, cancelable: true });
  await act(async () => link.dispatchEvent(press));
  await h!.settle();
  expect(press.defaultPrevented).toBe(true);
  expect(h!.transport.only("open_external")).toEqual([
    { call: "open_external", url: "https://example.com/very/long/path?q=1" },
  ]);
});

test("an address that isn't http or https sits under no link: a data: page and a file: result are plain text", async () => {
  await answered([
    visited(1, "data:text/html,<script>alert(1)</script>"),
    visited(2, "JavaScript:alert(1)", { title: "Looks fine" }),
    searched(3, "notice period", [
      { title: "Calculator", url: "file:///C:/Windows/System32/calc.exe" },
      { title: "India Code", url: "https://www.indiacode.nic.in/" },
    ]),
  ]);
  await open();
  const hrefs = [...h!.host.querySelectorAll("[data-testid=web-sources] a")].map((a) =>
    a.getAttribute("href"),
  );
  expect(hrefs).toEqual(["https://www.indiacode.nic.in/"]);
  // the words still show, as text
  const text = h!.one("web-sources")!.textContent ?? "";
  expect(text).toContain("data:text/html,<script>alert(1)</script>");
  expect(text).toContain("Looks fine");
  expect(text).toContain("Calculator");
});

test("a middle click on a source link is cancelled and opens through open_external", async () => {
  await answered([visited(1, "https://example.com/aux", { title: "Aux" })]);
  await open();
  const link = h!.one("web-source-link") as HTMLAnchorElement;
  const press = new MouseEvent("auxclick", { bubbles: true, cancelable: true, button: 1 });
  await act(async () => link.dispatchEvent(press));
  await h!.settle();
  expect(press.defaultPrevented).toBe(true);
  expect(h!.transport.only("open_external")).toEqual([
    { call: "open_external", url: "https://example.com/aux" },
  ]);
});

test("a Hindi title and query are marked Hindi, an English one isn't, and each is isolated", async () => {
  await answered([
    searched(1, "किराया नियंत्रण अधिनियम नोटिस"),
    visited(2, "https://example.in/x", { title: "दिल्ली किराया नियंत्रण अधिनियम, धारा 14" }),
    visited(3, "https://example.com/y", { title: "Rent Control Act" }),
  ]);
  await open();
  const bdis = rows().map((r) => r.querySelector("bdi"));
  // the query, then each title
  expect(bdis.map((b) => b?.getAttribute("lang"))).toEqual(["hi", "hi", null]);
});

test("a source whose tool call isn't one of the turn's shows nothing", async () => {
  h = await mountRoom();
  h.deliver([reset([userMessage(5, 1, "Q?"), agentText(6, 1, "A."), step(9)])]);
  await act(async () => h!.transport.deliver(eventsOf([searched(1, "elsewhere")])));
  expect(h.one("web-sources")).toBeNull();
});

test("no envelopes: nothing under the answer", async () => {
  await answered(null);
  expect(h!.one("web-sources")).toBeNull();
});

test("FakeTransport plays a new listener the envelopes delivered before it", () => {
  const transport = new FakeTransport();
  const first = searched(1, "before");
  transport.deliver(eventsOf([first]));
  const heard: Envelope[][] = [];
  const stop = transport.onEnvelopes((e) => heard.push([...e]));
  const second = visited(2, "https://example.com");
  transport.deliver(JSON.stringify(eventsOf([second])));
  stop();
  transport.deliver(eventsOf([visited(3, "https://example.com/c")]));
  expect(heard).toEqual([[first], [second]]);
});

test("under the folded step chip, the sources line comes after the answer's words and repeats none of the chip's words", async () => {
  await answered([
    searched(1, "notice period rent control", [
      { title: "Rent Control Act, s. 14", url: "https://example.org/act" },
    ]),
    visited(2, "https://example.com/a"),
  ]);
  const chip = h!.one("step-group")!;
  const said = h!.one("message-agent")!;
  const list = h!.one("web-sources")!;
  // the chip, then the answer's words, then the sources line, all in the answer's section
  expect(chip.compareDocumentPosition(said) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(said.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(list.closest("section")).toBe(chip.closest("section"));
  // the chip counts the lookups ("Looked online 2 times."), the line the distinct pages: neither says the other's
  const chipWords = words(chip.querySelector(".step-group-words"));
  expect(chipWords).toMatch(/online/);
  expect(chipWords).not.toMatch(/source/);
  expect(words(toggle())).toBe("2 sources");
});
