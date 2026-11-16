// The Hindi of words-refusals.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every
// words file's Hindi into one lazy chunk, and words-refusals.ts imports this file's type alone. The words follow the
// other files' glossary: मामला for a matter, जाँच for the check, "कुछ नहीं बदला" for nothing was changed, and
// Claude and Clauseo in Latin script.

export const HI = {
  // the safety check's reason, read from its tag: what the action looked like
  "Claude's built-in safety check stopped this: it looked like reaching files or systems Claude wasn't asked to use. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह ऐसी फ़ाइलों या सिस्टम तक पहुँचने जैसा लगा, जिनके उपयोग के लिए Claude से नहीं कहा गया था। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like sending matter information somewhere outside. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह मामले की जानकारी कहीं बाहर भेजने जैसा लगा। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like a search for passwords or sign-in keys. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह पासवर्ड या साइन-इन कुंजियाँ खोजने जैसा लगा। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like deleting or overwriting work in a way that can't be undone. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह काम को इस तरह हटाने या उस पर दोबारा लिखने जैसा लगा, जिसे वापस नहीं लाया जा सकता। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like getting around the checks on Claude's work. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह Claude के काम पर होने वाली जाँचों से बचने जैसा लगा। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like changing how Claude itself is set up. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह Claude की अपनी सेटिंग बदलने जैसा लगा। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like changing something outside this matter. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह इस मामले के बाहर कुछ बदलने जैसा लगा। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like following instructions hidden in a document or web page. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह किसी दस्तावेज़ या वेब पेज में छिपे निर्देशों का पालन करने जैसा लगा। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like running code from a source that isn't trusted. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह किसी ऐसे स्रोत का कोड चलाने जैसा लगा, जिस पर भरोसा नहीं है। कुछ नहीं बदला।",
  "Claude's built-in safety check stopped this: it looked like spending money or acting for you outside Clauseo. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया: यह पैसे ख़र्च करने या Clauseo के बाहर आपकी ओर से कुछ करने जैसा लगा। कुछ नहीं बदला।",
  // a reason it gave in words this list doesn't know
  "Claude's built-in safety check stopped this. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच ने इसे रोक दिया। कुछ नहीं बदला।",
  // the check gave no verdict (it was busy, or couldn't read this), so the action didn't run
  "Claude's built-in safety check couldn't look at this just now, so it didn't run. Nothing was changed.":
    "Claude की अपनी सुरक्षा जाँच अभी इसे देख नहीं सकी, इसलिए यह नहीं चला। कुछ नहीं बदला।",
  // a rule Clauseo set for the matter: its own app data, another matter, a matter's .clauseo folder
  "This tried to reach somewhere Clauseo keeps off limits here, such as Clauseo's own app data or another matter. Nothing was changed.":
    "इसने ऐसी जगह तक पहुँचने की कोशिश की जिसे Clauseo यहाँ बंद रखता है, जैसे Clauseo का अपना ऐप डेटा या कोई दूसरा मामला। कुछ नहीं बदला।",
} as const;

/** Every refusal line, English and Hindi, for the voice test and the copy review. */
export const refusalWords: Readonly<Record<string, string>> = HI;
