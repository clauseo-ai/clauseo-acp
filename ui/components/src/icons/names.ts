// Every icon in icons.tsx, with the shape it was cut from and what it means on screen.

/** Every icon icons.tsx exports (its Lucide shapes, the tool kinds' glyphs it re-exports from tool-kinds.tsx, and the
 * glyphs it re-exports from glyphs.tsx), with the shape it was cut from and what it means in the room. */
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
  {
    component: "IconArrowRight",
    source: "arrow",
    meaning: "Send, the arrow right, not used in the room yet",
  },
  {
    component: "IconArrowUp",
    source: "arrowUp",
    meaning: "Send, the arrow up, not used in the room yet",
  },
  {
    component: "IconSend",
    source: "send",
    meaning: "Send, the paper plane, not used in the room yet",
  },
  { component: "IconMatters", source: "panel", meaning: "Matters" },
  { component: "IconSearch", source: "search", meaning: "Search this matter" },
  { component: "IconHistory", source: "history", meaning: "Versions" },
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
  {
    component: "IconBreak",
    source: "circleAlert",
    meaning:
      "A break: the working line's lead when an answer stopped or failed, not used in the room yet",
  },
  { component: "IconOffline", source: "wifiOff", meaning: "Offline, not used in the room yet" },
  {
    component: "IconQuestion",
    source: "circleQuestionMark",
    meaning: "A question for you, not used in the room yet",
  },
  {
    component: "IconTryAgain",
    source: "rotateCcw",
    meaning: "Try again, Restart, Retry now, Try now, not used in the room yet",
  },
  {
    component: "IconStop",
    source: "square",
    meaning: "Stop, and a stopped matter or conversation, not used in the room yet",
  },
  {
    component: "IconCross",
    source: "x",
    meaning: "A cross: Don't allow, Not allowed, not used in the room yet",
  },
  {
    component: "IconConversation",
    source: "messageSquare",
    meaning: "A conversation: a version Clauseo made in one, not used in the room yet",
  },
  {
    component: "IconAnswerReady",
    source: "messageSquareDot",
    meaning: "Answer ready, not used in the room yet",
  },
  {
    component: "IconYourChange",
    source: "penLine",
    meaning: "A change you made outside Clauseo, not used in the room yet",
  },
  {
    component: "IconImported",
    source: "fileInput",
    meaning: "Papers imported, not used in the room yet",
  },
  {
    component: "IconCompare",
    source: "diff",
    meaning: "Compare, Compare with now, not used in the room yet",
  },
  {
    component: "IconLanguage",
    source: "languages",
    meaning: "Settings: Language, not used in the room yet",
  },
  {
    component: "IconAppearance",
    source: "contrast",
    meaning: "Settings: Appearance, not used in the room yet",
  },
  { component: "IconAbout", source: "info", meaning: "Settings: About, not used in the room yet" },
  { component: "IconPin", source: "pin", meaning: "Pin to the top, not used in the room yet" },
  {
    component: "IconShowInFinder",
    source: "folderOpen",
    meaning: "Show in Finder, or in Explorer, not used in the room yet",
  },
  {
    component: "IconLocateFolder",
    source: "folderSearch",
    meaning: "Locate folder, and a folder that moved, not used in the room yet",
  },
  {
    component: "IconRemoveFromList",
    source: "listX",
    meaning: "Remove from the list, not used in the room yet",
  },
  {
    component: "IconCommand",
    source: "squareSlash",
    meaning: "An agent's command, in the / menu, not used in the room yet",
  },
  {
    component: "IconNeedsPermission",
    source: "shieldQuestionMark",
    meaning: "Needs your permission, not used in the room yet",
  },
  {
    component: "IconPlanLimit",
    source: "gauge",
    meaning: "The plan's limit, not used in the room yet",
  },
  {
    component: "IconToolRead",
    source: "bookOpen",
    meaning: "A tool call that reads files or data, not used in the room yet",
  },
  {
    component: "IconToolDelete",
    source: "trash2",
    meaning: "A tool call that deletes files, not used in the room yet",
  },
  {
    component: "IconToolMove",
    source: "arrowRightLeft",
    meaning: "A tool call that moves or renames files, not used in the room yet",
  },
  {
    component: "IconToolExecute",
    source: "terminal",
    meaning: "A tool call that runs a command, not used in the room yet",
  },
  {
    component: "IconToolThink",
    source: "lightbulb",
    meaning: "A tool call that thinks or plans, with no side effect, not used in the room yet",
  },
  {
    component: "IconToolSwitchMode",
    source: "toggleRight",
    meaning: "A tool call that switches the session's mode, not used in the room yet",
  },
  {
    component: "IconToolOther",
    source: "wrench",
    meaning: "Any other tool call, not used in the room yet",
  },
  {
    component: "IconToolEdit",
    source: "penLine",
    meaning: "A tool call that edits files, not used in the room yet",
  },
  {
    component: "IconToolSearch",
    source: "search",
    meaning: "A tool call that searches, not used in the room yet",
  },
  {
    component: "IconToolFetch",
    source: "website",
    meaning:
      "A tool call that fetches from the network, a web fetch or a web search, not used in the room yet",
  },
  {
    component: "IconFileWord",
    source: "fileWord",
    meaning: "A Word document (.docx, .doc): the page with W on its tab, not used in the room yet",
  },
  {
    component: "IconFilePdf",
    source: "filePdf",
    meaning: "A PDF: the page with PDF on its tab, not used in the room yet",
  },
  {
    component: "IconFileMarkdown",
    source: "fileMarkdown",
    meaning: "A Markdown file (.md): the page with MD on its tab, not used in the room yet",
  },
  {
    component: "IconFileEmail",
    source: "fileEmail",
    meaning:
      "An email (.eml, .msg): the page with an envelope on its tab, not used in the room yet",
  },
  {
    component: "IconFileGeneric",
    source: "fileGeneric",
    meaning: "Any other file: the page alone, not used in the room yet",
  },
] as const;
