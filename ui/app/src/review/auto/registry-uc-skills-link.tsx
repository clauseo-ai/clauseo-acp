// Add a skill (RC A1, UC-1520 to UC-1539 and UC-1570; the reworded lines are UC-1086's and UC-971's own entries,
// and the four codes' words U3-err-skill.*'s): the real room over library-host.ts, Sharma v. State of
// Delhi opened first, the place opened from the rail's row, then Add a skill pressed, and Choose a
// zip pressed to the preview, held open, or refused as the state says.
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
const ADD_SKILL = '[data-testid="library-add-skill"]';
const ZIP = '[data-testid="add-skill-zip"]';
const ADD = '[data-testid="add-skill-add"]';

/** Sharma opened, the place opened, Add a skill pressed, then `clicks` pressed in order. */
const at =
  (state: LibraryState, ...clicks: string[]) =>
  () => (
    <RoomClicking
      host={() => libraryHost(state)}
      clicks={[MATTER_ROW, ENTRY, ADD_SKILL, ...clicks]}
    />
  );

const e = (id: string, key: LibraryKey, render: Entry["render"]): Entry => ({
  id,
  words: libWords(key),
  screen: "room/library",
  render,
});

const OPEN = at({});
const PREVIEW = at({}, ZIP);

const UC_SKILLS_LINK: Entry[] = [
  e("UC-1520", "Add a skill", OPEN),
  e("UC-1521", "Nothing is added until you've seen what it holds.", OPEN),
  e("UC-1522", "Paste a GitHub link or a skills.sh name", OPEN),
  e("UC-1523", "Your search words go to skills.sh. Nothing from your matters does.", OPEN),
  e("UC-1524", "Look at it", OPEN),
  e("UC-1525", "Previewing…", at({ adding: { hold: "preview" } }, ZIP)),
  e("UC-1526", "Choose a zip", OPEN),
  e("UC-1527", "Choose a folder", OPEN),
  e("UC-1528", "It holds {n} skills. Tick the ones to add.", PREVIEW),
  e("UC-1529", "It holds one skill.", at({ adding: { one: true } }, ZIP)),
  e("UC-1530", "You have this one. Adding it again replaces yours.", PREVIEW),
  e("UC-1531", "Holds programs", PREVIEW),
  e(
    "UC-1532",
    "Skills that hold programs can run them on this computer, with the same access you have. Add only skills you trust.",
    PREVIEW,
  ),
  e("UC-1533", "Use them in", PREVIEW),
  e("UC-1534", "Add", PREVIEW),
  e("UC-1535", "Adding…", at({ adding: { hold: "add" } }, ZIP, ADD)),
  e(
    "UC-1536",
    "Look again",
    at({ adding: { refuse: { add: "skill.preview_expired" } } }, ZIP, ADD),
  ),
  e(
    "UC-1537",
    "Clauseo couldn't add that skill, so nothing was added.",
    at({ adding: { refuse: { preview: "skill.invalid" } } }, ZIP),
  ),
  e("UC-1538", "Try again", at({ adding: { refuse: { preview: "skill.download_failed" } } }, ZIP)),
  e(
    "UC-1539",
    "There's no SKILL.md file in it. Choose a folder or a zip that holds one.",
    at({ adding: { refuse: { preview: "skill.no_skill_md" } } }, ZIP),
  ),
  e(
    "UC-1570",
    "Clauseo couldn't tell whether that skill was added. Look for it in the list before you add it again.",
    at({ adding: { refuse: { add: "internal" } } }, ZIP, ADD),
  ),
];

export default UC_SKILLS_LINK;
