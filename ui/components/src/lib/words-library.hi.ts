// The Hindi of words-library.ts, keyed by its English: none yet (hi: later), so a Hindi display
// shows the English. Loaded only for a Hindi display: words-hi.ts gathers every words file's Hindi
// into one lazy chunk, and words-library.ts imports this file's type alone. {who} and {Who} are the
// assistant: "Claude", "ChatGPT", or "the assistant" when both plans are signed in.

export const HI = {
  // UC-1050 to UC-1052: the rail's row (its name is also the place's title), its count, and its tag
  // until opened
  "Skills and connectors": null,
  "{skills} skills, {connectors} connectors": null,
  New: null,
  // UC-1053 to UC-1058: the two parts, and the lists' group heads
  Skills: null,
  Connectors: null,
  "Comes with Clauseo": null,
  "Added by you": null,
  "Your own connectors": null,
  "MCP servers": null,
  // UC-1059 to UC-1068: the built-ins' names and their one line each
  "Legal drafting": null,
  "Drafts and revises notices, replies, opinions and agreements from the matter's papers, and checks every citation before handing the draft over.":
    null,
  "Legal research": null,
  "Researches Indian law over several searches, reads the later judgments that cite a case, and ties every quote to a page it read.":
    null,
  "Matter brief": null,
  "Keeps a short brief of the matter in its notes folder: parties, facts, forum, positions taken and open questions.":
    null,
  "Word documents": null,
  "Opens and edits Word files in the matter, with each change shown as a tracked change you can accept or reject.":
    null,
  "Web browser": null,
  "Opens web pages that need a real browser, such as court websites with forms to fill.": null,
  // UC-1069 to UC-1071: where an item is on; {matters} is one name or several joined
  "Every matter": null,
  "Only {matters}": null,
  "Turned off": null,
  // UC-1072 to UC-1075: the row's word when something is in the way
  Stopped: null,
  "Can't be used": null,
  "Not allowed": null,
  "Key missing": null,
  // UC-1076 to UC-1081: where an added item came from; {source} is "skills.sh, owner/name",
  // "GitHub, owner/name" or a link's site
  "From a folder on this computer": null,
  "From a zip file on this computer": null,
  "From {source}": null,
  "MCP server, a program on this computer": null,
  "MCP server at {host}": null,
  "MCP server, brought over from {app}": null,
  // UC-1082 to UC-1086: the side panel on Skills with nothing picked
  "What a skill is": null,
  "A skill is a page of instructions for one kind of work, such as drafting a legal notice the way your chamber does. When a question calls for it, {who} reads the skill and follows it.":
    null,
  "Most skills are instructions only. A skill that holds programs says so, because {who} may run those programs on this computer.":
    null,
  "Where skills come from": null,
  "skills.sh lists skills people have shared. A colleague can also send you a skill as a folder or a zip file.":
    null,
  // UC-1087 to UC-1089, UC-1215, UC-1216: the side panel on Connectors with nothing picked
  "What a connector is": null,
  "A connector lets {who} reach something outside the matter's papers: a case-law database, a website, a program your firm runs. When one is used, the conversation shows it as a step.":
    null,
  "Your own connectors are MCP servers": null,
  "MCP is the common way apps such as Claude Desktop connect to outside tools, so one made for those apps works here too. You add it by its address on the internet, or by the command that starts it on this computer.":
    null,
  "It runs with the same access you have: your files, your programs and the internet. Add only connectors you trust.":
    null,
  // UC-1217 to UC-1221: an item's detail, its labels
  "Where {who} uses it": null,
  "How Clauseo reaches it": null,
  Address: null,
  Command: null,
  "Saved values": null,
  // UC-1222 to UC-1226: an item's detail, its notes and what stands in its way
  "This skill holds programs. {Who} may run them on this computer, with the same access you have.":
    null,
  "It comes with its own skill, Using the browser, with rules for using a browser on legal work. The sites it may open are in Settings, under Allowed sites.":
    null,
  "Its files changed after you added it, so Clauseo stopped using it.": null,
  "Your firm doesn't allow this one on this computer.": null,
  "Its saved key is missing, so {who} can't reach it.": null,
  // UC-1227 to UC-1232: Clauseo Case Law, unreachable or added
  "Can't reach it right now": null,
  Retry: null,
  "Clauseo can't reach Clauseo Case Law right now, so {who} can't search judgments.": null,
  "{credits} credits left": null,
  "About {n} searches": null,
  "No credits left": null,
  // UC-1233 to UC-1236: Clauseo Case Law picked before it is added
  "What it covers": null,
  "Judgments of the Supreme Court, the High Courts and tribunals, and central Acts. For any judgment, {who} can see which later judgments cite it, and how.":
    null,
  "What it costs": null,
  "A page of search results is 20 credits. Opening a judgment is 3 credits, plus 1 for each 4,000 tokens {who} reads (about 3,000 words).":
    null,
  // UC-1237, UC-1238: a list with nothing added yet
  "You haven't added a skill yet.": null,
  "You haven't added a connector of your own yet.": null,
  // UC-1239, UC-1240: {who} and {Who} when both plans are signed in, so no one assistant is named
  "the assistant": null,
  "The assistant": null,
  // UC-1241: the button beside "Can't reach it right now" until list_addons can check again itself:
  // it opens Settings at Connectors, whose opening checks
  "Open Settings to check": null,
  // UC-1242: Clauseo Case Law's detail while the lawyer has turned connectors off
  "Connectors are turned off. You can turn them on in Settings, under Connectors.": null,
  // UC-1243, UC-1244: Clauseo Case Law added once, its sign-in ended or its key revoked: the card's
  // word, and its detail above Open Settings
  "Signed out": null,
  "Sign in again in Settings, under Connectors.": null,
  // UC-1245: the button beside Turned off and Signed out: opens Settings at Connectors (the place's own
  // words, so its chunk shares no words file with the tool line's popover)
  "Open Settings": null,
} as const;
