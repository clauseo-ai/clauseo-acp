// The Hindi of words-settings.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-settings.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the window and its sections
  Sections: "अनुभाग",
  Language: "भाषा",
  Appearance: "दिखावट",
  About: "परिचय",
  "Plan and usage": "प्लान और उपयोग",
  Privacy: "निजता",
  // what each section says beside its name, at a glance
  "{theme}, text size {n}": "{theme}, अक्षरों का आकार {n}",
  "Version {version}": "संस्करण {version}",
  "Released {date}": "{date} को जारी",
  "{n}% of the 5-hour limit left": "5 घंटे की सीमा का {n}% बाकी",
  "Pay as you go": "उपयोग के अनुसार भुगतान",
  // a row the firm's policy locks (hi: later: the old Hindi said "your organisation")
  "Managed by firm": null,
  // each section's one line
  "The language of every word on screen. Papers, quotes and case names stay as filed.":
    "स्क्रीन के हर शब्द की भाषा। दस्तावेज़, उद्धरण और मामलों के नाम वैसे ही रहते हैं जैसे दर्ज हैं।",
  "How Clauseo looks on this computer.": "इस कंप्यूटर पर Clauseo कैसा दिखे।",
  "What each plan has left, and when it resets.": "हर प्लान में कितना बाकी है, और वह कब रीसेट होता है।",
  // plan and usage: the plan's own words are the limits' (words-limits.ts)
  "Updated at {time}": "{time} बजे अपडेट किया गया",
  "No limits to show. Each question is billed to the key's account.":
    "दिखाने को कोई सीमा नहीं। हर प्रश्न का शुल्क उसी key के खाते पर लगता है।",
  // language and appearance
  "Changes at once, and everything stays where it was.": "तुरंत बदलती है, और सब कुछ वहीं रहता है जहाँ था।",
  Day: "दिन",
  Night: "रात",
  // appearance's preview: the room in small, as the choices make it
  "How it looks": "यह कैसा दिखता है",
  "A strong case, because the complaint names Mr. Sharma only as a director.":
    "मामला मज़बूत है, क्योंकि शिकायत में श्री शर्मा को केवल निदेशक के रूप में नामित किया गया है।",
  // privacy: what leaves this computer, and the tools' switches
  "What leaves this computer": "इस कंप्यूटर से क्या बाहर जाता है",
  "To {name}, on your plan": "{name} को, आपके प्लान से",
  // hi: later: the lines below ship in English, their Hindi null until it comes
  // the plans' group note: what goes to each plan, said once
  "Questions, the papers' parts they need, web searches.": null,
  // behind an info icon: the button's name, then what it holds
  "More about Brave Search": null,
  "Anthropic lists Brave Search as its web search subprocessor.": null,
  "To the websites Clauseo visits": null,
  "About websites": null,
  "Websites see Clauseo's visits, as they see any browser's.": null,
  // the tools' switches; the group's note, the browser's reason and the lock's are the room's (words-tools.ts,
  // words-tools-line.ts)
  "On the web": null,
  "Web search": null,
  "Search the web and read pages.": null,
  Browser: null,
  "Runs on this computer.": null,
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const settingsWords: Hindi<keyof typeof HI> = HI;
