// The Hindi of words-library.ts, keyed by its English: L1's none yet (hi: later), so a Hindi
// display shows the English; L2's provisional. Loaded only for a Hindi display: words-hi.ts
// gathers every words file's Hindi into one lazy chunk, and words-library.ts imports this file's
// type alone. {who} and {Who} are the assistant: "Claude", "ChatGPT", or "the assistant" when both
// plans are signed in.

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
  // UC-1227 to UC-1230, UC-1232: Clauseo Case Law, unreachable or added
  "Can't reach it right now": null,
  Retry: null,
  "Clauseo can't reach Clauseo Case Law right now, so {who} can't search judgments.": null,
  "{credits} credits left": null,
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
  // UC-1242: Clauseo Case Law's detail while the lawyer has turned connectors off
  "Connectors are turned off. You can turn them on in Settings, under Connectors.": null,
  // UC-1243, UC-1244: Clauseo Case Law added once, its sign-in ended or its key revoked: the card's
  // word, and its detail above Open Settings
  "Signed out": null,
  "Sign in again in Settings, under Connectors.": null,
  // UC-1245: the button beside Turned off and Signed out: opens Settings at Connectors (the place's own
  // words, so its chunk shares no words file with the tool line's popover)
  "Open Settings": null,
  // UC-1248: Clauseo Case Law's card and detail when it answered but isn't offered yet, or this
  // install has no connectors: no button
  "Not available yet.": null,
  // Library L2 (UC-1301 to UC-1339; UC-1327 and UC-1336 retired), with provisional Hindi.
  // UC-1301 to UC-1303: the head's lens, "Switches for [matter | Every matter]", and each row
  // switch's accessible name
  "Switches for": "स्विच इसके लिए",
  "{name}, on in {matter}": "{name}, {matter} में चालू",
  "{name}, on in every matter": "{name}, हर मामले में चालू",
  // UC-1304, UC-1305: where an item is on, beside "Every matter" and "Only {matters}"
  "Every matter but {n}": "{n} को छोड़कर हर मामला",
  "Only {n} matters": "सिर्फ़ {n} मामले",
  // UC-1306 to UC-1310: the detail's "Where {who} uses it": the two choices, the tick boxes' note
  // on the matter open last, and when a change applies
  "Only the matters you tick": "सिर्फ़ वे मामले जिन पर आप निशान लगाएँ",
  "New matters too. Untick a matter to leave it out.":
    "नए मामले भी। किसी मामले को छोड़ने के लिए उसका निशान हटाएँ।",
  "New matters start without it.": "नए मामले इसके बिना शुरू होते हैं।",
  "open last": "आख़िरी बार खुला",
  "Changes apply from each conversation's next message.": "बदलाव हर बातचीत के अगले संदेश से लागू होते हैं।",
  // UC-1311 to UC-1315: Remove and its confirmation
  Remove: "हटाएँ",
  "Remove {name}?": "{name} हटाएँ?",
  "Clauseo deletes its files from this computer and turns it off in every matter. To use it again, add it again.":
    "Clauseo इसकी फ़ाइलें इस कंप्यूटर से मिटा देता है और इसे हर मामले में बंद कर देता है। इसे फिर से इस्तेमाल करने के लिए दोबारा जोड़ें।",
  "Clauseo removes it and its saved keys from this computer, and turns it off in every matter.":
    "Clauseo इसे और इसकी सहेजी गई कुंजियाँ इस कंप्यूटर से हटा देता है, और इसे हर मामले में बंद कर देता है।",
  Cancel: "रद्द करें",
  // UC-1316 to UC-1319: a built-in's line in place of Remove (a tool, then a skill), and the firm's
  // word on a row and in the detail
  "Comes with Clauseo, so it stays. Turn it off where you don't want it.":
    "Clauseo के साथ आता है, इसलिए यह रहता है। जहाँ न चाहिए, वहाँ इसे बंद करें।",
  "Comes with Clauseo, so it stays on in every matter.":
    "Clauseo के साथ आता है, इसलिए यह हर मामले में चालू रहता है।",
  "Set by your firm": "आपकी फ़र्म ने तय किया",
  "Your firm decides where this is used and whether it stays.":
    "यह कहाँ इस्तेमाल हो और रहे या नहीं, यह आपकी फ़र्म तय करती है।",
  // UC-1320 to UC-1323: a refused change, one line each: addon.unknown, addon.built_in,
  // settings.locked, and any other refusal
  "Clauseo couldn't find {name} on this computer, so nothing changed.":
    "Clauseo को इस कंप्यूटर पर {name} नहीं मिला, इसलिए कुछ नहीं बदला।",
  "{name} comes with Clauseo, so this can't be changed.":
    "{name} Clauseo के साथ आता है, इसलिए इसे बदला नहीं जा सकता।",
  "Your firm decides this for {name}, so nothing changed.":
    "{name} के लिए यह आपकी फ़र्म तय करती है, इसलिए कुछ नहीं बदला।",
  "Clauseo couldn't make that change to {name}. Try again.":
    "Clauseo {name} में वह बदलाव नहीं कर सका। फिर से कोशिश करें।",
  // UC-1324 to UC-1326: the explainer on what the switches do, by the lens
  "Every matter, or some": "हर मामला, या कुछ",
  "The switch on each row is for {matter}, the matter you had open. Choose Every matter above to change what new matters start with.":
    "हर पंक्ति का स्विच {matter} के लिए है, वह मामला जो आपने खोला था। नए मामले किसके साथ शुरू हों, यह बदलने के लिए ऊपर हर मामला चुनें।",
  "The switch on each row turns it on or off in every matter. Choose a matter above to change only that matter.":
    "हर पंक्ति का स्विच उसे हर मामले में चालू या बंद करता है। सिर्फ़ एक मामला बदलने के लिए ऊपर वह मामला चुनें।",
  // UC-1328: where it is on, for scope matter with no matter switched on
  "Not used in any matter": "किसी मामले में इस्तेमाल नहीं होता",
  // UC-1329, UC-1330: a built-in connector while the switches are every matter's (its switch
  // greyed): the browser (its switch in Settings › Privacy), Clauseo Case Law (the connectors'
  // switch in Settings › Connectors)
  "Comes with Clauseo. Switch it off for a matter from that matter, or turn it off everywhere in Settings › Privacy.":
    "Clauseo के साथ आता है। किसी मामले के लिए उसे उसी मामले से बंद करें, या सेटिंग्स › निजता में हर जगह बंद करें।",
  "Comes with Clauseo. Switch it off for a matter from that matter, or turn connectors off everywhere in Settings › Connectors.":
    "Clauseo के साथ आता है। किसी मामले के लिए उसे उसी मामले से बंद करें, या सेटिंग्स › कनेक्टर में कनेक्टर हर जगह बंद करें।",
  // UC-1331 to UC-1334: an added item turned off itself (on in no matter, switch or not): its
  // row, then its detail, for a tool and a skill
  "Turned off in Settings › Tools, so no matter uses it. Switch it on here to turn it back on.":
    "सेटिंग्स › टूल में बंद है, इसलिए कोई मामला इसका इस्तेमाल नहीं करता। फिर से चालू करने के लिए इसे यहाँ चालू करें।",
  "Turned off in Settings › Skills, so no matter uses it. Switch it on here to turn it back on.":
    "सेटिंग्स › स्किल में बंद है, इसलिए कोई मामला इसका इस्तेमाल नहीं करता। फिर से चालू करने के लिए इसे यहाँ चालू करें।",
  "Turned off in Settings › Tools, so no matter uses it. Choose where to use it to turn it back on.":
    "सेटिंग्स › टूल में बंद है, इसलिए कोई मामला इसका इस्तेमाल नहीं करता। फिर से चालू करने के लिए चुनें कि इसे कहाँ इस्तेमाल करना है।",
  "Turned off in Settings › Skills, so no matter uses it. Choose where to use it to turn it back on.":
    "सेटिंग्स › स्किल में बंद है, इसलिए कोई मामला इसका इस्तेमाल नहीं करता। फिर से चालू करने के लिए चुनें कि इसे कहाँ इस्तेमाल करना है।",
  // UC-1335: under the choice not chosen, while some matter has its own choice
  "This replaces the choices made in each matter.": "इससे हर मामले में किए गए चुनाव बदल जाते हैं।",
  // UC-1337, UC-1338: a built-in tool turned off itself: the browser (Settings › Privacy), Case
  // Law (the connectors' switch)
  "Turned off in Settings › Privacy, so no matter uses it. Turn it on there first.":
    "सेटिंग्स › निजता में बंद है, इसलिए कोई मामला इसका इस्तेमाल नहीं करता। पहले इसे वहाँ चालू करें।",
  "Connectors are turned off in Settings › Connectors, so no matter uses it. Turn them on there first.":
    "सेटिंग्स › कनेक्टर में कनेक्टर बंद हैं, इसलिए कोई मामला इसका इस्तेमाल नहीं करता। पहले उन्हें वहाँ चालू करें।",
  // UC-1339: the matter's own read failed with no good answer before it
  "Clauseo couldn't check what {matter} gets just now. The switches show how each is set.":
    "Clauseo अभी जाँच नहीं सका कि {matter} को क्या मिलता है। स्विच दिखाते हैं कि हर एक कैसे सेट है।",
} as const;
