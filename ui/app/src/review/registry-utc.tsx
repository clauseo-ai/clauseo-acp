// UTC's copy review entries (the step group), apart from registry.tsx to keep that file under the size limit: each
// string in the real room over FakeTransport, the step group folded with the summary that says it, opened (pressed as
// the lawyer would) for a row's words, and live, with the working line, for what the running step is doing.
import type { ToolCallEntry, ToolCallStatus, ToolKind, ViewEntry } from "../transport/protocol.ts";
import { agentText, step, toolCall, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { stepLiveWords, stepWords, type WordsRef } from "./words.ts";
import type { StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { StepLiveKey } from "@clauseo/components/lib/words-steps-live.ts";

/** A local time today. */
const atLocal = (h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, m).getTime();
};
const QUESTION: ViewEntry = (() => {
  const asked = userMessage(5, 1, "What does the lease say about notice to the bank?");
  return { user_message: { ...asked.user_message!, ts: atLocal(16, 31) } };
})();
const ANSWER = agentText(90, 1, "Clause nine needs thirty days' written notice to the bank.");
const ENDED = step(91, { ts: atLocal(16, 32) });

/** What a call touches: a paper's path in the matter, the raw input's words, or a title. */
type Touch = { path?: string; raw?: object; title?: string; background?: boolean };
let next = 0;
function call(kind: ToolKind, status: ToolCallStatus = "completed", touch: Touch = {}): ViewEntry {
  next += 1;
  const entry: ToolCallEntry = {
    ...toolCall(10 + next, 300 + next, status).tool_call!,
    kind,
    title: touch.title ?? "",
    locations: touch.path === undefined ? [] : [{ inside: { path: touch.path } }],
    ...(touch.raw === undefined ? {} : { raw_input: { inline: JSON.stringify(touch.raw) } }),
    ...(touch.background === true ? { background: { steer: QUESTION.user_message!.message } } : {}),
  };
  return { tool_call: { ...entry, order: 10 + next } };
}
const many = (n: number, make: () => ViewEntry) => Array.from({ length: n }, make);

const LEASE = { path: "papers/2019/Lease deed.pdf" };
const SEARCH = { raw: { pattern: "notice period" } };
const SITE = { raw: { url: "https://indiankanoon.org/doc/1/" } };
const ONLINE = { raw: { query: "Section 138 NI Act" } };

/** The settled thread with these calls before the answer; `open` presses the group's line. */
function settled(calls: ViewEntry[], open: boolean) {
  const host = () => threadHost({ entries: [QUESTION, ...calls, ANSWER, ENDED] });
  return open
    ? () => <RoomClicking host={host} clicks={['[data-testid="step-group"] button']} />
    : () => <RoomOver host={host} />;
}

/** The running thread, its newest call in hand: the live group's line and the working line say it. */
const running = (calls: ViewEntry[]) => () => (
  <RoomOver host={() => threadHost({ entries: [QUESTION, ...calls], status: "running" })} />
);

const folded = (id: string, key: StepKey, calls: ViewEntry[]): Entry => ({
  id,
  words: stepWords(key),
  screen: "room/thread",
  render: settled(calls, false),
});
const opened = (id: string, key: StepKey, calls: ViewEntry[]): Entry => ({
  id,
  words: stepWords(key),
  screen: "room/thread",
  render: settled(calls, true),
});
const live = (id: string, words: WordsRef, calls: ViewEntry[]): Entry => ({
  id,
  words,
  screen: "room/thread",
  render: running(calls),
});
const now = (id: string, key: StepLiveKey, calls: ViewEntry[]) =>
  live(id, stepLiveWords(key), calls);

export const UTC: Entry[] = [
  // a row, its step done, once the group is opened
  opened("UTC-01", "Read {name}.", [call("read", "completed", LEASE)]),
  opened("UTC-02", "Read the papers.", [call("read")]),
  opened("UTC-03", "Changed {name}.", [call("edit", "completed", LEASE)]),
  opened("UTC-04", "Changed a paper.", [call("edit")]),
  opened("UTC-05", "Deleted {name}.", [call("delete", "completed", LEASE)]),
  opened("UTC-06", "Deleted a paper.", [call("delete")]),
  opened("UTC-07", "Moved {name}.", [call("move", "completed", LEASE)]),
  opened("UTC-08", "Moved a paper.", [call("move")]),
  opened("UTC-09", "Searched for '{query}'.", [call("search", "completed", SEARCH)]),
  opened("UTC-10", "Searched the papers.", [call("search")]),
  opened("UTC-11", "Searched online for '{query}'.", [call("fetch", "completed", ONLINE)]),
  opened("UTC-12", "Read {name} online.", [call("fetch", "completed", SITE)]),
  opened("UTC-13", "Looked online.", [call("fetch")]),
  opened("UTC-14", "Ran a task.", [call("execute")]),
  opened("UTC-15", "Thought it through.", [call("think")]),
  opened("UTC-16", "Did one other thing.", [call("other")]),
  // a row's state
  opened("UTC-17", "Waiting", [call("read", "completed", LEASE), call("edit", "pending", LEASE)]),
  // UTC-18 was the row's "In the background": that word is U7-26's, one id for it
  // the folded line, one and many for each kind
  folded("UTC-19", "1 paper read.", [call("read", "completed", LEASE)]),
  folded(
    "UTC-20",
    "{n} papers read.",
    many(4, () => call("read", "completed", LEASE)),
  ),
  folded("UTC-21", "1 paper changed.", [call("edit")]),
  folded(
    "UTC-22",
    "{n} papers changed.",
    many(2, () => call("edit")),
  ),
  folded("UTC-23", "1 paper deleted.", [call("delete")]),
  folded(
    "UTC-24",
    "{n} papers deleted.",
    many(2, () => call("delete")),
  ),
  folded("UTC-25", "1 paper moved.", [call("move")]),
  folded(
    "UTC-26",
    "{n} papers moved.",
    many(2, () => call("move")),
  ),
  folded("UTC-27", "Searched once.", [call("search", "completed", SEARCH)]),
  folded(
    "UTC-28",
    "Searched {n} times.",
    many(3, () => call("search", "completed", SEARCH)),
  ),
  folded("UTC-29", "Looked online once.", [call("fetch", "completed", SITE)]),
  folded(
    "UTC-30",
    "Looked online {n} times.",
    many(2, () => call("fetch", "completed", SITE)),
  ),
  folded("UTC-31", "Ran 1 task.", [call("execute")]),
  folded(
    "UTC-32",
    "Ran {n} tasks.",
    many(2, () => call("execute")),
  ),
  folded("UTC-33", "Planned the work.", [call("think")]),
  folded("UTC-34", "Did 1 other thing.", [call("other")]),
  folded(
    "UTC-35",
    "Did {n} other things.",
    many(2, () => call("other")),
  ),
  folded("UTC-36", "And 1 more.", [
    call("read"),
    call("read"),
    call("search"),
    call("edit"),
    call("fetch"),
  ]),
  folded("UTC-37", "And {n} more.", [
    call("read"),
    call("read"),
    call("search"),
    call("edit"),
    call("fetch"),
    call("move"),
  ]),
  // what failed or was stopped, kept on the folded line
  folded("UTC-38", "It failed.", [call("fetch", "failed", SITE)]),
  folded("UTC-39", "1 of them failed.", [
    call("read", "completed", LEASE),
    call("fetch", "failed", SITE),
  ]),
  folded("UTC-40", "{n} of them failed.", [
    call("read"),
    ...many(2, () => call("fetch", "failed", SITE)),
  ]),
  folded("UTC-41", "It was stopped.", [call("read", "cancelled", LEASE)]),
  folded("UTC-42", "1 of them was stopped.", [call("read"), call("read", "cancelled", LEASE)]),
  folded("UTC-43", "{n} of them were stopped.", [
    call("read"),
    ...many(2, () => call("read", "cancelled")),
  ]),
  // the live line's count
  live("UTC-44", stepWords("1 done so far."), [
    call("read", "completed", LEASE),
    call("search", "in_progress", SEARCH),
  ]),
  live("UTC-45", stepWords("{n} done so far."), [
    ...many(3, () => call("read", "completed", LEASE)),
    call("search", "in_progress", SEARCH),
  ]),
  // what the running step is doing: the live group's line and the working line
  now("UTC-46", "Reading {name}.", [call("read", "in_progress", LEASE)]),
  now("UTC-47", "Reading the papers.", [call("read", "in_progress")]),
  now("UTC-48", "Changing {name}.", [call("edit", "in_progress", LEASE)]),
  now("UTC-49", "Changing a paper.", [call("edit", "in_progress")]),
  now("UTC-50", "Deleting {name}.", [call("delete", "in_progress", LEASE)]),
  now("UTC-51", "Deleting a paper.", [call("delete", "in_progress")]),
  now("UTC-52", "Moving {name}.", [call("move", "in_progress", LEASE)]),
  now("UTC-53", "Moving a paper.", [call("move", "in_progress")]),
  now("UTC-54", "Searching for '{query}'.", [call("search", "in_progress", SEARCH)]),
  now("UTC-55", "Searching the papers.", [call("search", "in_progress")]),
  now("UTC-56", "Searching online for '{query}'.", [call("fetch", "in_progress", ONLINE)]),
  now("UTC-57", "Reading {name} online.", [call("fetch", "in_progress", SITE)]),
  now("UTC-58", "Looking online.", [call("fetch", "in_progress")]),
  now("UTC-59", "Running a task.", [call("execute", "in_progress")]),
  now("UTC-60", "Thinking it through.", [call("think", "in_progress")]),
  now("UTC-61", "Working on it.", [call("other", "in_progress")]),
  // a step a steer moved to the background, told on the folded line (its row's word is U7-26)
  folded("UTC-62", "It went on in the background.", [
    call("read", "completed", { ...LEASE, background: true }),
  ]),
  folded("UTC-63", "1 of them went on in the background.", [
    call("search", "completed", SEARCH),
    call("read", "completed", { ...LEASE, background: true }),
  ]),
  folded("UTC-64", "{n} of them went on in the background.", [
    call("search", "completed", SEARCH),
    ...many(2, () => call("read", "completed", { ...LEASE, background: true })),
  ]),
];
