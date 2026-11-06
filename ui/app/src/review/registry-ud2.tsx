// UD2's copy review entries for the room after sign-in (UD2-01 to UD2-31, no UD2-05): the scan's lines under New
// matter's folder line, the first read's line and its question in the thread, and the expired sign-in card in the
// composer's notice line, each in the real room over a FakeTransport in the state that shows it. Every line is new or
// slotted, English only (hi: later), provisional until Rohan's batch review. Plans' ids are part B's.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { frd, type FirstReadKey } from "@clauseo/components/lib/words-first-read.ts";
import { sc, type ScanKey } from "@clauseo/components/lib/words-scan.ts";
import { sx, type SigninExpiredKey } from "@clauseo/components/lib/words-signin-expired.ts";
import { seedMarks } from "../room/first-read/marks.ts";
import {
  agentText,
  batch,
  event,
  header,
  reset,
  step,
  THREAD,
  TURN,
  userMessage,
} from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  FolderPlace,
  FolderScanned,
  QueueHoldReason,
  SetupEvent,
  SignIn,
  ThreadStatus,
  ViewEntry,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { RoomPicking } from "./room-picking.tsx";
import { matterWords, type WordsRef } from "./words.ts";

const scanWords = (key: ScanKey): WordsRef => ({
  file: "lib/words-scan.ts",
  key,
  text: (lang: Lang) => sc(lang, key),
  later: sc.hi[key] === null,
});
const readWords = (key: FirstReadKey): WordsRef => ({
  file: "lib/words-first-read.ts",
  key,
  text: (lang: Lang) => frd(lang, key),
  later: frd.hi[key] === null,
});
const expiredWords = (key: SigninExpiredKey): WordsRef => ({
  file: "lib/words-signin-expired.ts",
  key,
  text: (lang: Lang) => sx(lang, key),
  later: sx.hi[key] === null,
});

/* ---------- the scan ---------- */

const FOLDER = "review-folder";
const scanned = (over: Partial<FolderScanned>): FolderScanned => ({
  folder: FOLDER,
  files: 24,
  bytes: 38_200_000,
  place: "local",
  cloud_only: 0,
  done: false,
  cancelled: false,
  ...over,
});
const SYNCED: FolderPlace = { synced: { service: "one_drive" } };

/** New matter over a host whose picker gives a folder and whose scan plays `event`. */
const scanning = (event: FolderScanned) => () =>
  new FakeTransport({
    query: (query) =>
      typeof query === "object" && "folder_support" in query
        ? { folder_support: {} }
        : { matters: [MATTER] },
    pickFolder: () => ({ folder: FOLDER, display: "/Users/rmehra/Matters/Rathi Steels" }),
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    scan: () => [event],
  });
const scan = (event: FolderScanned) => () => <RoomPicking host={scanning(event)} />;

const COUNTING = scanned({ now_reading: "Board minutes/Board minutes.docx" });

/* ---------- the room's thread: the first read and the expired card ---------- */

const AT = (() => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), 16, 32).getTime();
})();

type Scene = {
  entries: ViewEntry[];
  status?: ThreadStatus;
  held?: boolean;
  /** Why the host holds the queue (#537's queue_held_reason). */
  reason?: QueueHoldReason;
  title?: string;
  /** The Claude plan's sign-in, on the setup wire (the first run, which gives the card). */
  signIn?: SignIn;
};

/** The room with the one matter and its one thread, over the setup wire. */
function sceneHost(s: Scene): () => FakeTransport {
  return () => {
    const head = header(s.status === "running" ? "running" : "idle");
    head.header!.status = s.status ?? "idle";
    if (s.held === true) head.header!.queue_held = true;
    if (s.reason !== undefined) head.header!.queue_held_reason = s.reason;
    if (s.title !== undefined) head.header!.title = s.title;
    const events: SetupEvent[] = [
      { setup_info: { keys_available: true } },
      { agent_install_progress: { agent: "claude-acp", state: { ready: { version: "0.1.0" } } } },
      {
        sign_in_changed: {
          agent: "claude-acp",
          state: s.signIn ?? { signed_in: { method: "plan", source: "own", at: 1 } },
        },
      },
    ];
    const transport: FakeTransport = new FakeTransport({
      query: (query) => {
        if (query === "list_matters") return { matters: [MATTER] };
        if (query === "get_settings") throw new Error("not scripted");
        if (typeof query === "object" && query.list_threads !== undefined)
          return {
            threads: [
              {
                thread: THREAD,
                matter: MATTER.matter,
                agent: "claude-acp",
                status: "idle",
                ...(s.title === undefined ? {} : { title: s.title }),
                ...(s.held === true ? { queue_held: true } : {}),
              },
            ],
          };
        throw new Error(`not scripted: ${JSON.stringify(query)}`);
      },
      subscribe: (stream) =>
        queueMicrotask(() =>
          transport.deliver(event(batch(1, [reset(s.entries, head)], undefined, stream))),
        ),
      setup: { first: () => events, command: () => {} },
    });
    return transport;
  };
}

const room = (s: Scene) => () => <RoomOver host={sceneHost(s)} />;

/** The first read in the thread, its turn marked with `papers` files. */
const firstRead = (s: Omit<Scene, "title">, papers: number) => (): ReactNode => {
  seedMarks({ [MATTER.matter]: { added: true, thread: THREAD, turns: [TURN], papers } });
  return <RoomOver host={sceneHost({ ...s, title: "First read" })} />;
};
const PROMPT = userMessage(2, 1, "Read every paper in this matter's folder.");
const reading = (title: string | null): ViewEntry => ({
  tool_call: {
    order: 3,
    tool_call: "00000000-0000-7000-8000-0000000000a7",
    turn: TURN,
    started_at: 3000,
    title: title === null ? "Read" : `Read ${title}`,
    kind: "read",
    status: "in_progress",
    locations: title === null ? [] : [{ inside: { path: `papers/${title}` } }],
    content: [],
  },
});
const ANSWER = agentText(
  4,
  1,
  "Rathi Steels sues Acme Traders for ₹42 lakh unpaid on four invoices (Invoices 2025.pdf). The next date is 14 November 2026.",
);
const STARTED: Omit<Scene, "title"> = { entries: [PROMPT], status: "running" };
const READING: Omit<Scene, "title"> = {
  entries: [PROMPT, reading("Board minutes.docx")],
  status: "running",
};
const DONE: Omit<Scene, "title"> = { entries: [PROMPT, ANSWER, step(5)] };
const STOPPED: Omit<Scene, "title"> = {
  entries: [PROMPT, ANSWER, step(5, { stop: "cancelled" })],
};

/** A question held after its turn failed with agent.auth_required, the plan's sign-in as given. */
const expired = (signIn: SignIn) =>
  room({
    entries: [
      userMessage(2, 1, "What does clause nine of the lease say about notice?"),
      step(3, { stop: "failed", failure: "agent.auth_required" }),
    ],
    held: true,
    signIn,
  });
const EXPIRED = expired({ failed: { code: "agent.auth_required", at: 2000 } });
const again = (over: { email?: string; plan?: string }) =>
  expired({ signed_in: { method: "plan", source: "own", at: AT, ...over } });

/** Two questions queued while an update installs: the host holds them (#537). */
const queuedQ = (order: number, n: number, text: string): ViewEntry => {
  const e = userMessage(order, n, text);
  e.user_message!.status = "queued";
  e.user_message!.turn = `00000000-0000-7000-8000-00000000000${n}`;
  return e;
};
const UPDATING = room({
  entries: [
    userMessage(2, 1, "What does clause nine of the lease say about notice?"),
    agentText(3, 1, "Clause nine asks for three months' notice in writing."),
    step(4),
    queuedQ(5, 2, "Draft the notice for the landlord."),
    queuedQ(6, 3, "List the dates the lease fixes."),
  ],
  held: true,
  reason: "update",
});

const entry = (id: string, words: WordsRef, screen: string, render: () => ReactNode): Entry => ({
  id,
  words,
  screen,
  render,
});

export const UD2: Entry[] = [
  entry("UD2-01", scanWords("{n} files, {mb} MB"), "matters/new-scan", scan(COUNTING)),
  entry(
    "UD2-02",
    scanWords("1 file, {mb} MB"),
    "matters/new-scan",
    scan(scanned({ files: 1, bytes: 2_400_000, now_reading: "Plaint.pdf" })),
  ),
  entry(
    "UD2-03",
    scanWords("Reading the folder. Now {title}."),
    "matters/new-scan",
    scan(COUNTING),
  ),
  entry(
    "UD2-04",
    scanWords("Reading the folder."),
    "matters/new-scan",
    scan(scanned({ files: 0, bytes: 0 })),
  ),
  entry(
    "UD2-06",
    scanWords(
      "This folder syncs with {service}, so changes to its papers sync too, including Clauseo's. Versions are kept on this computer, outside the folder.",
    ),
    "matters/new-synced",
    scan(scanned({ place: SYNCED, done: true })),
  ),
  entry(
    "UD2-07",
    scanWords("Got it"),
    "matters/new-synced",
    scan(scanned({ place: SYNCED, done: true })),
  ),
  entry(
    "UD2-08",
    scanWords("{n} files are kept only in {service} and download as they're read."),
    "matters/new-cloud-only",
    scan(scanned({ place: SYNCED, cloud_only: 6, done: true })),
  ),
  entry(
    "UD2-09",
    scanWords("1 file is kept only in {service} and downloads as it's read."),
    "matters/new-cloud-only",
    scan(scanned({ place: SYNCED, cloud_only: 1, done: true })),
  ),
  entry(
    "UD2-10",
    scanWords("{n} files are kept only online and download as they're read."),
    "matters/new-cloud-only-unnamed",
    scan(scanned({ cloud_only: 6, done: true })),
  ),
  entry(
    "UD2-11",
    scanWords("1 file is kept only online and downloads as it's read."),
    "matters/new-cloud-only-unnamed",
    scan(scanned({ cloud_only: 1, done: true })),
  ),
  entry("UD2-12", readWords("First read"), "room/first-read", firstRead(STARTED, 24)),
  entry("UD2-13", readWords("Clauseo's first read"), "room/first-read", firstRead(STARTED, 24)),
  entry("UD2-14", readWords("Reading {n} files."), "room/first-read", firstRead(STARTED, 24)),
  entry(
    "UD2-15",
    readWords("Reading {n} files. Now {title}."),
    "room/first-read",
    firstRead(READING, 24),
  ),
  entry("UD2-16", readWords("Reading 1 file."), "room/first-read", firstRead(STARTED, 1)),
  entry(
    "UD2-17",
    readWords("Reading 1 file. Now {title}."),
    "room/first-read",
    firstRead(READING, 1),
  ),
  entry("UD2-18", readWords("Read {n} files."), "room/first-read-done", firstRead(DONE, 24)),
  entry("UD2-19", readWords("Read 1 file."), "room/first-read-done", firstRead(DONE, 1)),
  entry(
    "UD2-20",
    readWords("Clauseo stopped. The answer so far is kept."),
    "room/first-read-stopped",
    firstRead(STOPPED, 24),
  ),
  entry(
    "UD2-21",
    expiredWords("Your {name} sign-in has expired. Your question is kept."),
    "limits/signin",
    EXPIRED,
  ),
  entry(
    "UD2-22",
    expiredWords(
      "Sign in again opens {name}'s own sign-in in your browser. Only the sign-in repeats: this matter, its papers and your settings stay as they are.",
    ),
    "limits/signin",
    EXPIRED,
  ),
  entry("UD2-23", expiredWords("Sign in again"), "limits/signin", EXPIRED),
  entry(
    "UD2-24",
    expiredWords("Finish signing in in your browser. Your question is asked once you're back."),
    "limits/signin-waiting",
    expired({ waiting: { since: 2500, nudge: true } }),
  ),
  entry(
    "UD2-25",
    expiredWords("Signed in again at {time} as {email}, {plan}."),
    "limits/signin-again",
    again({ email: "r.mehra@mehrachambers.in", plan: "Claude Max" }),
  ),
  entry(
    "UD2-26",
    expiredWords("Signed in again at {time} as {email}."),
    "limits/signin-again",
    again({ email: "r.mehra@mehrachambers.in" }),
  ),
  entry("UD2-27", expiredWords("Signed in again at {time}."), "limits/signin-again", again({})),
  entry(
    "UD2-28",
    expiredWords("Your question is kept."),
    "limits/signin-reuse-stopped",
    expired({ signed_out: { found: "reused_signed_out" } }),
  ),
  entry(
    "UD2-29",
    expiredWords("{name} didn't accept the API key. Your question is kept."),
    "limits/signin-key",
    expired({ signed_in: { method: "api_key", source: "own", at: 1 } }),
  ),
  entry(
    "UD2-30",
    expiredWords("To change it, open Settings, then Plans."),
    "limits/signin-key",
    expired({ signed_in: { method: "api_key", source: "own", at: 1 } }),
  ),
  entry(
    "UD2-31",
    expiredWords("Your firm's account didn't accept the question. Your question is kept."),
    "limits/signin-firm",
    expired({ signed_in: { method: "firm", source: "own", at: 1 } }),
  ),
  entry(
    "UD2-57",
    matterWords("Clauseo is installing an update. Your queued questions go once it's done."),
    "matters/held-update",
    UPDATING,
  ),
];
