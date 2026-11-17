// Settings › Skills (UC-970 to UC-978): the skills the lawyer adds and turns on, for Claude and ChatGPT alike. As in
// words.ts the English is the key, the exact string on screen; the Hindi comes later (null). The words other screens
// already say (Remove, Remove {name}?, Cancel) are theirs, reused.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-skills.hi.ts";

/** Every string of Settings › Skills, in English. */
export type SkillsKey = keyof typeof HI;

/** A Settings › Skills string in the language, its slots filled. */
export const sk = wordsOf("skills");
