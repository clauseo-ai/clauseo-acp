// The Hindi of words-blocklist.ts, keyed by its English. Loaded only for a Hindi display:
// words-hi.ts gathers every words file's Hindi into one lazy chunk, and words-blocklist.ts imports
// this file's type alone. "The unsafe-site list" is असुरक्षित वेबसाइटों की सूची, and blocked
// अवरुद्ध, as the blocklist.override_off error says them (words-errors.hi.ts); a matter is मामला.

export const HI = {
  // the notice in the conversation: its landmark name, what was blocked and why; {host} isolated
  "Unsafe site blocked": "असुरक्षित वेबसाइट अवरुद्ध",
  "Clauseo blocked {host}.": "Clauseo ने {host} को अवरुद्ध किया।",
  "This site is linked to harmful software.": "यह वेबसाइट हानिकारक सॉफ़्टवेयर से जुड़ी है।",
  "This site may try to steal passwords or details.":
    "यह वेबसाइट पासवर्ड या दूसरी जानकारी चुराने की कोशिश कर सकती है।",
  // {n} is 2 or more: one block says no count
  "Blocked {n} times in this answer.": "इस उत्तर में {n} बार अवरुद्ध किया गया।",
  "It's on Clauseo's unsafe-site list.": "यह Clauseo की असुरक्षित वेबसाइटों की सूची में है।",
  // the quiet detail after it: the date the list in use was made
  "List of {date}": "{date} की सूची",
  // the notice's two actions, and its line once one went through
  "Allow for this matter": "इस मामले के लिए अनुमति दें",
  "Always allow": "हमेशा अनुमति दें",
  // the first as words-versions.hi.ts has it
  "Allowed for this matter": "इस मामले के लिए अनुमति",
  "Allowed always": "हमेशा के लिए अनुमति",
  // the confirm before an allow: the site, the reason (above) and how far it reaches
  "Allow {host} for this matter?": "इस मामले के लिए {host} को अनुमति दें?",
  "Always allow {host}?": "{host} को हमेशा अनुमति दें?",
  "From now on, Clauseo can open this site in this matter.":
    "अब से Clauseo इस मामले में यह वेबसाइट खोल सकेगा।",
  "From now on, Clauseo can open this site in every matter.":
    "अब से Clauseo हर मामले में यह वेबसाइट खोल सकेगा।",
  "You can take this back in Settings, under Privacy.":
    "आप इसे सेटिंग्स में, निजता के अंतर्गत, वापस ले सकते हैं।",
  Allow: "अनुमति दें",
  // Settings, Privacy: the allowed sites' group
  "Allowed sites": "अनुमति-प्राप्त वेबसाइटें",
  "Sites you allowed although they're on the unsafe-site list.":
    "वे वेबसाइटें जिन्हें आपने असुरक्षित वेबसाइटों की सूची में होने के बावजूद अनुमति दी।",
  "No sites are allowed past the unsafe-site list.":
    "असुरक्षित वेबसाइटों की सूची की किसी वेबसाइट को अनुमति नहीं दी गई है।",
  // a row's reach: one matter by its name, or every matter
  "This matter: {matter}": "यह मामला: {matter}",
  // a matter allow whose matter this window doesn't know (a forgotten matter's allows are kept)
  "One matter": "एक मामला",
  Always: "हमेशा",
  "Allowed {date}": "{date} को अनुमति दी गई",
  // a site Clauseo couldn't save: its allow lasts until Clauseo closes
  "Until Clauseo closes": "Clauseo के बंद होने तक",
  Remove: "हटाएँ",
  // the Remove button's accessible name
  "Remove {host}": "{host} हटाएँ",
  // the lead line of a note for IT ("Copy details for IT", room/blocklist/it-details.ts), where the
  // firm forbids overriding the list (new, provisional)
  "Clauseo blocked a site on its unsafe-site list, and the firm's policy has turned off allowing it.":
    "Clauseo ने अपनी असुरक्षित वेबसाइटों की सूची की एक वेबसाइट अवरुद्ध की, और फ़र्म की नीति ने उसे अनुमति देना बंद कर रखा है।",
} as const;
