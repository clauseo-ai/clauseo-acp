// The Hindi of words-skills.ts, keyed by its English: none yet (hi: later), so a Hindi display shows the English.
// Loaded only for a Hindi display: words-hi.ts gathers every words file's Hindi into one lazy chunk, and
// words-skills.ts imports this file's type alone.

export const HI = {
  // UC-970, UC-971: the section's name, and the line under its head
  Skills: null,
  "Skills teach Claude and ChatGPT to do a task your way. Add a folder that holds a SKILL.md file, or the SKILL.md file itself.":
    null,
  // UC-972 to UC-974: the button, an added skill's source line, and the empty list
  "Add a skill": null,
  "Added on this computer": null,
  "No skills added yet.": null,
  // UC-975, UC-976: under Add a skill, a folder with no SKILL.md, and any other refusal
  "That folder has no SKILL.md file. Choose the folder that holds one, or the file itself.": null,
  "Clauseo couldn't make that change. Your skills are as they were.": null,
  // UC-977, UC-978: a skill that can't load as it stands, and the button beside a changed one
  "Clauseo can't use this skill as it is.": null,
  "Turn on again": null,
} as const;
