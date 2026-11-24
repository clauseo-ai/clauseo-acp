// UC's entry for the working line while the agent waits to retry its service (host #1335's HeaderEntry.retrying,
// UC-1377, provisional): Clauseo's words that the plan can't reach its service, in the real room over FakeTransport,
// the agent's own title and details in the header and never on screen.
import { agentText, userMessage } from "../../testing/stream-script.ts";
import type { Entry } from "../entry.ts";
import { threadHost } from "../hosts.ts";
import { RoomOver } from "../states.tsx";
import { matterWords } from "../words.ts";

const QUESTION = userMessage(
  5,
  1,
  "Read the lease and the landlord's notice, and tell me whether the notice gave the three months clause 18 asks for.",
);
const PARTIAL = agentText(6, 1, "Reading the lease: clause 18 sets the notice period.");

const host = () =>
  threadHost({
    entries: [QUESTION, PARTIAL],
    status: "running",
    retrying: {
      since: Date.UTC(2026, 10, 25, 11, 0),
      title: "Retrying Claude, attempt 2 of 10.",
      details: "HTTP 529 (overloaded). Next attempt in 8s.",
    },
  });

const ENTRIES: Entry[] = [
  // Claude's answer running while the agent waits to retry: the busy line in place of "Answering.", no pen
  {
    id: "UC-1377",
    words: matterWords("{name} isn't getting through to its service. It's trying again."),
    screen: "room/thread",
    render: () => <RoomOver host={host} />,
  },
];
export default ENTRIES;
