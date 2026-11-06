// The Hindi of words-protection.ts, keyed by its English. Loaded only for a Hindi display (words-hi.ts).
// The waiting lines are the design's own, with its Hindi; every other line is new (UC), provisional in English, its
// Hindi null until it comes (hi: later).

export const HI = {
  // the waiting lines, the design's (UC-14, UC-15): here, not in words-first-launch.ts, so the room's protection chunk
  // and the first run's chunk share no words chunk of their own
  "Choose Yes in the window Windows shows.": "Windows जो विंडो दिखाता है, उसमें Yes चुनें।",
  "Setting up the Safe workspace.": "सुरक्षित कार्यक्षेत्र तैयार हो रहा है।",
  // hi: later from here on
  // the set-up confirm (UC-11, UC-12, UC-40)
  "Set up protection?": null,
  "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to set up protection.":
    null,
  "Set up protection": null,
  // the remove confirm (UC-41 to UC-44)
  "Remove protection's set-up?": null,
  "Windows will ask if you want to allow changes, and it may show the publisher as unknown. That's expected. Choose Yes to remove protection's set-up.":
    null,
  "Afterwards, Clauseo works on this computer without protection until you set it up again.": null,
  "Remove set-up": null,
  // the notes (UC-22, UC-23, UC-25)
  "Protection isn't set up on this computer yet. Until it is, Clauseo can read any file on this computer, and asks before changing anything outside this matter's folder.":
    null,
  "Protection couldn't be set up on this computer, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.":
    null,
  "This computer can't give Clauseo's protection, so Clauseo can read any file on it. It asks before changing anything outside this matter's folder.":
    null,
  // a build without protection (R17 PR-B's unsupported { why: "not_built" }, every real build until desktop wires
  // protection in), said calmly (UC-38)
  "This build of Clauseo comes without protection, so Clauseo can read any file on this computer. It asks before changing anything outside this matter's folder.":
    null,
  // the pool, counts only (UC-27 to UC-32, UC-34, UC-35)
  "All {size} of this computer's protected spaces are in use, so this matter works without protection for now. It's protected again once one is free.":
    null,
  "In use by other open matters: {n}": null,
  "Kept by matters with nothing open: {n}": null,
  "Freed the next time Clauseo starts work: {n}": null,
  "Still holding files set aside: {n}": null,
  "To free one now, end a conversation in another matter.": null,
  "Clauseo can add room for 4 more. Windows asks for permission first.": null,
  "Add room": null,
  // Windows error 1385, once R17 PR-E names it (UC-33)
  "Windows policy on this computer doesn't let Clauseo's protection accounts sign in. Ask your IT team to add them to the Allow log on locally policy.":
    null,
  // the firm (UC-50, UC-51)
  "Your firm requires protection on this computer. Set it up once, then ask here.": null,
  "Your firm requires protection, and all of this computer's protected spaces are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.":
    null,
} as const;
