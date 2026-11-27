// Skills and connectors, library L2 (UC-1301 to UC-1339, UC-1327 and UC-1336 retired; 24 Nov
// round): the real room over library-host.ts, Sharma v. State of Delhi opened first so the lens
// has the matter open last, then the place opened from the rail's row and pressed to the state
// that shows each string: the lens, the rows' switches and where each is on, the detail's choice
// and tick boxes, Remove and its confirmation, the built-ins' and the firm's lines, an item turned
// off itself, a refusal's line, and a failed read of what the matter gets.
import { lb, type LibraryKey } from "@clauseo/components/lib/words-library.ts";
import type { Entry } from "../entry.ts";
import { libraryHost, type LibraryState } from "../library-host.ts";
import { RoomClicking } from "../states.tsx";
import type { WordsRef } from "../words.ts";

const libWords = (key: LibraryKey): WordsRef => ({
  file: "lib/words-library.ts",
  key,
  text: (lang) => lb(lang, key),
  later: lb.hi[key] === null,
});

const MATTER_ROW = ".room-matters .room-matter-row";
const ENTRY = '[data-testid="library-entry"]';
const CONN = '[role="tab"]:nth-of-type(2)';
const EVERY = '[data-testid="library-lens"] button:nth-of-type(2)';
const REMOVE = '[data-testid="library-remove"]';
const PDF_SWITCH = '[data-id="pdf"] + [role="switch"]';
const row = (id: string) => `[data-id="${id}"]`;

/** Sharma opened, the place opened from the rail, then `clicks` pressed in order. */
const at =
  (state: LibraryState, ...clicks: string[]) =>
  () => <RoomClicking host={() => libraryHost(state)} clicks={[MATTER_ROW, ENTRY, ...clicks]} />;

const e = (id: string, key: LibraryKey, render: Entry["render"]): Entry => ({
  id,
  words: libWords(key),
  screen: "room/library",
  render,
});

const SKILLS = at({});
const PDF = at({}, row("pdf"));

const UC_LIBRARY_L2: Entry[] = [
  e("UC-1301", "Switches for", SKILLS),
  e("UC-1302", "{name}, on in {matter}", SKILLS),
  e("UC-1303", "{name}, on in every matter", at({}, EVERY)),
  e("UC-1304", "Every matter but {n}", SKILLS),
  e("UC-1305", "Only {n} matters", at({}, CONN)),
  e("UC-1306", "Only the matters you tick", PDF),
  e("UC-1307", "New matters too. Untick a matter to leave it out.", PDF),
  e("UC-1308", "New matters start without it.", PDF),
  e("UC-1309", "open last", PDF),
  e("UC-1310", "Changes apply from each conversation's next message.", PDF),
  e("UC-1311", "Remove", PDF),
  e("UC-1312", "Remove {name}?", at({}, row("pdf"), REMOVE)),
  e(
    "UC-1313",
    "Clauseo deletes its files from this computer and turns it off in every matter. To use it again, add it again.",
    at({}, row("pdf"), REMOVE),
  ),
  e(
    "UC-1314",
    "Clauseo removes it and its saved keys from this computer, and turns it off in every matter.",
    at({}, CONN, row("court-diary"), REMOVE),
  ),
  e("UC-1315", "Cancel", at({}, row("pdf"), REMOVE)),
  e(
    "UC-1316",
    "Comes with Clauseo, so it stays. Turn it off where you don't want it.",
    at({}, CONN, row("browser")),
  ),
  e(
    "UC-1317",
    "Comes with Clauseo, so it stays on in every matter.",
    at({}, row("legal-drafting")),
  ),
  e("UC-1318", "Set by your firm", at({ locked: true })),
  e(
    "UC-1319",
    "Your firm decides where this is used and whether it stays.",
    at({ locked: true }, row("notice-drafting")),
  ),
  e(
    "UC-1320",
    "Clauseo couldn't find {name} on this computer, so nothing changed.",
    at({ refuse: "addon.unknown" }, PDF_SWITCH),
  ),
  e(
    "UC-1321",
    "{name} comes with Clauseo, so this can't be changed.",
    at({ refuse: "addon.built_in" }, PDF_SWITCH),
  ),
  e(
    "UC-1322",
    "Your firm decides this for {name}, so nothing changed.",
    at({ refuse: "settings.locked" }, PDF_SWITCH),
  ),
  e(
    "UC-1323",
    "Clauseo couldn't make that change to {name}. Try again.",
    at({ refuse: "internal" }, PDF_SWITCH),
  ),
  e("UC-1324", "Every matter, or some", SKILLS),
  e(
    "UC-1325",
    "The switch on each row is for {matter}, the matter you had open. Choose Every matter above to change what new matters start with.",
    SKILLS,
  ),
  e(
    "UC-1326",
    "The switch on each row turns it on or off in every matter. Choose a matter above to change only that matter.",
    at({}, EVERY),
  ),
  e("UC-1328", "Not used in any matter", at({}, CONN)),
  e(
    "UC-1329",
    "Comes with Clauseo. Switch it off for a matter from that matter, or turn it off everywhere in Settings › Privacy.",
    at({}, CONN, EVERY),
  ),
  e(
    "UC-1330",
    "Comes with Clauseo. Switch it off for a matter from that matter, or turn connectors off everywhere in Settings › Connectors.",
    at({ caseLaw: "ready" }, CONN, EVERY),
  ),
  e(
    "UC-1331",
    "Turned off in Settings › Tools, so no matter uses it. Switch it on here to turn it back on.",
    at({ toolOff: true }, CONN),
  ),
  e(
    "UC-1332",
    "Turned off in Settings › Skills, so no matter uses it. Switch it on here to turn it back on.",
    SKILLS,
  ),
  e(
    "UC-1333",
    "Turned off in Settings › Tools, so no matter uses it. Choose where to use it to turn it back on.",
    at({ toolOff: true }, CONN, row("court-diary")),
  ),
  e(
    "UC-1334",
    "Turned off in Settings › Skills, so no matter uses it. Choose where to use it to turn it back on.",
    at({}, row("cheque-bounce")),
  ),
  e("UC-1335", "This replaces the choices made in each matter.", PDF),
  e(
    "UC-1337",
    "Turned off in Settings › Privacy, so no matter uses it. Turn it on there first.",
    at({ disabled: ["browser"] }, CONN),
  ),
  e(
    "UC-1338",
    "Connectors are turned off in Settings › Connectors, so no matter uses it. Turn them on there first.",
    at({ caseLaw: "ready", disabled: ["case-law"] }, CONN, ".lib-feature-hit"),
  ),
  e(
    "UC-1339",
    "Clauseo couldn't check what {matter} gets just now. The switches show how each is set.",
    at({ matterReadFails: true }),
  ),
];

export default UC_LIBRARY_L2;
