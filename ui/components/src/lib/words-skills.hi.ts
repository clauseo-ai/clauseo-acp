// The Hindi of words-skills.ts, keyed by its English: none yet (hi: later), so a Hindi display shows the English.
// Loaded only for a Hindi display: words-hi.ts gathers every words file's Hindi into one lazy chunk, and
// words-skills.ts imports this file's type alone.

export const HI = {
  // UC-970, UC-971: the section's name, and the line under its head
  Skills: null,
  "Skills teach Claude and ChatGPT to do a task your way. Add one by its skills.sh name or a GitHub link, or from a zip file or the folder that holds its SKILL.md file.":
    "स्किल Claude और ChatGPT को कोई काम आपके तरीक़े से करना सिखाते हैं। किसी को उसके skills.sh नाम या GitHub लिंक से जोड़ें, या किसी zip फ़ाइल या उस फ़ोल्डर से जिसमें उसकी SKILL.md फ़ाइल है।",
  // UC-972 to UC-974: the button, an added skill's source line, and the empty list
  "Add a skill": null,
  "Added on this computer": null,
  "No skills added yet.": null,
  // UC-976: under Add a skill, a refused switch or Remove (UC-975, a folder with no SKILL.md, retired: Add a skill's
  // panel says it, library/add-skill.tsx)
  "Clauseo couldn't make that change. Your skills are as they were.": null,
  // UC-977, UC-978: a skill that can't load as it stands, and the button beside a changed one
  "Clauseo can't use this skill as it is.": null,
  "Turn on again": null,
} as const;
