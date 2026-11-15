// "Always for this matter" on Claude's permission requests, the lawyer's side (UC-500 to UC-509):
// a matter's kept permissions in the real rail over FakeTransport, and the once-only notices in
// the room's thread. The matter's menu offers Kept permissions…; pressed, the list opens over the
// room with one rule of each kind (a command, a file inside the matter, one outside it, a site, an
// MCP tool), or none. Each notice says an "Always" kept nothing, so the request was allowed this
// once only, and why.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { kp, type KeptKey } from "@clauseo/components/lib/words-kept.ts";
import type { ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type { KeptPermission, NotKeptReason, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { RoomTriage, type Act } from "./triage-states.tsx";
import { threadWords, type WordsRef } from "./words.ts";

/** The rail open beside the work, every row in view. */
const RAIL = { width: 1280, height: 1100 } as const;

const words = (key: KeptKey): WordsRef => ({
  file: "lib/words-kept.ts",
  key,
  text: (lang: Lang) => kp(lang, key),
  later: kp.hi[key] === null,
});

/** Several lines of one id, joined with " / " as the approved page writes them. */
const joined = (keys: readonly KeptKey[]): WordsRef => ({
  file: "lib/words-kept.ts",
  key: keys.join(" / "),
  text: (lang: Lang) => keys.map((key) => kp(lang, key)).join(" / "),
  later: keys.some((key) => kp.hi[key] === null),
});

const MENU: readonly Act[] = [
  { press: '[aria-label="Actions for Sharma v. State of Delhi"]' },
  { focus: '[role="menuitem"]' },
];
const LIST: readonly Act[] = [
  { press: '[aria-label="Actions for Sharma v. State of Delhi"]' },
  { item: "Kept permissions" },
];
/** The first Remove focused: its name for assistive technology is the row's own words. */
const FOCUSED: readonly Act[] = [...LIST, { focus: '[data-testid="kept-permission-remove"]' }];

const KEPT: readonly KeptPermission[] = [
  { rule: "Bash(npm test)", kind: "run", subject: "npm test" },
  {
    rule: "Edit(//Users/asha/Matters/Sharma/Drafts/reply.docx)",
    kind: "edit",
    subject: "Drafts/reply.docx",
  },
  {
    rule: "Read(//Users/asha/Downloads/judgment.pdf)",
    kind: "read",
    subject: "/Users/asha/Downloads/judgment.pdf",
  },
  { rule: "WebFetch(domain:indiankanoon.org)", kind: "site", subject: "indiankanoon.org" },
  {
    rule: "mcp__court-search__find_case",
    kind: "tool",
    subject: "find_case",
    server: "court-search",
  },
];

const listed = (
  id: string,
  ref: WordsRef,
  acts: readonly Act[],
  kept: readonly KeptPermission[] = KEPT,
): Entry => ({
  id,
  words: ref,
  screen: "matters/kept-permissions",
  ...RAIL,
  render: () => <RoomTriage acts={acts} kept={kept} />,
});

/** One once-only line per reason. */
const NOT_KEPT_LINE: Record<Exclude<NotKeptReason, "unknown">, ThreadKey> = {
  no_rule: "Allowed this once only: Clauseo can't keep “Always” for this kind of request.",
  refused:
    "Allowed this once only: Clauseo keeps “Always” only for one exact command, site or file.",
  outside_matter:
    "Allowed this once only: Clauseo keeps “Always” only for files in this matter's folder.",
  full: "Allowed this once only: This matter keeps as many permissions as it can. Remove some from Kept permissions to keep more.",
};

/** The lawyer's question, then the host's notice that an Always kept nothing, in a thread of
 * claude-acp. */
const thread =
  (reason: NotKeptReason): (() => ReactNode) =>
  () => {
    const notice: ViewEntry = {
      notice: {
        order: 6,
        event: uuid(1, 10),
        level: "info",
        text: "Allowed this once only.",
        ts: 6000,
        kind: { always_not_kept: { reason } },
      },
    };
    return (
      <RoomOver
        host={() =>
          threadHost({
            entries: [userMessage(5, 1, "Run the tests in the tools folder."), notice],
          })
        }
      />
    );
  };

export const CLAUDE_ALWAYS: Entry[] = [
  listed("UC-500", words("Kept permissions…"), MENU),
  listed("UC-501", words("Kept permissions"), LIST),
  listed(
    "UC-502",
    words("Claude doesn't ask before these in this matter: you chose “Always for this matter”."),
    LIST,
  ),
  listed(
    "UC-503",
    joined([
      "Run: {command}",
      "Edit: {file}",
      "Read: {file}",
      "Any page on {site}",
      "Use tool: {server} {tool}",
    ]),
    LIST,
  ),
  listed("UC-504", joined(["Remove", "Remove {permission}", "Close"]), FOCUSED),
  listed("UC-505", words("Nothing is kept for this matter. Claude asks each time."), LIST, []),
  ...(
    [
      ["UC-506", "no_rule"],
      ["UC-507", "refused"],
      ["UC-508", "outside_matter"],
      ["UC-509", "full"],
    ] as const
  ).map(([id, reason]): Entry => ({
    id,
    words: threadWords(NOT_KEPT_LINE[reason]),
    screen: "room/thread",
    render: thread(reason),
  })),
];
