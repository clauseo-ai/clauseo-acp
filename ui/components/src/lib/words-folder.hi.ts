// The Hindi of words-folder.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-folder.ts imports this file's type alone.

export const HI = {
  // FR1: the notice's title, then its tooltip and accessible name
  "Clauseo asks before each edit": null, // hi: later
  "Clauseo asks before each edit in this folder": null, // hi: later
  // FR2: behind FR1's info icon
  "You can change files here, but not this folder's permissions. Clauseo needs them to keep Claude and Codex inside the matter. Some edits won't go through.":
    null, // hi: later
  // FR3: the notice's button, which opens FR5
  "What to tell IT": null, // hi: later
  // FR5: the text to send IT, selected, with Copy; {path} is the matter's folder
  'Please give my account "Change permissions" on this folder: {path}. Clauseo, the app I use for this matter, adds one entry to the folder\'s permissions so Claude and Codex, which it runs, can write only inside it.':
    null, // hi: later
  Copy: null, // hi: later
  // FR12: a network drive's notice: its words, its tooltip and accessible name, and behind its info icon
  "On a network drive.": null, // hi: later
  "Claude asks before every change. Codex can't work here.": null, // hi: later
  "This matter's folder is on a network drive": null, // hi: later
  "Clauseo can't keep Claude and Codex inside this folder. For full protection, move the matter to a folder on this computer.":
    null, // hi: later
  // FR15: a drive without file permissions, as FR12
  "No file permissions.": null, // hi: later
  "This matter's folder is on a drive that doesn't keep file permissions": null, // hi: later
  "Clauseo can't keep Claude and Codex inside this folder. USB drives often don't keep permissions. For full protection, move the matter to a folder on this computer.":
    null, // hi: later
  // FR7: New matter's warning for a folder picked on a network drive, then its tooltip and accessible name
  "On a network drive": null, // hi: later
  "This folder is on a network drive": null, // hi: later
  // FR8: the warning's body is FR12's second line; behind its info icon (provisional: "an agent" reworded)
  "Clauseo can't keep Claude and Codex inside a folder on a network drive.": null, // hi: later
  // FR9: the warning's main button, which opens the picker again
  "Choose another folder": null, // hi: later
  // FR10: the warning's other button, which makes the matter there
  "Create anyway": null, // hi: later
  // FR13: the warning for a drive without file permissions, then its tooltip and accessible name
  "No file permissions": null, // hi: later
  "This drive doesn't keep file permissions": null, // hi: later
  // FR14: its body as FR8's; behind its info icon (provisional: "an agent" reworded)
  "Clauseo can't keep Claude and Codex inside a folder on this drive. USB drives often don't keep permissions.":
    null, // hi: later
  // FR20 (provisional): the room's notice for a folder the host couldn't check
  "Clauseo couldn't check this folder, so Claude asks before every change.": null, // hi: later
} as const;
