// @vitest-environment happy-dom
// The web sources at the end of an answer, in the real room over FakeTransport: envelopes on the channel's events arm
// put a shut summary under their turn's answer, which opens to one row per search or page; Codex's search and open
// page say what Codex doesn't tell; a link opens through open_external; a Hindi title is marked Hindi; and a turn with
// none draws nothing.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import { eventsOf, searched, visited } from "../../testing/web-sources.ts";
import type { Envelope } from "../../transport/protocol.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
});

async function answered(envelopes: Envelope[] | null) {
  h = await mountRoom();
  h.deliver([
    reset([
      userMessage(5, 1, "What is the notice period under the Rent Control Act?"),
      agentText(6, 1, "Thirty days, in writing."),
      step(9),
    ]),
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
      { title: "Rent Control Act, s. 14", url: "https://indiankanoon.org/doc/1/" },
    ]),
    visited(2, "https://indiankanoon.org/doc/1/", { title: "Rent Control Act, s. 14" }),
    visited(3, "https://example.com/a"),
  ]);
  const list = h!.one("web-sources");
  expect(list?.closest("section")?.classList.contains("step-answer")).toBe(true);
  const words = h!.one("message-agent")!;
  expect(words.compareDocumentPosition(list!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(toggle().textContent).toBe("1 web search. 2 pages read.");
  expect(toggle().getAttribute("aria-expanded")).toBe("false");
  expect(toggle().querySelectorAll("svg[aria-hidden]").length).toBe(3);
  expect(rows()).toEqual([]);
});

test("opening shows one row per source, in order, and shuts again", async () => {
  await answered([
    searched(1, "notice period", [
      { title: "Rent Control Act, s. 14", url: "https://www.indiankanoon.org/doc/1/" },
      { title: "Notice to quit", url: "https://example.com/notice" },
    ]),
    visited(2, "https://example.com/page", { title: "A page" }),
  ]);
  await open();
  expect(toggle().getAttribute("aria-expanded")).toBe("true");
  expect(rows().map((r) => r.dataset["kind"])).toEqual(["search", "web_fetch"]);
  expect(rows()[0]!.querySelector("p")?.textContent).toBe("Searched the web for notice period.");
  const results = h!.all("web-source-result").map((r) => r.textContent);
  expect(results).toEqual([
    "Rent Control Act, s. 14 indiankanoon.org",
    "Notice to quit example.com",
  ]);
  expect(rows()[1]!.textContent).toBe("Read A page example.com.");
  await open();
  expect(rows()).toEqual([]);
});

test("a Claude search with no results says so", async () => {
  await answered([searched(1, "rare query")]);
  await open();
  expect(rows()[0]!.textContent).toBe("Searched the web for rare query. No results.");
});

test("a Codex search says Codex doesn't report its pages, and shows no links", async () => {
  await answered([searched(1, "notice period", [], { agent: "codex-acp" })]);
  await open();
  expect(rows()[0]!.textContent).toBe(
    "Searched the web for notice period. ChatGPT doesn't report which pages it found.",
  );
  expect(rows()[0]!.querySelector("a")).toBeNull();
});

test("a page Codex opened says it was opened on OpenAI's computers", async () => {
  await answered([visited(1, "https://example.com/x", { via: "codex_open_page" })]);
  await open();
  expect(rows()[0]!.textContent).toBe(
    "Read https://example.com/x example.com.ChatGPT opened this page on OpenAI's computers, not on this one.",
  );
});

test("a page the browser opened says so, and an unknown way reads as a plain read", async () => {
  await answered([
    visited(1, "https://example.com/b", { via: "browser", title: "B" }),
    visited(2, "https://example.com/u", { via: "teleport" as never, title: "U" }),
  ]);
  await open();
  expect(rows()[0]!.textContent).toBe("Opened B example.com in the browser.");
  expect(rows()[1]!.textContent).toBe("Read U example.com.");
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
