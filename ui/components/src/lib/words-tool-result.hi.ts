// The Hindi of words-tool-result.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-tool-result.ts imports this file's type alone.

export const HI = {
  // U17: the row's press, closed and open
  "Show the result": "परिणाम दिखाएँ",
  "Hide the result": "परिणाम छिपाएँ",
  // the blocks it didn't draw: pictures, then anything else
  "1 picture isn't shown here.": "1 चित्र यहाँ नहीं दिखाया गया है।",
  "{n} pictures aren't shown here.": "{n} चित्र यहाँ नहीं दिखाए गए हैं।",
  "1 more item isn't shown here.": "1 और आइटम यहाँ नहीं दिखाया गया है।",
  "{n} more items aren't shown here.": "{n} और आइटम यहाँ नहीं दिखाए गए हैं।",
  // past 12 lines, and past the shown cap
  "Show all": "सभी दिखाएँ",
  "The rest of this result isn't shown here.": "इस परिणाम का बाकी हिस्सा यहाँ नहीं दिखाया गया है।",
  "Nothing came back.": "कुछ भी वापस नहीं आया।",
  // a blob's read past 400 ms, and a blob too large to read or whose read failed
  "Reading the result.": "परिणाम पढ़ा जा रहा है।",
  "This result is too large to show here.": "यह परिणाम यहाँ दिखाने के लिए बहुत बड़ा है।",
} as const;
