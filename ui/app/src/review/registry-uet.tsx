// UET's copy review entries, the web and browser brief, for the tool line (room/tool-line.tsx), apart from registry.tsx to keep that file under
// the size limit: each string in the real room over FakeTransport, its thread's header carrying the tools in the
// state that shows it, the tool line pressed so its popover stands open. The Chrome download is faked: the host
// plays its `browser_install` reports (room/browser-install.ts) once the thread's view is in.
import { toolsLine, type ToolsLineKey } from "@clauseo/components/lib/words-tools-line.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { batch, event, header, reset, THREAD } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { ToolState } from "../transport/protocol.ts";
import type { BrowserInstallReport } from "../room/browser-install.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomPressing } from "./states.tsx";
import type { WordsRef } from "./words.ts";

const lineWords = (key: ToolsLineKey): WordsRef => ({
  file: "lib/words-tools-line.ts",
  key,
  text: (lang) => toolsLine(lang, key),
  later: toolsLine.hi[key] === null,
});

const popWords = (key: ToolsKey): WordsRef => ({
  file: "lib/words-tools.ts",
  key,
  text: (lang) => tw(lang, key),
  later: tw.hi[key] === null,
});

const web = (reason?: string): ToolState => ({
  tool: "web_search",
  availability: reason === undefined ? "available" : ({ unavailable: { reason } } as never),
});
const browser = (reason?: string): ToolState => ({
  tool: "browser",
  availability: reason === undefined ? "available" : ({ unavailable: { reason } } as never),
});

/** A host with one matter and its one thread, whose header carries `tools`, with Settings, and the app feed that
 * plays `install`'s reports once the view is in. */
function toolsHost(
  tools: ToolState[],
  install: readonly BrowserInstallReport[] = [],
): FakeTransport {
  const head = header();
  head.header!.tools = tools;
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined)
        return {
          threads: [{ thread: THREAD, matter: MATTER.matter, agent: "claude-acp", status: "idle" }],
        };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    openSettings: () => {},
    next: { parts: ["app"], query: () => Promise.reject(new Error("not scripted")) },
    subscribe: (stream) =>
      queueMicrotask(() => {
        transport.deliver(event(batch(1, [reset([], head)], undefined, stream)));
        for (const report of install) transport.appEvent({ browser_install: report });
      }),
  });
  return transport;
}

const entry = (
  id: string,
  words: WordsRef,
  tools: ToolState[],
  install?: readonly BrowserInstallReport[],
): Entry => ({
  id,
  words,
  screen: "room/status",
  render: () => <RoomPressing host={() => toolsHost(tools, install)} press="tool-line" />,
});

const NONE = [browser("browser_not_installed")];
const OFFER: BrowserInstallReport = { offer: "download", bytes: 201_000_000 };
const failed = (code: "disk" | "refused" | "check" | "other"): BrowserInstallReport[] => [
  OFFER,
  { failed: { code } },
];

/** UET's tool line entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const UET: Entry[] = [
  entry("UET-01", lineWords("Web search and browser on"), [web(), browser()]),
  entry("UET-02", lineWords("Web search on"), [web(), browser("off")]),
  entry("UET-03", lineWords("Web pages and browser on"), [
    web("no_search_on_this_login"),
    browser(),
  ]),
  entry("UET-04", lineWords("Web pages only"), [web("no_search_on_this_login")]),
  entry("UET-05", lineWords("Browser on, no web search"), [web("off"), browser()]),
  entry("UET-06", lineWords("Web search off"), [web("locked_off"), browser("no_proxy")]),
  entry("UET-07", lineWords("Browser on"), [browser()]),
  entry("UET-08", lineWords("Browser off"), [browser("too_many_browsers")]),
  entry("UET-09", popWords("Web search"), [web(), browser()]),
  entry("UET-10", popWords("Browser"), [web(), browser()]),
  entry("UET-11", popWords("On. Clauseo can search the web and read pages."), [web()]),
  entry(
    "UET-12",
    popWords("This {name} login can't search the web. Clauseo can still read web pages."),
    [web("no_search_on_this_login")],
  ),
  entry("UET-13", popWords("Off. You turned it off in Settings."), [web("off"), browser("off")]),
  entry("UET-14", popWords("Off. {managed}."), [web("locked_off"), browser("locked_off")]),
  entry("UET-15", popWords("Open Settings"), [web("off")]),
  entry("UET-16", popWords("On. Clauseo can open web pages in its own browser."), [browser()]),
  entry(
    "UET-17",
    popWords(
      "Off. Your organisation chooses the tools {name} has on this computer, so Clauseo adds none.",
    ),
    [browser("managed_by_firm")],
  ),
  entry("UET-18", popWords("Off until Clauseo can check each website the browser opens."), [
    browser("no_proxy"),
  ]),
  entry(
    "UET-19",
    popWords(
      "Off. Other conversations are using every browser this computer can run. One frees up when another conversation closes.",
    ),
    [browser("too_many_browsers")],
  ),
  entry("UET-20", popWords("Off. There's no Chrome on this computer."), NONE),
  entry(
    "UET-21",
    popWords(
      "Clauseo can download its own Chrome from Google, about 200 MB, once for all conversations. It needs about 1 GB free.",
    ),
    NONE,
    [OFFER],
  ),
  entry("UET-22", popWords("Download Chrome"), NONE, [OFFER]),
  entry("UET-23", popWords("Install Google Chrome to use the browser."), NONE, [{ offer: "none" }]),
  entry("UET-24", popWords("Downloading Chrome. {done} of {total} MB."), NONE, [
    OFFER,
    { downloading: { bytes: 64_400_000, total: 201_000_000 } },
  ]),
  entry("UET-25", popWords("Unpacking Chrome."), NONE, [OFFER, "unpacking"]),
  entry("UET-26", popWords("Chrome is ready. New conversations get the browser."), NONE, [
    OFFER,
    "done",
  ]),
  entry(
    "UET-27",
    popWords("Chrome couldn't be downloaded. It needs about 1 GB free on this computer."),
    NONE,
    failed("disk"),
  ),
  entry("UET-28", popWords("Your network is blocking the download."), NONE, failed("refused")),
  entry(
    "UET-29",
    popWords("The download didn't match Google's, so Clauseo deleted it."),
    NONE,
    failed("check"),
  ),
  entry("UET-30", popWords("Chrome couldn't be downloaded."), NONE, failed("other")),
  entry("UET-31", popWords("Try again"), NONE, failed("refused")),
];
