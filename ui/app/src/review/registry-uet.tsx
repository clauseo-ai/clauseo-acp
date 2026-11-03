// UET's copy review entries, the web and browser brief, for the tool line (room/tool-line.tsx), apart from registry.tsx to
// keep that file under the size limit: each string in the real room over FakeTransport, its thread's header carrying
// the tools in the state that shows it, the tool line pressed so its popover stands open. A string that is only a
// tooltip and an accessible name shows with its tooltip open, its control focused from the keyboard. The Chrome
// download is faked: the host plays its `browser_install` reports (room/browser-install.ts) once the thread's view is in.
import { Composer } from "@clauseo/components/clauseo/composer.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { toolsLine, type ToolsLineKey } from "@clauseo/components/lib/words-tools-line.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { batch, event, header, reset, THREAD } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { ToolState } from "../transport/protocol.ts";
import type { BrowserInstallReport } from "../room/browser-install.ts";
import { toolsSegment } from "../room/tool-line-room.ts";
import { knownTools } from "../room/tools.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomFocusing, RoomPressFocusing, RoomPressing } from "./states.tsx";
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

type Show = "popover" | "tip" | { tip: string };

const entry = (
  id: string,
  words: WordsRef,
  tools: ToolState[],
  install?: readonly BrowserInstallReport[],
  show: Show = "popover",
): Entry => ({
  id,
  words,
  screen: "room/status",
  render: () => {
    const host = () => toolsHost(tools, install);
    if (show === "popover") return <RoomPressing host={host} press="tool-line" />;
    if (show === "tip") return <RoomFocusing host={host} focus='[data-testid="tool-line"]' />;
    return <RoomPressFocusing host={host} press="tool-line" focus={show.tip} />;
  },
});

/** The status line at its fullest, for its fit: the time taken, the plan, the tool line and the ready hint together,
 * on the composer as the room draws it. The room shows no ready hint yet, so the line stands on the composer alone,
 * its fixtures in English. At 1100 px the hint gives up its words first; at 390 px the line wraps to two. */
const statusFit = (lang: Lang) => (
  <RoomScopeProvider>
    <div className="flex min-h-dvh flex-col justify-end">
      <Composer
        took="Took 38 seconds"
        plan="Uses your Claude plan"
        hint={{
          what: "Draft reply ready",
          label: "Draft reply, ready and not yet opened",
          state: "ready",
          onTurn: () => {},
        }}
        tools={toolsSegment(lang, knownTools([web(), browser()]))}
      />
    </div>
  </RoomScopeProvider>
);

const NONE = [browser("browser_not_installed")];
const OFFER: BrowserInstallReport = { offer: "download", bytes: 201_000_000 };
const DOWNLOADING: BrowserInstallReport = {
  downloading: { bytes: 64_400_000, total: 201_000_000 },
};
/** A failed download by the host's code; its message is the log's, and never shows. */
const failed = (code: string): BrowserInstallReport[] => [
  OFFER,
  { failed: { code, message: `review: ${code}` } },
];

/** UET's tool line entries, each id written as it is: an id never moves when an entry is added or taken out. Retired
 * in round 3 (icons first, DESIGN.md §Icons): UET-02 to UET-08, UET-16, UET-17, UET-32 and UET-36. */
export const UET: Entry[] = [
  // the line's name and tooltip
  entry(
    "UET-01",
    lineWords("{tool}: {state}."),
    [web(), browser("browser_not_installed")],
    [],
    "tip",
  ),
  entry("UET-39", lineWords("on"), [web(), browser()], [], "tip"),
  entry(
    "UET-40",
    lineWords("off, {reason}"),
    [web("locked_off"), browser("too_many_browsers")],
    [],
    "tip",
  ),
  // the rows: names, chips and reasons
  entry("UET-09", lineWords("Web search"), [web(), browser()]),
  entry("UET-10", lineWords("Browser"), [web(), browser()]),
  entry("UET-11", popWords("On"), [web(), browser()]),
  entry("UET-41", popWords("Off"), [web(), browser("off")]),
  entry("UET-12", lineWords("Can't search on this login"), [web("no_search_on_this_login")]),
  entry("UET-13", lineWords("Off in Settings"), [web("off"), browser("off")]),
  entry("UET-14", lineWords("Managed by firm"), [web("locked_off"), browser("managed_by_firm")]),
  entry("UET-18", lineWords("Not available yet"), [browser("no_proxy")]),
  entry("UET-19", lineWords("Too many browsers open"), [browser("too_many_browsers")]),
  entry("UET-20", lineWords("Needs Chrome"), NONE),
  // the foot
  entry("UET-15", popWords("Open Settings"), [web("off")]),
  entry(
    "UET-33",
    popWords("Changes apply to new conversations."),
    [web("off"), browser("off")],
    [],
    {
      tip: "tool-settings",
    },
  ),
  // the download
  entry("UET-22", popWords("Download Chrome"), NONE, [OFFER]),
  entry("UET-42", popWords("200\u00a0MB"), NONE, [OFFER]),
  entry(
    "UET-21",
    popWords("About 200\u00a0MB from Google, once. Needs about 1\u00a0GB free."),
    NONE,
    [OFFER],
    { tip: "tool-download" },
  ),
  entry("UET-23", popWords("Install Chrome to use it"), NONE, [{ offer: "none" }]),
  entry("UET-24", popWords("{done} of {total}\u00a0MB"), NONE, [OFFER, DOWNLOADING]),
  entry("UET-25", popWords("Unpacking"), NONE, [OFFER, "unpacking"]),
  entry("UET-26", popWords("Chrome ready"), NONE, [OFFER, "done"]),
  entry("UET-34", popWords("Connection dropped"), NONE, failed("network")),
  entry("UET-29", popWords("File didn't match"), NONE, failed("signature")),
  entry("UET-27", popWords("Needs 1\u00a0GB free"), NONE, failed("disk_space")),
  entry("UET-28", popWords("Network blocked it"), NONE, failed("host_refused")),
  entry("UET-35", popWords("Couldn't unpack"), NONE, failed("unpack")),
  entry("UET-37", popWords("Download stopped"), NONE, failed("cancelled")),
  // a code this version doesn't know reads as unknown
  entry("UET-30", popWords("Download failed"), NONE, failed("a_newer_code")),
  entry("UET-31", popWords("Try again"), NONE, failed("host_refused"), { tip: "tool-retry" }),
  {
    id: "UET-38",
    words: lineWords("{tool}: {state}."),
    screen: "room/status",
    render: statusFit,
  },
];
