// The Hindi of words-steps.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-steps.ts imports this file's type alone.

export const HI = {
  // a row, its step done
  "Read {name}.": "{name} पढ़ा गया।",
  "Read the papers.": "दस्तावेज़ पढ़े गए।",
  "Changed {name}.": "{name} बदला गया।",
  "Changed a paper.": "एक दस्तावेज़ बदला गया।",
  "Deleted {name}.": "{name} हटाया गया।",
  "Deleted a paper.": "एक दस्तावेज़ हटाया गया।",
  "Moved {name}.": "{name} की जगह बदली गई।",
  "Moved a paper.": "एक दस्तावेज़ की जगह बदली गई।",
  "Searched for '{query}'.": "'{query}' खोजा गया।",
  "Searched the papers.": "दस्तावेज़ों में खोजा गया।",
  "Searched online for '{query}'.": "ऑनलाइन '{query}' खोजा गया।",
  "Read {name} online.": "{name} ऑनलाइन पढ़ा गया।",
  "Looked online.": "ऑनलाइन देखा गया।",
  "Ran a command.": null,
  // a task named by the program it ran and the paper it ran on, where the agent gave no words of its own (D-017)
  "Ran {program} on {name}.": "{name} पर {program} चलाया गया।",
  "Ran {program}.": "{program} चलाया गया।",
  // a web search limited to one site, the site said apart from the words searched for (D-028)
  "Searched {site} for '{query}'.": "{site} पर '{query}' खोजा गया।",
  "Thought it through.": "विचार किया गया।",
  "Used a tool.": null,
  // a task named by its own title or the text it shows (UC-246, UC-247; provisional)
  "Ran {description}.": "{description} चलाया गया।",
  "Tried to run {description}.": "{description} चलाने की कोशिश की गई।",
  // a skill that names none (UC-248, UC-249; provisional again 24 Nov, were "Read a guide.")
  "Used a skill.": null,
  "Tried to use a skill.": null,
  // a step named only by the sandbox's permission for the internet (UC-166; provisional)
  "Let a task reach the internet.": "एक काम को इंटरनेट तक पहुँचने दिया गया।",
  // a step that failed or was stopped: what it set out to do, in the past, never as if it were still going; the row's
  // mark (Failed, Stopped) says how it ended
  "Tried to read {name}.": "{name} पढ़ने की कोशिश की गई।",
  "Tried to read the papers.": "दस्तावेज़ पढ़ने की कोशिश की गई।",
  "Tried to change {name}.": "{name} बदलने की कोशिश की गई।",
  "Tried to change a paper.": "एक दस्तावेज़ बदलने की कोशिश की गई।",
  "Tried to delete {name}.": "{name} हटाने की कोशिश की गई।",
  "Tried to delete a paper.": "एक दस्तावेज़ हटाने की कोशिश की गई।",
  "Tried to move {name}.": "{name} की जगह बदलने की कोशिश की गई।",
  "Tried to move a paper.": "एक दस्तावेज़ की जगह बदलने की कोशिश की गई।",
  "Tried to search for '{query}'.": "'{query}' खोजने की कोशिश की गई।",
  "Tried to search the papers.": "दस्तावेज़ों में खोजने की कोशिश की गई।",
  "Tried to search online for '{query}'.": "ऑनलाइन '{query}' खोजने की कोशिश की गई।",
  "Tried to read {name} online.": "{name} ऑनलाइन पढ़ने की कोशिश की गई।",
  "Tried to look online.": "ऑनलाइन देखने की कोशिश की गई।",
  "Tried to run a command.": null,
  "Tried to run {program} on {name}.": "{name} पर {program} चलाने की कोशिश की गई।",
  "Tried to run {program}.": "{program} चलाने की कोशिश की गई।",
  // a chained command, counted rather than named by its first program (UC-300; provisional); done, "Ran {n}
  // commands."
  "Tried to run {n} commands.": null,
  "Tried to search {site} for '{query}'.": "{site} पर '{query}' खोजने की कोशिश की गई।",
  "Tried to think it through.": "विचार करने की कोशिश की गई।",
  "Tried to use a tool.": null,
  // a helper asked for a task, done and tried: its task is the helper's description (helper-words.ts; UC-906, UC-907;
  // provisional; hi: later)
  "Asked a helper: {task}.": null,
  "Tried to ask a helper: {task}.": null,
  // UC-167; provisional
  "Tried to let a task reach the internet.": "एक काम को इंटरनेट तक पहुँचने देने की कोशिश की गई।",
  // a step named only by the agent's own words (its title or a task's description), tried and not done: those words
  // after the attempt, so the agent's imperative is never told as done; the words stay as the agent sent them
  "Tried: {title}.": "कोशिश की गई: {title}।",
  // a row's state, after its words, while it waits or once it carried on behind a steer
  Waiting: "प्रतीक्षा में",
  // a step whose state this version can't read (a newer Clauseo's): neither done nor failed, said as such
  "State not known": "स्थिति ज्ञात नहीं",
  // the folded line of a group whose every step is in such a state, and the row of one nothing names: no tense
  "State not known.": "स्थिति ज्ञात नहीं।",
  // the folded line's parts, the most done first, joined with commas into one sentence (step-summary.ts); until 24
  // Nov a sentence each ("4 papers read."), the ids kept
  "read 1 paper": null,
  "read {n} papers": null,
  "changed 1 paper": null,
  "changed {n} papers": null,
  "deleted 1 paper": null,
  "deleted {n} papers": null,
  "moved 1 paper": null,
  "moved {n} papers": null,
  "searched the papers": null,
  "looked online": null,
  "looked at {sites} online": null,
  "ran 1 command": null,
  "Ran {n} commands.": null,
  "Planned the work.": "काम की योजना बनाई गई।",
  "used {names}": null,
  "used {n} skills": null,
  // past the three kinds done most
  "And 1 more.": "और 1 काम।",
  "And {n} more.": "और {n} काम।",
  // what failed or was stopped stays on the folded line, counted on its own after the steps that finished, never as
  // "of them" (the summary counts only what finished, so "of them" would point at nothing it named); a group with
  // nothing finished says what it tried, once
  "1 failed.": "1 विफल रहा।",
  "{n} failed.": "{n} विफल रहे।",
  "1 stopped.": "1 रोका गया।",
  "{n} stopped.": "{n} रोके गए।",
  // a step a steer moved to the background stays told on the folded line (its row says "In the background", U7's).
  // Hindi in the cut the failed and stopped lines had, and in U7's own words for it ("वह काम पृष्ठभूमि में चलता रहा")
  "It went on in the background.": "यह पृष्ठभूमि में चलता रहा।",
  "1 of them went on in the background.": "उनमें से 1 पृष्ठभूमि में चलता रहा।",
  "{n} of them went on in the background.": "उनमें से {n} पृष्ठभूमि में चलते रहे।",
  // the live line, after what is being done now
  "1 done so far.": "अब तक 1 पूरा हुआ।",
  "{n} done so far.": "अब तक {n} पूरे हुए।",
  // a step of Clauseo's case-law connector, done and tried (tool-names.ts, caseLawOf): case law searched (UC-476,
  // UC-477); the lawyer's credits read (UC-480, UC-481); a payment link got, never a payment (UC-482, UC-483).
  // Provisional; hi: later
  "Searched case law.": null,
  "Tried to search case law.": null,
  // a judgment read, a citator call, citations checked: each named apart (UC-1480..1485; provisional; hi: later)
  "Read a judgment.": null,
  "Tried to read a judgment.": null,
  "Checked who cites it.": null,
  "Tried to check who cites it.": null,
  "Checked the citations.": null,
  "Tried to check the citations.": null,
  "Checked your case-law credits.": null,
  "Tried to check your case-law credits.": null,
  "Got a payment link for case-law credits.": null,
  "Tried to get a payment link for case-law credits.": null,
  // the folded line's parts for the case-law connector's steps, counted by what each does (group-kind.ts):
  // case law searched (UC-493, UC-494); the credits read (UC-495, UC-496); payment links got (UC-497, UC-498).
  // Provisional; hi: later
  "searched case law": null,
  "searched case law {n} times": null,
  // judgments read, citators run, citations checked (UC-1489..1494; provisional; hi: later)
  "read 1 judgment": null,
  "read {n} judgments": null,
  "checked who cites a judgment": null,
  "checked who cites {n} judgments": null,
  "checked the citations": null,
  "checked the citations {n} times": null,
  "checked your case-law credits": null,
  "checked your case-law credits {n} times": null,
  "got a payment link for case-law credits": null,
  "got {n} payment links for case-law credits": null,
  // issue 13 (UC-1100 to UC-1114; provisional; hi: later): a step named by what ran. A tool by its readable name
  // ("Indian Kanoon: search judgments"), a skill by its name, a command by the agent's own words for it, a helper
  // asked with no task given
  "Used {tool}.": null,
  "Tried to use {tool}.": null,
  "Used the {skill} skill.": null,
  "Tried to use the {skill} skill.": null,
  "Ran: {description}.": null,
  "Tried to run: {description}.": null,
  "Asked a helper.": null,
  "Tried to ask a helper.": null,
  // the folded line's parts, joined with commas into one sentence (step-summary.ts): what ran, named
  "ran {n} commands": null,
  "used the {skill} skill": null,
  "used 1 skill": null,
  "asked a helper": null,
  "asked {n} helpers": null,
  "planned the work": null,
  "used a tool": null,
  // a page that couldn't open, and a step Claude's safety check held back: neither told as failed (step-misses.ts;
  // Windows tester item C; UC-1185 to UC-1198; provisional; hi: later)
  "Couldn't open {site}: {reason}.": null,
  "Couldn't open {site}.": null,
  "Couldn't open a page: {reason}.": null,
  "Couldn't open a page.": null,
  "the site refused": null,
  "not found": null,
  "too large": null,
  "couldn't reach the site": null,
  "the site took too long": null,
  "Couldn't open 1 page.": null,
  "Couldn't open {n} pages.": null,
  "1 held back by Claude's safety check.": null,
  "{n} held back by Claude's safety check.": null,
  "Held back by Claude's safety check": null,
  // Claude's Yield, its wait for its own background work (helpers, background commands): UC-1199, UC-1202, UC-1203;
  // provisional; hi: later
  "Waited for the background work.": null,
  "Tried to wait for the background work.": null,
  "waited for the background work": null,
  // Claude's own tools by what they did, never their names (tool-names.ts, ownToolOf); a read Clauseo's off-limits
  // rule held back (step-misses.ts). UC-1261, UC-1265 to UC-1273; provisional; hi: later
  "Tried to plan the work.": null,
  "Stopped background work.": null,
  "Tried to stop background work.": null,
  "Asked you.": null,
  "Tried to ask you.": null,
  "stopped background work": null,
  "asked you": null,
  "Clauseo keeps this off limits": null,
  "1 kept off limits by Clauseo.": null,
  "{n} kept off limits by Clauseo.": null,
  // ChatGPT's wait for its helpers, and its other helper tools (helper-words.ts; UC-1274, UC-1282; provisional; hi:
  // later)
  "Waited for helpers.": null,
  "Worked with a helper.": null,
  // a helper step that failed, the helper already asked; its part of the folded line; an edit, deletion, move or
  // command held back, counted in the trouble line apart from what failed (UC-1283 to UC-1286; provisional; hi: later)
  "Tried to work with a helper.": null,
  "worked with a helper": null,
  "1 held back.": null,
  "{n} held back.": null,
  // a helper's card in the answer (helper-card.tsx, UC-1115 to UC-1124, UC-1127 to UC-1129).
  // Provisional; hi: later
  "Working for {time}": null,
  "Took {time}": null,
  "Failed after {time}": null,
  "Stopped after {time}": null,
  "less than a minute": null,
  "{n} min": null,
  "Finished.": null,
  "It ended without reporting back to {agent}.": null,
  "Before that: {what}": null,
  "By then: {what}": null,
  "Paper reader": null,
  Researcher: null,
  "Helper {n}": null,
  // a code mode script that failed (stage B's B4 notice "script_failed"): one quiet line inside the step group, never
  // counted (script-failed.ts; UC-1246; provisional; hi: later)
  "A script didn't run; Claude tried another way.": null,
  // a helper the agent cut at its turn limit: done, the limit said plainly (helper-card.tsx;
  // UC-1340; provisional, hi provisional)
  "It stopped at its turn limit before reporting back to {agent}.":
    "{agent} को बताने से पहले यह अपनी बारी की सीमा पर रुक गया।",
  // a helper still working after its answer ended: Claude's background helper, which the host
  // leaves open when its turn ends by itself (helper-view.ts stateWords; UC-1341; provisional, hi
  // provisional)
  "Working in the background": "पृष्ठभूमि में काम जारी है",
  // who asks, above a helper's permission request or question (helper-asker.tsx; UC-1342,
  // UC-1343; provisional, hi provisional)
  "{helper} asks:": "{helper} पूछ रहा है:",
  "A helper asks:": "एक सहायक पूछ रहा है:",
  // before what a stopped or failed helper had written so far (helper-card.tsx; UC-1344, UC-1345;
  // provisional, hi provisional)
  "What it had written when it stopped:": "रुकने तक इसने यह लिखा था:",
  "What it had written when it failed:": "विफल होने तक इसने यह लिखा था:",
} as const;

/** Every key, for the banned-words screen in the tests (null: hi: later). */
export const stepWords: Readonly<Record<keyof typeof HI, string | null>> = HI;
