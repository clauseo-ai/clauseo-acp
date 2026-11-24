// Skills and connectors, library L1 (UC-1050 to UC-1089, UC-1215 to UC-1244; 24 Nov round, issues 6
// to 8): the real room over library-host.ts, the place opened from the rail's row and pressed to
// the state that shows each string. UC_LIBRARY_CASE_LAW registers Clauseo Case Law's shared words
// (UC-1090 to UC-1095, carried here in UC-kanoon-card.json) on the library's card, so this branch
// stands alone; the home card's branch (#1297), stacked on it, drops the spread and registers them
// through its own export.
import { cr, type ConnectorsRoomKey } from "@clauseo/components/lib/words-connectors-room.ts";
import { lb, type LibraryKey } from "@clauseo/components/lib/words-library.ts";
import type { Entry } from "./entry.ts";
import { libraryHost, type LibraryState } from "./library-host.ts";
import { RoomClicking, RoomOver } from "./states.tsx";
import type { WordsRef } from "./words.ts";

const libWords = (key: LibraryKey): WordsRef => ({
  file: "lib/words-library.ts",
  key,
  text: (lang) => lb(lang, key),
  later: lb.hi[key] === null,
});

const roomWords = (key: ConnectorsRoomKey): WordsRef => ({
  file: "lib/words-connectors-room.ts",
  key,
  text: (lang) => cr(lang, key),
  later: cr.hi[key] === null,
});

const ENTRY = '[data-testid="library-entry"]';
const CONN = '[role="tab"]:nth-of-type(2)';
const CARD = '[data-testid="case-law-card"] .lib-feature-hit';
const row = (id: string) => `[data-id="${id}"]`;

/** The place opened from the rail, then `clicks` pressed in order. */
const at =
  (state: LibraryState, ...clicks: string[]) =>
  () => <RoomClicking host={() => libraryHost(state)} clicks={[ENTRY, ...clicks]} />;

const rail = () => <RoomOver host={() => libraryHost()} />;

const e = (id: string, key: LibraryKey, render: Entry["render"]): Entry => ({
  id,
  words: libWords(key),
  screen: "room/library",
  render,
});

const SKILLS = at({});
const CONNECTORS = at({}, CONN);

export const UC_LIBRARY: Entry[] = [
  e("UC-1050", "Skills and connectors", rail),
  e("UC-1051", "{skills} skills, {connectors} connectors", rail),
  e("UC-1052", "New", rail),
  e("UC-1053", "Skills", SKILLS),
  e("UC-1054", "Connectors", SKILLS),
  e("UC-1055", "Comes with Clauseo", SKILLS),
  e("UC-1056", "Added by you", SKILLS),
  e("UC-1057", "Your own connectors", CONNECTORS),
  e("UC-1058", "MCP servers", CONNECTORS),
  e("UC-1059", "Legal drafting", SKILLS),
  e(
    "UC-1060",
    "Drafts and revises notices, replies, opinions and agreements from the matter's papers, and checks every citation before handing the draft over.",
    SKILLS,
  ),
  e("UC-1061", "Legal research", SKILLS),
  e(
    "UC-1062",
    "Researches Indian law over several searches, reads the later judgments that cite a case, and ties every quote to a page it read.",
    SKILLS,
  ),
  e("UC-1063", "Matter brief", SKILLS),
  e(
    "UC-1064",
    "Keeps a short brief of the matter in its notes folder: parties, facts, forum, positions taken and open questions.",
    SKILLS,
  ),
  e("UC-1065", "Word documents", SKILLS),
  e(
    "UC-1066",
    "Opens and edits Word files in the matter, with each change shown as a tracked change you can accept or reject.",
    SKILLS,
  ),
  e("UC-1067", "Web browser", CONNECTORS),
  e(
    "UC-1068",
    "Opens web pages that need a real browser, such as court websites with forms to fill.",
    CONNECTORS,
  ),
  e("UC-1069", "Every matter", SKILLS),
  e("UC-1070", "Only {matters}", SKILLS),
  e("UC-1071", "Turned off", SKILLS),
  e("UC-1072", "Stopped", SKILLS),
  e("UC-1073", "Can't be used", SKILLS),
  e("UC-1074", "Not allowed", SKILLS),
  e("UC-1075", "Key missing", CONNECTORS),
  e("UC-1076", "From a folder on this computer", at({}, row("notice-drafting"))),
  e("UC-1077", "From a zip file on this computer", SKILLS),
  e("UC-1078", "From {source}", at({}, row("pdf"))),
  e("UC-1079", "MCP server, a program on this computer", CONNECTORS),
  e("UC-1080", "MCP server at {host}", CONNECTORS),
  e("UC-1081", "MCP server, brought over from {app}", CONNECTORS),
  e("UC-1082", "What a skill is", SKILLS),
  e(
    "UC-1083",
    "A skill is a page of instructions for one kind of work, such as drafting a legal notice the way your chamber does. When a question calls for it, {who} reads the skill and follows it.",
    SKILLS,
  ),
  e(
    "UC-1084",
    "Most skills are instructions only. A skill that holds programs says so, because {who} may run those programs on this computer.",
    SKILLS,
  ),
  e("UC-1085", "Where skills come from", SKILLS),
  e(
    "UC-1086",
    "skills.sh lists skills people have shared. A colleague can also send you a skill as a folder or a zip file.",
    SKILLS,
  ),
  e("UC-1087", "What a connector is", CONNECTORS),
  e(
    "UC-1088",
    "A connector lets {who} reach something outside the matter's papers: a case-law database, a website, a program your firm runs. When one is used, the conversation shows it as a step.",
    CONNECTORS,
  ),
  e("UC-1089", "Your own connectors are MCP servers", CONNECTORS),
  e(
    "UC-1215",
    "MCP is the common way apps such as Claude Desktop connect to outside tools, so one made for those apps works here too. You add it by its address on the internet, or by the command that starts it on this computer.",
    CONNECTORS,
  ),
  e(
    "UC-1216",
    "It runs with the same access you have: your files, your programs and the internet. Add only connectors you trust.",
    CONNECTORS,
  ),
  e("UC-1217", "Where {who} uses it", at({}, row("legal-drafting"))),
  e("UC-1218", "How Clauseo reaches it", at({}, CONN, row("court-diary"))),
  e("UC-1219", "Address", at({}, CONN, row("precedents"))),
  e("UC-1220", "Command", at({}, CONN, row("court-diary"))),
  e("UC-1221", "Saved values", at({}, CONN, row("court-diary"))),
  e(
    "UC-1222",
    "This skill holds programs. {Who} may run them on this computer, with the same access you have.",
    at({}, row("pdf")),
  ),
  e(
    "UC-1223",
    "It comes with its own skill, Using the browser, with rules for using a browser on legal work. The sites it may open are in Settings, under Allowed sites.",
    at({}, CONN, row("browser")),
  ),
  e(
    "UC-1224",
    "Its files changed after you added it, so Clauseo stopped using it.",
    at({}, row("lease-review")),
  ),
  e(
    "UC-1225",
    "Your firm doesn't allow this one on this computer.",
    at({}, row("contract-review")),
  ),
  e(
    "UC-1226",
    "Its saved key is missing, so {who} can't reach it.",
    at({}, CONN, row("precedents")),
  ),
  e("UC-1227", "Can't reach it right now", at({ caseLaw: "unreachable" }, CONN)),
  // Retry once list_addons checks again itself (stage D's D1); until then the button opens Settings
  e("UC-1228", "Retry", at({ caseLaw: "unreachable", again: true }, CONN)),
  e(
    "UC-1229",
    "Clauseo can't reach Clauseo Case Law right now, so {who} can't search judgments.",
    at({ caseLaw: "unreachable" }, CONN, CARD),
  ),
  e("UC-1230", "{credits} credits left", at({ caseLaw: "ready" }, CONN)),
  e("UC-1231", "About {n} searches", at({ caseLaw: "ready" }, CONN)),
  e("UC-1232", "No credits left", at({ caseLaw: "out" }, CONN)),
  e("UC-1233", "What it covers", at({}, CONN, CARD)),
  e(
    "UC-1234",
    "Judgments of the Supreme Court, the High Courts and tribunals, and central Acts. For any judgment, {who} can see which later judgments cite it, and how.",
    at({}, CONN, CARD),
  ),
  e("UC-1235", "What it costs", at({}, CONN, CARD)),
  e(
    "UC-1236",
    "A page of search results is 20 credits. Opening a judgment is 3 credits, plus 1 for each 4,000 tokens {who} reads (about 3,000 words).",
    at({}, CONN, CARD),
  ),
  e("UC-1237", "You haven't added a skill yet.", at({ added: false })),
  e("UC-1238", "You haven't added a connector of your own yet.", at({ added: false }, CONN)),
  e("UC-1241", "Open Settings to check", at({ caseLaw: "unreachable" }, CONN)),
  e(
    "UC-1242",
    "Connectors are turned off. You can turn them on in Settings, under Connectors.",
    at({ caseLaw: "off" }, CONN, CARD),
  ),
  e("UC-1243", "Signed out", at({ caseLaw: "ended" }, CONN)),
  e(
    "UC-1244",
    "Sign in again in Settings, under Connectors.",
    at({ caseLaw: "ended" }, CONN, CARD),
  ),
  e("UC-1245", "Open Settings", at({ caseLaw: "off" }, CONN)),
  e("UC-1239", "the assistant", at({ both: true })),
  e("UC-1240", "The assistant", at({ both: true }, row("pdf"))),
];

const shared = (id: string, key: ConnectorsRoomKey): Entry => ({
  id,
  words: roomWords(key),
  screen: "room/library",
  render: CONNECTORS,
});

/** Clauseo Case Law's agreed words on the library's card: the home card's ids (see the top of this
 * file). */
export const UC_LIBRARY_CASE_LAW: Entry[] = [
  shared(
    "UC-1090",
    "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.",
  ),
  shared("UC-1091", "Add Clauseo Case Law"),
  shared("UC-1092", "Clauseo Case Law"),
  shared("UC-1093", "From {price}, about {n} searches"),
  shared("UC-1095", "Prepaid, including GST. No subscription."),
  shared("UC-1094", "Powered by Indian Kanoon"),
];
