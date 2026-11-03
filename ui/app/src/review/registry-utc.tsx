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

const LEASE = { path: "papers/2019/Lease deed.pdf" };
const SEARCH = { raw: { pattern: "notice period" } };
const SITE = { raw: { url: "https://indiankanoon.org/doc/1/" } };
const ONLINE = { raw: { query: "Section 138 NI Act" } };
// the matter's papers as the agent names them, by their full paths (the row shows the file's name, its folder on
// hover), and what it searched for: the frames with more than one step read like the prototype's list
const SHARMA = "/Users/x/Matters/Sharma";
const paper = (name: string): Touch => ({ raw: { file_path: `${SHARMA}/${name}` } });
const COMPLAINT = paper("Complaint.pdf");
const FORM = paper("Form DIR-12.pdf");
const MANDATE = paper("Bank mandate.pdf");
const MINUTES = paper("Board minutes, 3 March 2023.pdf");
const REPLY = paper("Reply to the complaint.docx");
const ANNEXURE = paper("Annexure P-3.pdf");
const sought = (pattern: string): Touch => ({ raw: { pattern } });
const LIVELAW = { raw: { url: "https://www.livelaw.in/supreme-court/" } };
const task = (description: string): Touch => ({ raw: { command: "…", description } });
const titled = (title: string): Touch => ({ title });

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
  opened("UTC-17", "Waiting", [
    call("read", "completed", COMPLAINT),
    call("edit", "pending", REPLY),
  ]),
  // a row that failed or was stopped: what it set out to do, in the past
  opened("UTC-65", "Tried to read {name}.", [call("read", "failed", LEASE)]),
  opened("UTC-66", "Tried to read the papers.", [call("read", "failed")]),
  opened("UTC-67", "Tried to change {name}.", [call("edit", "failed", LEASE)]),
  opened("UTC-68", "Tried to change a paper.", [call("edit", "cancelled")]),
  opened("UTC-69", "Tried to delete {name}.", [call("delete", "failed", LEASE)]),
  opened("UTC-70", "Tried to delete a paper.", [call("delete", "failed")]),
  opened("UTC-71", "Tried to move {name}.", [call("move", "failed", LEASE)]),
  opened("UTC-72", "Tried to move a paper.", [call("move", "failed")]),
  opened("UTC-73", "Tried to search for '{query}'.", [call("search", "failed", SEARCH)]),
  opened("UTC-74", "Tried to search the papers.", [call("search", "failed")]),
  opened("UTC-75", "Tried to search online for '{query}'.", [call("fetch", "failed", ONLINE)]),
  opened("UTC-76", "Tried to read {name} online.", [call("fetch", "failed", SITE)]),
  opened("UTC-77", "Tried to look online.", [call("fetch", "failed")]),
  opened("UTC-78", "Tried to run a task.", [call("execute", "failed")]),
  opened("UTC-79", "Tried to think it through.", [call("think", "cancelled")]),
  opened("UTC-80", "Tried one other thing.", [call("other", "failed")]),
  opened("UTC-81", "Tried: {title}.", [call("other", "cancelled", titled("Check the cause list"))]),
  // UTC-18 was the row's "In the background": that word is U7-26's, one id for it
  // the folded line, one and many for each kind
  folded("UTC-19", "1 paper read.", [call("read", "completed", LEASE)]),
  folded(
    "UTC-20",
    "{n} papers read.",
    [COMPLAINT, FORM, MANDATE, MINUTES].map((p) => call("read", "completed", p)),
  ),
  folded("UTC-21", "1 paper changed.", [call("edit", "completed", REPLY)]),
  folded("UTC-22", "{n} papers changed.", [
    call("edit", "completed", REPLY),
    call("edit", "completed", paper("List of dates.docx")),
  ]),
  folded("UTC-23", "1 paper deleted.", [call("delete", "completed", paper("Draft notes.docx"))]),
  folded("UTC-24", "{n} papers deleted.", [
    call("delete", "completed", paper("Draft notes.docx")),
    call("delete", "completed", paper("Old reply.docx")),
  ]),
  folded("UTC-25", "1 paper moved.", [call("move", "completed", ANNEXURE)]),
  folded("UTC-26", "{n} papers moved.", [
    call("move", "completed", ANNEXURE),
    call("move", "completed", paper("Annexure P-4.pdf")),
  ]),
  folded("UTC-27", "Searched once.", [call("search", "completed", SEARCH)]),
  folded(
    "UTC-28",
    "Searched {n} times.",
    ["S.M.S. Pharmaceuticals", "Pooja Ravinder Devidasani", "Sunita Palita"].map((q) =>
      call("search", "completed", sought(q)),
    ),
  ),
  folded("UTC-29", "Looked online once.", [call("fetch", "completed", SITE)]),
  folded("UTC-30", "Looked online {n} times.", [
    call("fetch", "completed", SITE),
    call("fetch", "completed", LIVELAW),
  ]),
  folded("UTC-31", "Ran 1 task.", [call("execute", "completed", task("Count the pages"))]),
  folded("UTC-32", "Ran {n} tasks.", [
    call("execute", "completed", task("Count the pages")),
    call("execute", "completed", task("Number the paper book")),
  ]),
  folded("UTC-33", "Planned the work.", [call("think")]),
  folded("UTC-34", "Did 1 other thing.", [
    call("other", "completed", titled("Ask the court's records")),
  ]),
  folded("UTC-35", "Did {n} other things.", [
    call("other", "completed", titled("Ask the court's records")),
    call("other", "completed", titled("Check the cause list")),
  ]),
  folded("UTC-36", "And 1 more.", [
    call("read", "completed", COMPLAINT),
    call("read", "completed", FORM),
    call("search", "completed", sought("S.M.S. Pharmaceuticals")),
    call("edit", "completed", REPLY),
    call("fetch", "completed", SITE),
  ]),
  folded("UTC-37", "And {n} more.", [
    call("read", "completed", COMPLAINT),
    call("read", "completed", FORM),
    call("search", "completed", sought("S.M.S. Pharmaceuticals")),
    call("edit", "completed", REPLY),
    call("fetch", "completed", SITE),
    call("move", "completed", ANNEXURE),
  ]),
  // what failed or was stopped, kept on the folded line
  // UTC-38 and UTC-41 ("It failed.", "It was stopped.") are gone: a group of one that failed says what it tried, once
  folded("UTC-39", "1 of them failed.", [
    call("read", "completed", COMPLAINT),
    call("fetch", "failed", SITE),
  ]),
  folded("UTC-40", "{n} of them failed.", [
    call("read", "completed", COMPLAINT),
    call("fetch", "failed", SITE),
    call("fetch", "failed", LIVELAW),
  ]),
  folded("UTC-42", "1 of them was stopped.", [
    call("read", "completed", COMPLAINT),
    call("read", "cancelled", FORM),
  ]),
  folded("UTC-43", "{n} of them were stopped.", [
    call("read", "completed", COMPLAINT),
    call("read", "cancelled", FORM),
    call("read", "cancelled", MANDATE),
  ]),
  // the live line's count
  live("UTC-44", stepWords("1 done so far."), [
    call("read", "completed", COMPLAINT),
    call("search", "in_progress", SEARCH),
  ]),
  live("UTC-45", stepWords("{n} done so far."), [
    ...[COMPLAINT, FORM, MANDATE].map((p) => call("read", "completed", p)),
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
    call("read", "completed", { ...COMPLAINT, background: true }),
  ]),
  folded("UTC-64", "{n} of them went on in the background.", [
    call("search", "completed", SEARCH),
    call("read", "completed", { ...COMPLAINT, background: true }),
    call("read", "completed", { ...FORM, background: true }),
  ]),
];
