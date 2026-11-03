// The probe build's page (vite.config.ts's probe mode, `pnpm build:probe`): the room over FakeTransport, with one
// matter (or none, with ?matters=none, the empty rail; or with ?rail=states, matters showing every rail state: working,
// a question, a permission, an answer ready, stopped, the limit with and without a reset, a moved folder, pinned, and
// each date group), its papers, and the scripted stream (testing/stream-script.ts) played to whatever subscribes; with
// ?thread=permission the stream ends, before its step, with the agent's permission requests: a pending prompt on a
// paper above one answered once and done; with ?confirm=1 as well, the host holds every answer, so the probe can press
// Always for this matter and find the prompt confirming (the OS line up, the answers waiting). The page's query sets its display for this page only (theme=night,
// lang=hi, lite=1, motion=reduced, size=18), as the WebKit probe (scripts/webkit-probe.swift) asks for it, and
// window.__clauseoDisplay and __clauseoAxe are set (dev-hooks.ts).
// With ?versions=panel, band, undo, restore, wait, newest or word, the room has Versions open over the matter's history
// (probe-states.tsx): the timeline with its kinds of row, a past version selected, its undo or restore dialog, the
// conversation running, the newest turn picked, or its undo refused because Word holds the file.
// With ?word=lock, general, open, tile or tile-top (word/probe-states.ts), the open conversation's last turn changed papers and
// the wall lists two Word documents: the change note's Undo refused with the lock (Word's lines for one Word document,
// or the general line for two files), the note's "Open in Word", or the wall's Word tile, for its tooltip.
// With ?thread=commands, the thread is idle with the agent's commands in its header (COMMANDS below), so the composer's
// `/` menu has items, and no turn runs; window.__clauseoProbeCalls is the fake transport's calls, for the page to read
// what was sent. With ?papers=mixed as well, the matter's papers add MIXED's eight (Devanagari, Tamil, mixed scripts,
// brackets, an `@` in a name), so one page has an idle composer, its commands and these papers for `@`. With
// ?papers=hindi, the wall holds HINDI's five long Devanagari names in place of the five, for its wrapping. With
// ?thread=hindi, the thread is a question asked in Hindi and a recorded Hindi answer (testing/hindi-answer.ts), finished,
// for screenshots of Devanagari tables, ₹ and bold in the room. With ?thread=shaping, the thread is a question and a
// fixed answer (testing/shaping-answer.ts), finished: the cells in every block type, for the Devanagari shaping audit's
// 1a and 1d. Not shipped.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App.tsx";
import type {
  AvailableCommand,
  CommandOutcome,
  HostEvent,
  MatterSummary,
  ViewPatchBody,
} from "../transport/protocol.ts";
import { type FakeCall, FakeTransport } from "../transport/fake-transport.ts";
import { HINDI_ANSWER, HINDI_QUESTION } from "../testing/hindi-answer.ts";
import { SHAPING_ANSWER, SHAPING_QUESTION } from "../testing/shaping-answer.ts";
import {
  batch,
  chosen,
  editCall,
  agentText,
  event,
  header,
  MATTER,
  permission,
  reset,
  scriptedTurn,
  THREAD,
  upsert,
  uuid,
  userMessage,
} from "../testing/stream-script.ts";
import { installDevHooks, queryPatch } from "./dev-hooks.ts";
import { detailed, fakeNext, railStates } from "./fake-next.ts";
import type { MatterInfo, PaperSummary } from "./host-next.ts";
import { probeThreads } from "./probe-threads.ts";
import { connectRoom } from "./start.ts";
import { versionsHost, versionsPageOf } from "./versions/probe-states.tsx";
import { LOCKED, wordFiles, wordHost, wordPageOf } from "./word/probe-states.ts";

const FILES = [
  "Notice to the bank.pdf",
  "Reply to the notice.docx",
  "Board minutes, 2 March 2001.pdf",
  "Complaint under section 138.docx",
  "Bank statement, March 2001.pdf",
];

/** ?papers=mixed: names the `@` menu's real-engine rows type against (G1a's M5), after FILES, all under papers/:
 * Devanagari (one with a space), Tamil, English with Devanagari, English with brackets, and one with an `@`. */
const MIXED = [
  "वकालतनामा.pdf",
  "अनुबंध की प्रति.docx",
  "शपथपत्र.pdf",
  "தீர்ப்பு.pdf",
  "ஒப்பந்தம்.docx",
  "Annexure A अनुलग्नक.pdf",
  "Exhibit (P-3) [certified].pdf",
  "Letter to ravi@sharmalaw.pdf",
];

/** ?papers=hindi: long Devanagari names on the wall (Hindi audit 3b), each one the wrapping has to keep whole: spaces,
 * a comma and Latin digits; the nukta letter क़ as filed (U+0958, which NFC splits); one compound with no space, so the
 * title wraps inside a word, through conjuncts (त्त, त्र, क्ष, ग्न); श्र, ज्ञ, ष्ट्र and a reph (र्ज़). */
const HINDI = [
  "अनुबंध की प्रमाणित प्रति, धारा 138 के अंतर्गत नोटिस का उत्तर.pdf",
  "\u0958ानूनी नोटिस.docx",
  "प्रत्युत्तरशपथपत्रक्षेत्राधिकारसंलग्नक.pdf",
  "श्रीमती ज्ञानवती बनाम राष्ट्रीय कृषि बैंक, अपील संख्या 2481.pdf",
  "परक्राम्य लिखत अधिनियम के अंतर्गत शिकायत की अर्ज़ी.docx",
];

const page = new URLSearchParams(location.search);
const mixedPapers = page.get("papers") === "mixed";
const hindiPapers = page.get("papers") === "hindi";

/** ?word=…: the Word pages (word/probe-states.ts), in place of the scripted stream, with the petition on the wall. */
const wordPage = wordPageOf(page.get("word"));
const wording = wordPage === null ? null : wordHost(wordPage);

const papers: PaperSummary[] = (
  wordPage !== null
    ? wordFiles(FILES, wordPage)
    : mixedPapers
      ? [...FILES, ...MIXED]
      : hindiPapers
        ? HINDI
        : FILES
).map((name, i) => ({
  file: uuid(i + 1, 8),
  path: `papers/${name}`,
  size: 40_000 + i * 1_000,
  modified: 1_730_380_000_000 + i * 60_000,
}));

/** ?thread=permission: the turn stops short of its step, waiting on the lawyer, at a pending prompt to save a new
 * version of a paper, above one answered once whose call completed. */
const asking = page.get("thread") === "permission";
/** ?confirm=1: answer_permission never answers, as while the OS's own confirm is up. */
const holding = asking && page.get("confirm") === "1";
/** ?thread=commands: an idle thread whose header lists COMMANDS, and no scripted turn, so the composer is not busy. */
const commanding = page.get("thread") === "commands";
/** ?thread=hindi: an idle thread, a question in Hindi and Claude's recorded Hindi answer (Hindi audit 5c). */
const hindiThread = page.get("thread") === "hindi";
/** ?thread=shaping: an idle thread, a question and the fixed answer with the cells in every block type (Devanagari
 * shaping audit 1a, 1d). */
const shapingThread = page.get("thread") === "shaping";
/** The pages whose matter lists one idle thread, which the room opens and subscribes to. */
const idleThread = commanding || hindiThread || shapingThread;

/** The agent's commands on ?thread=commands, in the order the composer's `/` menu lists them: the first six are the
 * spike's fixture (spike/src/mentions/fixtures.json), the last two match a name typed in Devanagari or Tamil. */
const COMMANDS: AvailableCommand[] = [
  { name: "review", description: "Review the open document" },
  { name: "redline", description: "Redline against the last version" },
  { name: "cite", description: "Insert a citation" },
  { name: "research", description: "Research a point of law", input_hint: "the question" },
  { name: "plan", description: "Make a plan before acting" },
  { name: "web", description: "Search the web", input_hint: "query to search for" },
  { name: "न्यायालय", description: "Court (a Devanagari name, test only)" },
  { name: "நீதிமன்றம்", description: "Court (a Tamil name, test only)" },
];

/** The agent's permission requests the turn ends with on ?thread=permission: a pending prompt above an answered one. */
const PERMISSIONS: ViewPatchBody[] = [
  upsert(editCall(100, 301, "pending")),
  upsert(permission(101, 301)),
  upsert(editCall(102, 302, "completed")),
  upsert(permission(103, 302, chosen("allow_once"))),
];

/** The scripted turn after the prompt, in batches of 25 bodies, as the bridge would deliver them. */
function stream(): HostEvent[] {
  const turn = scriptedTurn().bodies;
  const bodies: ViewPatchBody[] = hindiThread
    ? [reset([userMessage(2, 1, HINDI_QUESTION), agentText(3, 100, HINDI_ANSWER)], header("idle"))]
    : shapingThread
      ? [
          reset(
            [userMessage(2, 1, SHAPING_QUESTION), agentText(3, 100, SHAPING_ANSWER)],
            header("idle"),
          ),
        ]
      : commanding
        ? [
            reset(
              [
                userMessage(2, 1, "When did the bank reply?"),
                agentText(3, 100, "The bank replied on 12 March 2001."),
              ],
              header("idle", COMMANDS),
            ),
          ]
        : asking
          ? [
              reset([userMessage(2, 1, "Tighten the notice to the bank.")], header("needs_you")),
              ...turn.slice(0, -1),
              ...PERMISSIONS,
            ]
          : [reset([userMessage(2, 1, "When did the bank reply?")]), ...turn];
  const events: HostEvent[] = [];
  for (let at = 0; at < bodies.length; at += 25) {
    events.push(event(batch(at + 1, bodies.slice(at, at + 25))));
  }
  return events;
}

/** ?matters=none: a host with no matters, so the title says so ("No matters yet"), the rail is empty and the wall
 * stays empty. */
const noMatters = page.get("matters") === "none";

const OPEN: MatterSummary = { matter: MATTER, name: "Sharma v. State of Delhi", kind: "folder" };
const SCRATCH: MatterSummary = { matter: uuid(99, 2), name: "Scratch", kind: "scratch" };

const matters: MatterInfo[] = noMatters
  ? []
  : page.get("rail") === "states"
    ? railStates(OPEN, SCRATCH, Date.now())
    : [detailed(OPEN), detailed(SCRATCH)];

/** ?versions=…: the matter's Versions (probe-states.tsx), in place of the scripted stream. */
const versionsPage = versionsPageOf(page.get("versions"));
const versioning = versionsPage === null ? null : versionsHost(versionsPage);
/** The page's host for Versions or Word, where it has one. */
const staged = versioning ?? wording;

const transport = new FakeTransport({
  platform: "mac",
  // on the pages whose undo meets a lock, every undo_step is refused with matter.file_locked
  dispatch: (envelope): CommandOutcome => {
    if (staged?.refusesUndo === true && envelope.command.undo_step !== undefined) throw LOCKED;
    return "done";
  },
  query: (query) => {
    const answer = staged?.query(query);
    if (answer !== undefined) return answer;
    // ?thread=permission: the turn's thread, so the room subscribes to it and shows the prompts
    if (asking && typeof query === "object" && query.list_threads !== undefined)
      return {
        threads: [{ thread: THREAD, matter: MATTER, agent: "claude-acp", status: "needs_you" }],
      };
    // ?thread=commands, ?thread=hindi and ?thread=shaping: the same thread, idle, so the room subscribes to it and
    // shows its header's commands, the Hindi answer or the shaping answer
    if (idleThread && typeof query === "object" && query.list_threads !== undefined)
      return {
        threads: [{ thread: THREAD, matter: MATTER, agent: "claude-acp", status: "idle" }],
      };
    throw new Error(`the probe's host is a NextHost: it answers no ${JSON.stringify(query)}`);
  },
  subscribe: (streamId) => {
    queueMicrotask(() => {
      if (staged !== null) {
        for (const message of staged.subscribe(streamId)) transport.deliver(message);
        return;
      }
      for (const message of stream()) transport.deliver(message);
    });
  },
  // the thread a matter lists: the host's next_query answers list_threads (the `query` above answers only the legacy
  // HostQuery form), so ?thread=permission lists its thread waiting on the lawyer, and ?thread=commands its idle one
  next: fakeNext({
    matters: () => matters,
    papers,
    location: "Documents/Clauseo",
    threads: () =>
      probeThreads(
        { asking, commanding: idleThread, shaping: shapingThread },
        wording?.threads ?? [],
      ),
  }),
});

if (holding) transport.answerPermission = () => new Promise<void>(() => {});

declare global {
  interface Window {
    /** ?thread=commands: the calls the fake transport has had (sends among them), for the probe to read. */
    __clauseoProbeCalls?: FakeCall[];
  }
}
// the fake's calls, for the probe to read what the composer sent: only on ?thread=commands, so no other page has them
if (commanding) window.__clauseoProbeCalls = transport.calls;

// the page's query over the settings, before the first render, so night never starts as day; and the hooks the
// by-hand checks use, so the WebKit probe can run window.__clauseoAxe() as it would run in the app
installDevHooks(connectRoom(transport, queryPatch(location.search)));

const container = document.getElementById("root");
if (container === null) throw new Error("index.html has no #root element");
createRoot(container).render(
  <StrictMode>
    <App transport={transport} />
  </StrictMode>,
);
