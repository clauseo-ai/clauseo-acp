// UC's entries for a helper's card in the answer (H1 of the helpers design, round 24;
// helper-card.tsx): the card's time in whole minutes, how it ended, its name, and the working line
// while the lead waits on its helpers. The real room over FakeTransport on a matter with nine
// papers (helper-card-scenes.ts).
import type { Entry } from "./entry.ts";
import { threadHost, type ThreadState } from "./hosts.ts";
import {
  chatgptHelpers,
  claudeHelpers,
  settledHelpers,
  unnamedHelper,
} from "./helper-card-scenes.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import { stepLiveWords, stepWords } from "./words.ts";

const over = (state: () => ThreadState) => () => <RoomOver host={() => threadHost(state())} />;
/** The settled answer with the first card opened in place. */
const opened = () => (
  <RoomClicking
    host={() => threadHost(settledHelpers())}
    clicks={['[data-testid="helper-card"] button']}
  />
);

export const UC_HELPER_CARD: Entry[] = [
  {
    id: "UC-1115",
    words: stepWords("Working for {time}"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1116",
    words: stepWords("Took {time}"),
    screen: "room/thread",
    render: over(settledHelpers),
  },
  {
    id: "UC-1117",
    words: stepWords("Failed after {time}"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1118",
    words: stepWords("Stopped after {time}"),
    screen: "room/thread",
    render: over(settledHelpers),
  },
  {
    id: "UC-1119",
    words: stepWords("less than a minute"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1120",
    words: stepWords("{n} min"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1121",
    words: stepWords("Finished."),
    screen: "room/thread",
    render: over(settledHelpers),
  },
  {
    id: "UC-1122",
    words: stepWords("It ended without reporting back to {agent}."),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1123",
    words: stepWords("Before that: {what}"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  { id: "UC-1124", words: stepWords("By then: {what}"), screen: "room/thread", render: opened },
  {
    id: "UC-1125",
    words: stepLiveWords("Waiting for a helper to finish."),
    screen: "room/thread",
    render: over(chatgptHelpers),
  },
  {
    id: "UC-1126",
    words: stepLiveWords("Waiting for {n} helpers to finish."),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1127",
    words: stepWords("Paper reader"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1128",
    words: stepWords("Researcher"),
    screen: "room/thread",
    render: over(claudeHelpers),
  },
  {
    id: "UC-1129",
    words: stepWords("Helper {n}"),
    screen: "room/thread",
    render: over(unnamedHelper),
  },
];
