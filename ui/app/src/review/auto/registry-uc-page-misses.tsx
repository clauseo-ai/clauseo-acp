// UC's entries for a page that couldn't open and a step Claude's safety check held back (UC-1185 to UC-1198,
// provisional; Windows tester item C): never told as "failed". The page's row gives the site and a plain reason, the
// folded line counts the pages ("Couldn't open 4 pages."), and a held-back step says so. In the real room over
// FakeTransport, the steps shaped as the agents send them (testing/step-lines.ts).
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { agentText, step, userMessage } from "../../testing/stream-script.ts";
import { fetchOf, heldOf, readOf, testerCard } from "../../testing/step-lines.ts";
import type { StepKey } from "@clauseo/components/lib/words-steps.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { RoomClicking, RoomOver } from "../states.tsx";
import { stepWords } from "../words.ts";

const QUESTION = userMessage(
  5,
  1,
  "Find the Delhi High Court's latest order on the bank's possession notice and tell me its date.",
);
const ANSWER = agentText(90, 1, "The order of 12 March 2024 stays the possession notice.");

const host = (calls: ToolCallEntry[]) => () =>
  threadHost({
    entries: [QUESTION, ...calls.map((tool_call) => ({ tool_call })), ANSWER, step(91)],
  });
const opened = (id: string, key: StepKey, calls: ToolCallEntry[]): Entry => ({
  id,
  words: stepWords(key),
  screen: "room/thread",
  render: () => <RoomClicking host={host(calls)} clicks={['[data-testid="step-group"] button']} />,
});
const folded = (id: string, key: StepKey, calls: ToolCallEntry[]): Entry => ({
  id,
  words: stepWords(key),
  screen: "room/thread",
  render: () => <RoomOver host={host(calls)} />,
});

const LIVELAW = "https://www.livelaw.in/high-court/delhi-high-court/";
const refused = () =>
  fetchOf("Claude", LIVELAW, "completed", "The server returned HTTP 403 Forbidden.");
const missing = () =>
  fetchOf(
    "Claude",
    "https://delhihighcourt.nic.in/orders/123",
    "failed",
    "Request failed with status code 404",
  );
const large = () =>
  fetchOf(
    "Claude",
    "https://www.sci.gov.in/judgment.pdf",
    "failed",
    "maxContentLength size of 10485760 exceeded",
  );
const unreachable = () =>
  fetchOf(
    "Claude",
    "https://ecourts.example.in/",
    "failed",
    "getaddrinfo ENOTFOUND ecourts.example.in",
  );
const slow = () =>
  fetchOf("ChatGPT", "https://www.barandbench.com/", "failed", "timeout of 60000ms exceeded");

export const UC_PAGE_MISSES: Entry[] = [
  opened("UC-1185", "Couldn't open {site}: {reason}.", testerCard()),
  opened("UC-1186", "Couldn't open {site}.", [fetchOf("Claude", LIVELAW, "failed", "")]),
  opened("UC-1187", "Couldn't open a page: {reason}.", [
    { ...missing(), raw_input: { inline: "{}" } },
  ]),
  opened("UC-1188", "Couldn't open a page.", [
    { ...fetchOf("Claude", LIVELAW, "failed", ""), raw_input: { inline: "{}" } },
  ]),
  opened("UC-1189", "the site refused", [refused()]),
  opened("UC-1190", "not found", [missing()]),
  opened("UC-1191", "too large", [large()]),
  opened("UC-1192", "couldn't reach the site", [unreachable()]),
  opened("UC-1193", "the site took too long", [slow()]),
  folded("UC-1194", "Couldn't open 1 page.", [readOf("Claude", "Plaint.docx"), refused()]),
  folded("UC-1195", "Couldn't open {n} pages.", testerCard()),
  folded("UC-1196", "1 held back by Claude's safety check.", [
    readOf("Claude", "Plaint.docx"),
    heldOf(),
  ]),
  folded("UC-1197", "{n} held back by Claude's safety check.", [
    readOf("Claude", "Plaint.docx"),
    heldOf(),
    heldOf(),
  ]),
  opened("UC-1198", "Held back by Claude's safety check", [heldOf()]),
];

export default UC_PAGE_MISSES;
