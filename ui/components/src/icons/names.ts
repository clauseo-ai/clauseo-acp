// Every icon in icons.tsx, with the shape it was cut from and what it means on screen.

/** Every icon icons.tsx exports (its Lucide shapes, and the glyphs it re-exports from glyphs.tsx), with the shape it
 * was cut from and what it means in the room. */
export const ICON_NAMES = [
  { component: "IconFold", source: "chevron", meaning: "A fold" },
  { component: "IconMore", source: "chevronDown", meaning: "N more" },
  { component: "IconListOfDates", source: "doc", meaning: "List of dates" },
  { component: "IconScratchMatter", source: "scratch", meaning: "Scratch matter" },
  { component: "IconChecking", source: "wait", meaning: "Checking" },
  { component: "IconGoodLaw", source: "check", meaning: "Good law" },
  {
    component: "IconTick",
    source: "check",
    meaning: "A tick: the chosen row, a checked box, a thing done",
  },
  { component: "IconOpen", source: "open", meaning: "Open" },
  { component: "IconTurnToIt", source: "arrow", meaning: "Turn to it" },
  { component: "IconBackToAnswer", source: "back", meaning: "Back to the answer" },
  { component: "IconMatters", source: "panel", meaning: "Matters" },
  { component: "IconSearch", source: "search", meaning: "Search this matter" },
  { component: "IconHistory", source: "history", meaning: "History" },
  { component: "IconUndo", source: "undo", meaning: "Undo" },
  { component: "IconNewMatter", source: "plus", meaning: "New matter" },
  { component: "IconAddPapers", source: "addPaper", meaning: "Add papers" },
  {
    component: "IconPaperBookPage",
    source: "paperBook",
    meaning: "Paper book page, not used in the room yet",
  },
  { component: "IconAccept", source: "accept", meaning: "Accept into the matter" },
  {
    component: "IconPageTurn",
    source: "pageTurn",
    meaning: "The prepared draft, waiting to be turned to",
  },
  { component: "IconSafeWorkspace", source: "safeRoom", meaning: "Safe workspace" },
  { component: "IconNotProtected", source: "notProtected", meaning: "Not protected" },
  { component: "IconFolder", source: "folder", meaning: "A folder beyond the matter" },
  { component: "IconWebsite", source: "website", meaning: "A website" },
] as const;
