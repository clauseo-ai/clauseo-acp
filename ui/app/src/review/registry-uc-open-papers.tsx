// UC's entries for the change note's press beside each paper it names (R24, UC-1205, UC-1206): a turn that changed a
// Word memo, a spreadsheet and a PDF, the note listing them one to a row, with Open in Word, Open in Excel and Open. The
// real room over FakeTransport, the wall listing the three papers with their file refs.
import { agentText, step, userMessage, uuid } from "../testing/stream-script.ts";
import type { PaperSummary, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { matterWords, shared } from "./words.ts";

const MEMO = "Advice on the Westlake lease, termination and damages.docx";
const SHEET = "Damages schedule, rent arrears to November.xlsx";
const PDF = "Exhibit C, notice of termination.pdf";

const QUESTION = userMessage(
  5,
  1,
  "Read the lease and the notice of termination, then write me an advice memo on whether Westlake can terminate " +
    "for the arrears, and put the arrears and the damages we can claim in a schedule I can check.",
);
const ANSWER = agentText(
  6,
  1,
  "The memo and the damages schedule are ready. Westlake can terminate, on **clause 14.2**.",
);
const CHANGED: ViewEntry = step(9, {
  files: [MEMO, SHEET, PDF].map((path) => ({
    path,
    change: path === MEMO ? "modified" : "added",
    size_bytes: 1200,
    may_include_lawyer_edits: false,
  })),
  checkpoint_after: uuid(1, 9),
});
const PAPERS: PaperSummary[] = [MEMO, SHEET, PDF].map((path, i) => ({
  file: uuid(i + 1, 8),
  path,
  size: 1200,
  modified: 1_730_000_000_000,
}));

const note = () => (
  <RoomOver host={() => threadHost({ entries: [QUESTION, ANSWER, CHANGED], papers: PAPERS })} />
);
const WINDOW = { width: 1440, height: 900 } as const;

export const UC_OPEN_PAPERS: Entry[] = [
  {
    id: "UC-1205",
    words: matterWords("Open in Excel"),
    screen: "room/thread",
    ...WINDOW,
    render: note,
  },
  { id: "UC-1206", words: shared("Open"), screen: "room/thread", ...WINDOW, render: note },
];
