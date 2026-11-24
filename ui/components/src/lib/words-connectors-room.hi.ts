// The Hindi of words-connectors-room.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers
// every words file's Hindi into one lazy chunk, and words-connectors-room.ts imports this file's type alone.

export const HI = {
  // U15: the case-law hint under an answer; {price} is rupees() of the cheapest pack's gross price, GST included
  "For judgments, Clauseo can search Indian Kanoon directly.":
    "निर्णयों के लिए Clauseo सीधे Indian Kanoon पर खोज सकता है।",
  "It's a paid add-on, from {price}, including GST.": "यह सशुल्क ऐड-ऑन है, GST सहित {price} से शुरू।",
  "It's a paid add-on.": "यह सशुल्क ऐड-ऑन है।",
  "Set up in Settings": "सेटिंग्स में सेट अप करें",
  "Don't show again": "फिर न दिखाएँ",
  // the hint's landmark name
  "Indian Kanoon suggestion": "Indian Kanoon सुझाव",
  // the out-of-credit note under the composer's field
  "Out of case-law credits.": "निर्णय-खोज के क्रेडिट समाप्त।",
  "Searches of Indian Kanoon stop until you add more.":
    "और क्रेडिट जोड़ने तक Indian Kanoon पर खोज रुकी रहेगी।",
  "Add credits": "क्रेडिट जोड़ें",
  // the note's quiet icon button, its accessible name
  "Hide in this conversation": "इस बातचीत में छिपाएँ",
  // the home card offering the case-law connector, where no conversation is open yet (24 Nov round: Hindi later);
  // {price} is rupees() of the smallest pack's gross price from the host's live price list, GST included;
  // no count of searches: what a pack buys depends on the question
  "Clauseo Case Law": null,
  "Find and read Supreme Court, High Court and tribunal judgments and central Acts, and see who cites each judgment.":
    null,
  "From {price}.": null,
  "Prepaid, including GST. No subscription.": null,
  // the terms alone, while the host lists no price
  "Prepaid credits. No subscription.": null,
  "Add Clauseo Case Law": null,
  "Powered by Indian Kanoon": null,
  // Clauseo Case Law out of credit, the same words in Settings, the library and by the composer (r7363; Hindi later):
  // with a figure left under one search's price, and at 0
  "Not enough credits for a search. Add credits to keep searching.": null,
  "No credits. Add credits to search.": null,
  // by the composer of a conversation refused for credits, once they arrive, until its next message
  "Credits added. Ask again to search case law.": null,
  // the case_law_out_of_credit notice's row once credit is back, at the point in the conversation it was refused
  "Clauseo Case Law had no credits for this search.": null,
  // Settings out of credit with no packs listed and no pay link: the sentences without "Add credits to…"
  "Not enough credits for a search.": null,
  "No credits.": null,
} as const;
