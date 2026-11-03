// @vitest-environment happy-dom
// The web sources at the end of an answer, in the real room over FakeTransport: envelopes on the channel's events arm
// put a shut summary under their turn's answer, which opens to one row per search or page; Codex's search and open
// page say what Codex doesn't tell; a link opens through open_external; a Hindi title is marked Hindi; and a turn with
// none draws nothing.
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

test("envelopes delivered put the shut summary under that turn's answer, after its words", async () => {
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
  const words = h!.one("message-agent")!;
  expect(words.compareDocumentPosition(list!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(toggle().textContent).toBe("3 sources");
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  expect(toggle().querySelectorAll("svg[aria-hidden]").length).toBe(3);
  expect(rows()).toEqual([]);
});

test("opening shows one row per source, in order, and shuts again", async () => {
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
  expect(rows()[0]!.querySelector("p")?.textContent).toBe("Searched online for 'notice period'.");
  const results = h!.all("web-source-result").map((r) => r.textContent);
  expect(results).toEqual([
    "Rent Control Act, s. 14 indiacode.nic.in",
    "Notice to quit example.com",
  ]);
  expect(rows()[1]!.textContent).toBe("Read 'A page' online. example.com");
  await open();
  expect(rows()).toEqual([]);
});

test("a page opened twice in one tool call shows once", async () => {
  await answered([
    visited(1, "https://example.com/a", { call: "c" }),
    visited(2, "https://example.com/a", { call: "c", title: "A" }),
  ]);
  expect(toggle().textContent).toBe("1 source");
  await open();
  expect(rows().map((r) => r.textContent)).toEqual(["Read 'A' online. example.com"]);
});

test("a Claude search with no results says so", async () => {
  await answered([searched(1, "rare query")]);
  await open();
  expect(rows()[0]!.textContent).toBe("Searched online for 'rare query'. No results.");
});

test("a Codex search says Codex doesn't report its pages, and shows no links", async () => {
  await answered([searched(1, "notice period", [], { agent: "codex-acp" })], "codex-acp");
  await open();
  expect(rows()[0]!.textContent).toBe(
    "Searched online for 'notice period'. ChatGPT doesn't report which pages it found.",
  );
  expect(rows()[0]!.querySelector("a")).toBeNull();
});

test("a page Codex opened says it was opened on OpenAI's computers", async () => {
  await answered([visited(1, "https://example.com/x", { via: "codex_open_page" })], "codex-acp");
  await open();
  expect(rows()[0]!.textContent).toBe(
    "Read https://example.com/x online.ChatGPT opened this page on OpenAI's computers, not on this one.",
  );
});

test("a page the browser opened says so, and an unknown way reads as a plain read", async () => {
  await answered([
    visited(1, "https://example.com/b", { via: "browser", title: "B" }),
    visited(2, "https://example.com/u", { via: "teleport" as never, title: "U" }),
  ]);
  await open();
  expect(rows()[0]!.textContent).toBe("Opened 'B' in the browser. example.com");
  expect(rows()[1]!.textContent).toBe("Read 'U' online. example.com");
});

test("the summary counts the open list's rows: each search once, whatever its results, and each page once", async () => {
  await answered([
    searched(1, "notice period", [
      { title: "A", url: "https://example.com/a" },
      { title: "B", url: "https://example.com/b" },
    ]),
    searched(2, "rare query"),
    visited(3, "https://example.com/c", { call: "c" }),
    visited(4, "https://example.com/c", { call: "c", title: "C" }),
  ]);
  expect(toggle().textContent).toBe("3 sources");
  await open();
  expect(rows().length).toBe(3);
});

test("a page with no title is its address, a link, with no site after it and no quotes in the browser", async () => {
  await answered([
    visited(1, "https://example.com/plain"),
    visited(2, "https://example.com/opened", { via: "browser" }),
    searched(3, "notice period", [{ title: "", url: "https://example.com/untitled" }]),
  ]);
  await open();
  expect(rows()[0]!.textContent).toBe("Read https://example.com/plain online.");
  expect(rows()[1]!.textContent).toBe("Opened https://example.com/opened in the browser.");
  expect(h!.all("web-source-result").map((r) => r.textContent)).toEqual([
    "https://example.com/untitled",
  ]);
  expect(h!.all("web-source-site")).toEqual([]);
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
