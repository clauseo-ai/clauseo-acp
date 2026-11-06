// The Hindi of words-matters.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-matters.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  /* ---------- the rail: finding a matter, a folder that moved ---------- */
  "Find a matter": "मामला खोजें",
  // RO-20: Find's placeholder and name once it searches inside (U16a)
  Find: "खोजें",
  "Find a matter by its name, client or case number": "नाम, मुवक्किल या केस नंबर से मामला खोजें",
  Recent: "हाल के",
  "Matching matters": "मेल खाते मामले",
  "No matter matches '{q}'.": "कोई मामला '{q}' से मेल नहीं खाता।",
  "Search covers names, clients and case numbers.": "खोज में नाम, मुवक्किल और केस नंबर शामिल हैं।",
  // where the host gives no client (today's), find searches names only; with clients, names and clients
  "Find a matter by its name": "नाम से मामला खोजें",
  "Find a matter by its name or client": "नाम या मुवक्किल से मामला खोजें",
  "Search covers names.": "खोज में नाम शामिल हैं।",
  "Search covers names and clients.": "खोज में नाम और मुवक्किल शामिल हैं।",
  // the scratch matter's line under its title in the toolbar
  "Quick questions, kept like any matter": "त्वरित प्रश्न, किसी भी मामले की तरह सहेजे गए",
  "Client, {client}": "मुवक्किल, {client}",
  "Folder moved": "फ़ोल्डर कहीं और ले जाया गया",
  "A matter waits for you": "एक मामला आपकी प्रतीक्षा में है",
  /* ---------- a new matter: a folder the host refused, a create that failed, a next date that doesn't read ---------- */
  // matter.folder_refused, which doesn't say why: true of a folder already a matter, an unreadable matter file, the
  // matters' own folder, a drive, the home folder and a system folder alike
  "Clauseo can't make a matter of this folder. Choose another folder.":
    "Clauseo इस फ़ोल्डर से मामला नहीं बना सकता। दूसरा फ़ोल्डर चुनें।",
  "{path} is a whole drive.": "{path} पूरी ड्राइव है।",
  "A matter is one folder of papers. Choose the folder for one matter.":
    "एक मामला दस्तावेज़ों का एक फ़ोल्डर होता है। किसी एक मामले का फ़ोल्डर चुनें।",
  "{path} is your home folder.": "{path} आपका होम फ़ोल्डर है।",
  "It holds far more than one matter. Choose the folder for one matter.":
    "इसमें एक मामले से कहीं अधिक है। किसी एक मामले का फ़ोल्डर चुनें।",
  "{path} is a folder the computer keeps for itself.":
    "{path} वह फ़ोल्डर है जिसे कंप्यूटर अपने लिए रखता है।",
  "Choose the folder that holds the matter's papers.": "वह फ़ोल्डर चुनें जिसमें मामले के दस्तावेज़ हैं।",
  "Clauseo can't read {path}.": "Clauseo {path} नहीं पढ़ पा रहा।",
  "Clauseo can't read that folder.": "Clauseo वह फ़ोल्डर नहीं पढ़ पा रहा।",
  "It may be locked, or on a drive that's no longer connected.":
    "हो सकता है वह लॉक हो, या ऐसी ड्राइव पर हो जो अब जुड़ी नहीं है।",
  "Clauseo couldn't create the matter. Try again, or choose another folder.":
    "Clauseo मामला नहीं बना सका। फिर से प्रयास करें, या दूसरा फ़ोल्डर चुनें।",
  "Write the date as 30 October 2026.": "तारीख इस तरह लिखें: 30 अक्टूबर 2026।",
  // matter.folder_is_matter: the folder is already a matter, by any spelling or nested with one; the matter's name when
  // the list has it, and Open it beside the line
  "This folder is already a matter: {name}.": "यह फ़ोल्डर पहले से एक मामला है: {name}।",
  "This folder is already a matter.": "यह फ़ोल्डर पहले से एक मामला है।",
  // matter.folder_in_matters_root: the folder Clauseo keeps matters in, a choose-another refusal
  "{path} is the folder Clauseo keeps matters in.": "{path} वह फ़ोल्डर है जिसमें Clauseo मामले रखता है।",
  "That folder is where Clauseo keeps matters.": "उस फ़ोल्डर में Clauseo मामले रखता है।",
  "Choose the folder for one matter.": "एक मामले का फ़ोल्डर चुनें।",
  /* ---------- a matter whose folder has moved ---------- */
  "This matter's folder has moved.": "इस मामले का फ़ोल्डर कहीं और ले जाया गया है।",
  "It was at {path}. Find it where it is now, and the matter opens as you left it.":
    "यह {path} पर था। यह अब जहाँ है वहाँ इसे ढूँढें, और मामला वैसे ही खुलेगा जैसा आपने छोड़ा था।",
  "Its conversations and versions are kept on this computer, so nothing is lost while the folder is away.":
    "इसकी बातचीत और संस्करण इसी कंप्यूटर पर रखे हैं, इसलिए फ़ोल्डर के न मिलने तक भी कुछ नहीं खोता।",
  "Locate the folder to ask about this matter": "इस मामले के बारे में पूछने के लिए फ़ोल्डर ढूँढें",
  /* ---------- the scratch matter's line ---------- */
  "A matter for quick questions, with no papers of its own.":
    "त्वरित प्रश्नों के लिए एक मामला, जिसके अपने कोई दस्तावेज़ नहीं हैं।",
  "Every question asked here is kept, with its versions, like any matter's.":
    "यहाँ पूछा गया हर प्रश्न, अपने संस्करणों के साथ, किसी भी मामले की तरह रखा जाता है।",
  /* ---------- conversations inside a matter ---------- */
  "New conversation": "नई बातचीत",
  "Conversations in {name}": "{name} की बातचीत",
  "Actions for {title}": "{title} के विकल्प",
  "Continue with {agent}": "{agent} के साथ जारी रखें",
  "A new conversation, with a summary of this one and a link back.":
    "एक नई बातचीत, इस बातचीत के सारांश और इस पर लौटने की कड़ी के साथ।",
  "Waiting for '{title}'": "'{title}' की प्रतीक्षा में",
  // a new conversation before its first message, at the reading column's top
  "A new conversation. It reads this matter's {n} papers.":
    "एक नई बातचीत। यह इस मामले के {n} दस्तावेज़ पढ़ती है।",
  // the same with one paper, which the design's line has no form for (U6's draft, QU.8)
  "A new conversation. It reads this matter's one paper.":
    "एक नई बातचीत। यह इस मामले का एक दस्तावेज़ पढ़ती है।",
  // H13: Retry from here on one of the lawyer's questions, the line a retried conversation opens with, and its way
  // back to the first
  "Retry from here": "यहाँ से फिर पूछें",
  "Retry from here starts a new conversation from this question. Nothing is deleted.":
    "यहाँ से फिर पूछने पर इसी प्रश्न से एक नई बातचीत शुरू होती है। कुछ भी हटाया नहीं जाता।",
  "Retried from your question at {time}. The first answer stays in '{title}'.":
    "{time} बजे के आपके प्रश्न से फिर पूछा गया। पहला उत्तर '{title}' में बना रहता है।",
  "Open it": "खोलें",
  /* ---------- messages while Clauseo works: the queue ---------- */
  Queue: "कतार में रखें",
  "Queued messages": "कतार के संदेश",
  "{n} queued, sent in order.": "{n} कतार में, क्रम से भेजे जाएँगे।",
  "1 queued, sent once this answer is done.": "1 कतार में, यह उत्तर पूरा होते ही जाएगा।",
  "The queue holds {n}.": "कतार में {n} संदेश रह सकते हैं।",
  "{n} more": "{n} और",
  "Show fewer": "कम दिखाएँ",
  Edit: "बदलें",
  Remove: "हटाएँ",
  "Send now": "अभी भेजें",
  Save: "सहेजें",
  "Edit queued message {n}": "कतार का संदेश {n} बदलें",
  "The queue is held. You stopped the answer at {time}, so nothing more is sent until you send.":
    "कतार रुकी है। आपने {time} बजे उत्तर रोका, इसलिए जब तक आप न भेजें, आगे कुछ नहीं भेजा जाएगा।",
  "The queue is held. You didn't allow a request at {time}, so nothing more is sent until you send.":
    "कतार रुकी है। आपने {time} बजे एक अनुरोध की अनुमति नहीं दी, इसलिए आपके भेजने तक और कुछ नहीं भेजा जाएगा।",
  "The queue is held. The answer failed at {time}, so nothing more is sent until you send.":
    "कतार रुकी है। {time} बजे उत्तर पूरा नहीं हो सका, इसलिए जब तक आप न भेजें, आगे कुछ नहीं भेजा जाएगा।",
  "Send next": "अगला भेजें",
  // a queued message's Edit, Remove or Send now refused with thread.turn_not_queued: it had gone as it was pressed
  "That message had already gone.": "वह संदेश पहले ही जा चुका था।",
  // held with no stop to name (a restart, a limit, an answer that ended well): the count alone, nothing sent until
  // the lawyer sends
  "{n} queued.": "{n} कतार में।",
  "1 queued.": "1 कतार में।",
  // UD2-57: under the count while the host holds the queue for an update install (#537's queue_held_reason)
  "Clauseo is installing an update. Your queued questions go once it's done.":
    "Clauseo एक अपडेट इंस्टॉल कर रहा है। इसके पूरा होते ही कतार में रखे आपके प्रश्न भेजे जाएँगे।",
  /* ---------- steering the running answer: the words a host gives SteerHint, SteerMark and SteerNote ---------- */
  "Send now gives your words to Clauseo while it writes.":
    "अभी भेजें से Clauseo लिखते समय ही आपके शब्द ले लेता है।",
  "ChatGPT can't take words mid-answer. Send now stops this answer, keeps what it wrote, and sends your words first.":
    "ChatGPT उत्तर के बीच में शब्द नहीं ले सकता। अभी भेजें इस उत्तर को रोकता है, जितना लिखा गया वह रखता है, और आपके शब्द पहले भेजता है।",
  "You, {time}, while Clauseo answered": "आप, {time}, जब Clauseo उत्तर दे रहा था",
  // where a steered message went: the answer had just ended (requeued), or it was stopped for the words (sent now)
  "Your words came as the answer ended, so they go next.":
    "आपके शब्द उत्तर पूरा होते समय पहुँचे, इसलिए वे अगले भेजे जाएँगे।",
  "Stopped at {time} to send your words first. The answer so far is kept.":
    "आपके शब्द पहले भेजने के लिए {time} बजे रोका गया। अब तक का उत्तर रखा गया है।",
  "Your words at {time} came in while Clauseo searched Indian Kanoon for {what}. That search carried on in the background, finished at {done}, and this answer uses it.":
    "आपके {time} बजे के शब्द तब पहुँचे जब Clauseo Indian Kanoon पर {what} खोज रहा था। वह खोज पृष्ठभूमि में चलती रही, {done} बजे पूरी हुई, और यह उत्तर उसका इस्तेमाल करता है।",
  "Show what it found": "क्या मिला, देखें",
  // tool calls a steer moved to the background (any agent, any tool): the note under the answer once they finished
  // before it ended, naming none (its list "Show what it found" names each), and the mark on each call's row
  "Your words at {time} came in before some work had finished. It went on in the background and finished at {done}. The answer uses what it found.":
    "आपके {time} बजे के शब्द कुछ काम पूरा होने से पहले पहुँचे। वह काम पृष्ठभूमि में चलता रहा और {done} बजे पूरा हुआ। उत्तर में उससे मिली बातों का इस्तेमाल हुआ है।",
  "In the background": "पृष्ठभूमि में",
  /* ---------- another conversation is changing the matter ---------- */
  "Waiting for '{title}' to finish, by {time}": "'{title}' के पूरा होने की प्रतीक्षा, {time} बजे तक",
  "Waiting for '{title}' to finish.": "'{title}' के पूरा होने की प्रतीक्षा है।",
  "One conversation changes a matter's papers at a time. Your message is ready to go.":
    "किसी मामले के दस्तावेज़ एक समय में एक ही बातचीत बदलती है। आपका संदेश भेजने के लिए तैयार है।",
  "Stop the other": "दूसरी रोकें",
} as const;

/** Every key, for the banned-words screen in the tests. */
export const matterWords: Hindi<keyof typeof HI> = HI;
