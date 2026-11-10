// The Hindi of words-update.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-update.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the notice's name for assistive technology, and Settings' section
  Updates: "अपडेट",
  // R23-01 to R23-11: the room's notice
  "Clauseo {version} is ready to install.": "Clauseo {version} इंस्टॉल के लिए तैयार है।",
  "It installs the next time Clauseo opens.": "यह अगली बार Clauseo खुलने पर इंस्टॉल होगा।",
  "Restart to update": "फिर से शुरू करके अपडेट करें",
  "After this answer": "इस उत्तर के बाद",
  "Restarting now stops the answer in progress.": "अभी फिर से शुरू करने पर चल रहा उत्तर रुक जाएगा।",
  "Clauseo will restart to update after this answer.":
    "इस उत्तर के बाद Clauseo अपडेट के लिए फिर से शुरू होगा।",
  Cancel: "रद्द करें",
  "Installing the update…": "अपडेट इंस्टॉल हो रहा है…",
  "Clauseo was updated from {from} to {current}.": "Clauseo को {from} से {current} में अपडेट किया गया।",
  "Got it": "ठीक है",
  // R23-12 onwards: Settings, Updates
  "Clauseo {version}": "Clauseo {version}",
  "Last checked {time}": "पिछली जाँच {time}",
  "When an update is ready": "अपडेट तैयार होने पर",
  "Install updates when Clauseo next opens": "Clauseo के अगली बार खुलने पर अपडेट इंस्टॉल करें",
  "Tell me first": "पहले मुझे बताएँ",
  "Check now": "अभी जाँचें",
  "Checking for updates…": "अपडेट की जाँच हो रही है…",
  "Downloading the update…": "अपडेट डाउनलोड हो रहा है…",
  "An update is ready. Restart to update from the main window.":
    "एक अपडेट तैयार है। मुख्य विंडो से फिर से शुरू करके अपडेट करें।",
  "Clauseo is up to date.": "Clauseo पूरी तरह अपडेट है।",
  "Clauseo couldn't check for updates. Try again.":
    "Clauseo अपडेट की जाँच नहीं कर सका। फिर से प्रयास करें।",
  // who manages updates, where Clauseo doesn't update itself
  "Your firm manages updates for Clauseo.": "आपकी फ़र्म Clauseo के अपडेट संभालती है।",
  "Clauseo is installed for everyone on this computer, so whoever looks after it installs updates.":
    "Clauseo इस कंप्यूटर पर सभी के लिए इंस्टॉल है, इसलिए इसकी देखरेख करने वाले व्यक्ति अपडेट इंस्टॉल करते हैं।",
  "Clauseo can't update itself from where it's installed. Ask whoever looks after this computer.":
    "Clauseo जहाँ इंस्टॉल है, वहाँ से वह ख़ुद को अपडेट नहीं कर सकता। इस कंप्यूटर की देखरेख करने वाले व्यक्ति से पूछें।",
  "Updates come through the package manager that installed Clauseo.":
    "अपडेट उसी पैकेज मैनेजर से आते हैं जिससे Clauseo इंस्टॉल हुआ था।",
  "This version of Clauseo doesn't update itself. To update, install the new version over this one. Your matters and settings stay.":
    "Clauseo का यह संस्करण अपने आप अपडेट नहीं होता। अपडेट करने के लिए, नया संस्करण इसी के ऊपर इंस्टॉल करें। आपके मामले और सेटिंग्स बने रहते हैं।",
} satisfies Hindi<string>;
