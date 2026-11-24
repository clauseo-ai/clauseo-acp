// UC-1180's copy review entry: the rail's row for a new conversation before its first message gives it a title, said
// as "Not started yet", muted (the + New conversation button beside it keeps its words). The real room over
// FakeTransport at a wide window, the matter open with two other conversations under it, New conversation pressed.
import type { MatterSummary } from "../../transport/protocol.ts";
import { FakeTransport } from "../../transport/fake-transport.ts";
import { fakeNext, railStates } from "../../room/fake-next.ts";
import type { ThreadInfo } from "../../room/host-next.ts";
import type { Entry } from "../entry.ts";
import { MATTER } from "../hosts.ts";
import { RoomAfter } from "../states.tsx";
import { matterWords } from "../words.ts";

/** A wide window, where the rail stands beside the work. */
const WIDE = { width: 1512, height: 945 } as const;

const SCRATCH: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000099",
  name: "Scratch",
  kind: "scratch",
};

const thread = (n: number, title: string, agent = "claude-acp"): ThreadInfo => ({
  thread: `00000000-0000-7000-8000-00000000c${n}00`,
  matter: MATTER.matter,
  agent,
  status: "idle",
  seen: true,
  title,
});

const THREADS = [
  thread(1, "Waiting period and pre-existing conditions"),
  thread(2, "Maternity cover under the 2026 renewal", "codex"),
];

const host = () =>
  new FakeTransport({
    next: fakeNext({
      matters: () => railStates(MATTER, SCRATCH, Date.now()),
      threads: (m) => (m === MATTER.matter ? THREADS : []),
    }),
  });

const UC_NEW_CONVERSATION: Entry[] = [
  {
    id: "UC-1180",
    words: matterWords("Not started yet"),
    screen: "matters/new-conversation",
    ...WIDE,
    render: () => <RoomAfter host={host} steps={[{ press: "new-conversation" }]} />,
  },
];

export default UC_NEW_CONVERSATION;
