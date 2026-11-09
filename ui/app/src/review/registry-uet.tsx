// UET's copy review entries, the web and browser brief, for the tool line (room/tool-line.tsx), apart from registry.tsx to
// keep that file under the size limit: each string in the real room over FakeTransport, its thread's header carrying
// the tools in the state that shows it, the tool line pressed so its popover stands open. A string that is only a
// tooltip and an accessible name shows with its tooltip open, its control focused from the keyboard. The firm's
// browser policy re-read is faked: the host plays its `browser_policy` reports (room/browser-policy.ts) once the
// thread's view is in. What to install for the browser follows the host's OS: a Mac unless the entry says Windows.
import { Composer } from "@clauseo/components/clauseo/composer.tsx";
import { RoomScopeProvider } from "@clauseo/components/clauseo/room-scope-provider.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { toolsLine, type ToolsLineKey } from "@clauseo/components/lib/words-tools-line.ts";
import { tw, type ToolsKey } from "@clauseo/components/lib/words-tools.ts";
import { batch, event, header, reset, THREAD } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { BrowserPolicyState, ToolState } from "../transport/protocol.ts";
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

/** What an entry's host does beyond its header: the policy reports it plays, and its OS. */
type HostPlay = { policy?: readonly BrowserPolicyState[]; platform?: "mac" | "windows" };

/** A host with one matter and its one thread, whose header carries `tools`, with Settings, and the app feed that
 * plays `play.policy`'s reports once the view is in. */
function toolsHost(tools: ToolState[], play: HostPlay = {}): FakeTransport {
  const head = header();
  head.header!.tools = tools;
  const transport: FakeTransport = new FakeTransport({
    platform: play.platform ?? "mac",
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
        for (const state of play.policy ?? []) transport.appEvent({ browser_policy: { state } });
      }),
  });
  return transport;
}

type Show = "popover" | "tip" | { tip: string };

const entry = (
  id: string,
  words: WordsRef,
  tools: ToolState[],
  play?: HostPlay,
  show: Show = "popover",
): Entry => ({
  id,
  words,
  screen: "room/status",
  render: () => {
    const host = () => toolsHost(tools, play);
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
        tools={toolsSegment(lang, "mac", knownTools([web(), browser()]))}
      />
    </div>
  </RoomScopeProvider>
);

const NONE = [browser("browser_not_installed")];
const WINDOWS: HostPlay = { platform: "windows" };
const UNREAD = [browser("browser_policy_unreadable")];
/** A policy read that found nothing blocking: the row is ready for new conversations. */
const CLEAR: HostPlay = { policy: ["clear"] };

/** UET's tool line entries, each id written as it is: an id never moves when an entry is added or taken out. Retired
 * in round 3 (icons first, DESIGN.md §Icons): UET-02 to UET-08, UET-16, UET-17, UET-32 and UET-36. UET-37 (Download
 * stopped) is retired too: a cancelled download goes back to the offer with no line. UET-26's words
 * were Chrome ready, UET-13's Off in Settings and UET-20's Needs Chrome; each keeps its id for its new words. Retired
 * after #870's shell browser (the computer's own Chrome or Edge, no proxy, no slots, no download): UET-18, UET-19,
 * UET-21 to UET-25, UET-27 to UET-30, UET-34, UET-35 and UET-42. */
export const UET: Entry[] = [
  // the line's name and tooltip
  entry(
    "UET-01",
    lineWords("{tool}: {state}."),
    [web(), browser("browser_not_installed")],
    {},
    "tip",
  ),
  entry("UET-39", lineWords("on"), [web(), browser()], {}, "tip"),
  entry(
    "UET-40",
    lineWords("off, {reason}"),
    [web("locked_off"), browser("browser_not_installed")],
    {},
    "tip",
  ),
  // the rows: names, chips and reasons
  entry("UET-09", lineWords("Web search"), [web(), browser()]),
  entry("UET-10", lineWords("Browser"), [web(), browser()]),
  entry("UET-11", popWords("On"), [web(), browser()]),
  entry("UET-41", popWords("Off"), [web(), browser("off")]),
  entry("UET-12", lineWords("Can't search on this login"), [web("no_search_on_this_login")]),
  // beside the Off chip, in its row
  entry("UET-13", lineWords("In Settings"), [web("off"), browser("off")]),
  entry("UET-14", lineWords("Managed by firm"), [web("locked_off"), browser("locked_off")]),
  // no browser on the computer: what to install, by the OS, then in full under it
  entry("UET-20", lineWords("Install Chrome"), NONE),
  entry("UET-48", lineWords("Install Chrome or Edge"), NONE, WINDOWS),
  entry("UET-49", popWords("Install Google Chrome, then close and reopen Clauseo."), NONE),
  entry(
    "UET-50",
    popWords("Install Google Chrome or Microsoft Edge, then close and reopen Clauseo."),
    NONE,
    WINDOWS,
  ),
  // the firm's Chrome or Edge policy blocks every browser: the reason, then in full under it
  entry("UET-44", lineWords("Blocked by your firm"), [browser("browser_blocked_by_policy")]),
  entry(
    "UET-45",
    popWords(
      "Your firm doesn't allow apps to control a browser on this computer, so Clauseo can't use one for you.",
    ),
    [browser("browser_blocked_by_policy")],
  ),
  // the firm's browser policy couldn't be read: the reason, then in full under it, and Try again
  entry("UET-46", lineWords("Couldn't check browser settings"), [
    browser("browser_policy_unreadable"),
  ]),
  entry(
    "UET-47",
    popWords(
      "Clauseo couldn't check this computer's browser settings, so it won't use a browser for now. It checks again the next time it needs one.",
    ),
    UNREAD,
  ),
  // the foot
  entry("UET-15", popWords("Open Settings"), [web("off")]),
  entry(
    "UET-33",
    popWords("Changes apply to new conversations."),
    [web("off"), browser("off")],
    {},
    {
      tip: "tool-settings",
    },
  ),
  entry("UET-31", popWords("Try again"), UNREAD),
  // a read since found nothing blocking: the reason it keeps, then its tooltip shown from the keyboard
  entry("UET-26", popWords("Ready for new conversations"), UNREAD, CLEAR),
  entry("UET-43", popWords("The browser comes with your next conversation."), UNREAD, CLEAR, {
    tip: "tool-row-reason",
  }),
  {
    id: "UET-38",
    words: lineWords("{tool}: {state}."),
    screen: "room/status",
    render: statusFit,
  },
];
