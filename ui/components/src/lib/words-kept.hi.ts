// The Hindi of words-kept.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts
// gathers every words file's Hindi into one lazy chunk, and words-kept.ts imports this file's type
// alone. A permission is अनुमति and kept रखी गई, as the thread's "Always for this matter" (इस
// मामले के लिए हमेशा, words-thread.hi.ts) and its once-only notices say them; a matter is मामला.
// Every string here is new and provisional (UC-500 to UC-505).

export const HI = {
  // the rail's matter menu item that opens the list (new, provisional)
  "Kept permissions…": "रखी गई अनुमतियाँ…",
  // the list's title and its one line under it (new, provisional)
  "Kept permissions": "रखी गई अनुमतियाँ",
  "Claude doesn't ask before these in this matter: you chose “Always for this matter”.":
    "इस मामले में Claude इनसे पहले नहीं पूछता: आपने “इस मामले के लिए हमेशा” चुना था।",
  // the empty list (new, provisional)
  "Nothing is kept for this matter. Claude asks each time.":
    "इस मामले के लिए कुछ नहीं रखा गया है। Claude हर बार पूछता है।",
  // one row per kept rule, by its kind; each slot is isolated: a command, a file (relative to the
  // matter folder when inside it), a site's host, an MCP server and its tool (new, provisional)
  "Run: {command}": "चलाना: {command}",
  "Edit: {file}": "बदलना: {file}",
  "Read: {file}": "पढ़ना: {file}",
  "Any page on {site}": "{site} का कोई भी पेज",
  "Use tool: {server} {tool}": "टूल का इस्तेमाल: {server} {tool}",
  // each row's button, and its accessible name: the row's own words (new, provisional)
  Remove: "हटाएँ",
  "Remove {permission}": "{permission} हटाएँ",
  // the list's one way out (new, provisional)
  Close: "बंद करें",
} as const;
