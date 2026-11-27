// Settings › Skills' copy review entries (UC-970 to UC-978): each string in the real settings window over the fake host
// (settings/sections/skills-fake.ts), open at Skills: the list, the empty list, and Add a skill refused. Hindi later.
import { sk, type SkillsKey } from "@clauseo/components/lib/words-skills.ts";
import { SkillsOver, type SkillsPage } from "../settings/sections/skills-probe.tsx";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** The settings window, 960 by 700 (DESIGN.md). */
const WINDOW = { width: 960, height: 700 } as const;

const entry = (id: string, key: SkillsKey, page: SkillsPage): Entry => ({
  id,
  words: {
    file: "lib/words-skills.ts",
    key,
    text: (lang) => sk(lang, key),
    later: sk.hi[key] === null,
  } satisfies WordsRef,
  screen: "settings/skills",
  ...WINDOW,
  render: (lang) => <SkillsOver page={page} lang={lang} />,
});

/** Skills' entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const UC_SKILLS: Entry[] = [
  entry("UC-970", "Skills", "list"),
  entry(
    "UC-971",
    "Skills teach Claude and ChatGPT to do a task your way. Add one by its skills.sh name or a GitHub link, or from a zip file or the folder that holds its SKILL.md file.",
    "list",
  ),
  entry("UC-972", "Add a skill", "list"),
  entry("UC-973", "Added on this computer", "list"),
  entry("UC-974", "No skills added yet.", "empty"),
  entry("UC-976", "Clauseo couldn't make that change. Your skills are as they were.", {
    refused: "skill.invalid",
  }),
  entry("UC-977", "Clauseo can't use this skill as it is.", "list"),
  entry("UC-978", "Turn on again", "list"),
];
