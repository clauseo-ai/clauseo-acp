<!-- Translated from the English page as it reads after the copy review's edits, not from 489eaff52's text. If the English changes, update this page with it. -->

# S mode से बाहर निकलना

[English version](windows-s-mode.md)

कुछ Windows लैपटॉप S mode में आते हैं। S mode में Clauseo न इंस्टॉल हो सकता है, न चल सकता है,
इसलिए पहले S mode बंद करना होगा। यह मुफ़्त है और इसमें कुछ ही मिनट लगते हैं, पर एक बार बंद करने
के बाद S mode फिर से चालू नहीं किया जा सकता। अगर यह कंप्यूटर आपको आपकी फ़र्म ने दिया है, तो
शुरू करने से पहले अपनी IT टीम से पूछें। फ़र्म इसे S mode में रख सकती है, और तब केवल IT टीम ही
इसे बदल सकती है।

## S mode क्या है

S mode, Windows की एक सीमित सेटिंग है। इसमें केवल Microsoft Store के ऐप चलते हैं। कुछ नए
लैपटॉप में यह पहले से चालू आती है, अक्सर Windows 11 Home वाले सस्ते लैपटॉप में।

## कैसे पता करें कि आपका कंप्यूटर S mode में है

जब आप Clauseo का इंस्टॉलर खोलते हैं, तो Windows उसे शुरू नहीं करता। इसके बजाय वह एक संदेश
दिखाता है, जो इस तरह शुरू होता है:

> For security and performance, this mode of Windows only runs Microsoft-verified apps

(यानी: सुरक्षा और परफ़ॉर्मेंस के लिए Windows का यह मोड केवल Microsoft से जाँचे गए ऐप चलाता है।)

Check on the laptop pass: the exact wording and buttons of this message on Windows 11 and
Windows 10.

> Screenshot to come: the "only runs Microsoft-verified apps" message.
<!-- img/windows-s-mode-blocked.png -->

यह नीला "Windows protected your PC" बॉक्स नहीं है, जिसमें **Run anyway** बटन होता है। यह Smart
App Control भी नहीं है, जिसके संदेश में Smart App Control का नाम लिखा होता है। इन दोनों के बारे
में [Windows पर Clauseo इंस्टॉल करना](windows.md) में बताया गया है।

पक्का करने के लिए Start मेन्यू खोलें, फिर **Settings**, **System**, **About**। **Windows
specifications** के नीचे **Edition** वाली पंक्ति में "in S mode" लिखा होता है, जैसे "Windows 11
Home in S mode"।

Check on the laptop pass: where the edition shows "in S mode".

## Clauseo के लिए S mode बंद क्यों होना चाहिए

Clauseo, Microsoft Store से नहीं आता। काम के दौरान वह कुछ और प्रोग्राम भी चलाता है जो Store से
नहीं आते, जैसे वे AI टूल जिनके साथ वह काम करता है। S mode इन सभी को रोक देता है। Store में
Clauseo का कोई संस्करण होता, तब भी कोई फ़र्क नहीं पड़ता, क्योंकि वे प्रोग्राम फिर भी रुके रहते।

## शुरू करने से पहले

- **यह मुफ़्त है।** S mode से बाहर निकलने के लिए Microsoft कोई शुल्क नहीं लेता।
- **इसमें कुछ ही मिनट लगते हैं।** आपकी फ़ाइलें और ऐप नहीं हटते।
- **इसे वापस नहीं बदला जा सकता।** एक बार बाहर निकलने के बाद Windows हमेशा के लिए S mode से
  बाहर रहता है। उसके बाद आपका कंप्यूटर ज़्यादातर कंप्यूटरों की तरह सामान्य Windows चलाता है, और
  Store के बाहर के ऐप भी इंस्टॉल कर सकता है।
- **इंटरनेट चाहिए, और शायद एक Microsoft खाता भी।** यह बदलाव Microsoft Store करता है। Store
  आपसे Microsoft खाते से साइन इन करने को कह सकता है, इसलिए उस खाते का ईमेल पता और पासवर्ड
  तैयार रखें।

Check on the laptop pass: whether the Store asks you to sign in.

## Windows 11 पर क्या करें

1. Start मेन्यू खोलें, फिर **Settings**।
2. **System** पर क्लिक करें, फिर **Activation** पर।
3. **Switch to Windows 11 Pro** या **Switch to Windows 11 Home** नाम का हिस्सा ढूँढें, और
   **Go to the Store** पर क्लिक करें। कुछ कंप्यूटरों पर इसकी जगह **S mode** नाम की पंक्ति होती
   है, जिसमें **Open Store** बटन होता है। उस पर क्लिक करें।

   > **ध्यान दें:** **Upgrade your edition of Windows** के नीचे वाले लिंक पर क्लिक न करें। वह
   > पैसे देकर होने वाला अपग्रेड है, और उससे S mode चालू ही रहता है।

4. Microsoft Store **Switch out of S mode** (S mode से बाहर निकलें) नाम का पेज खोलता है, या
   इससे मिलते-जुलते नाम का। **Get** पर क्लिक करें।
5. Store पुष्टि माँगे, तो पुष्टि करें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस
   कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं। जब पेज पर लिखा आए कि काम पूरा हो
   गया है, तो आपने जो Clauseo इंस्टॉलर डाउनलोड किया था, उसे फिर से चलाएँ, और
   [Windows पर Clauseo इंस्टॉल करना](windows.md) में दिए तरीके से आगे बढ़ें।

Check on the laptop pass: the Activation page's wording, the Store page and its confirmation, and
whether Windows asks for an administrator's password or a restart.

> Screenshot to come: Settings, Activation, with "Go to the Store" or "Open Store".
<!-- img/windows-s-mode-activation.png -->

> Screenshot to come: the Store's "Switch out of S mode" page with "Get".
<!-- img/windows-s-mode-store.png -->

## Windows 10 पर क्या करें

1. Start मेन्यू खोलें, फिर **Settings**।
2. **Update & Security** पर क्लिक करें, फिर **Activation** पर।
3. **Switch to Windows 10 Home** या **Switch to Windows 10 Pro** नाम का हिस्सा ढूँढें, और
   **Go to the Store** पर क्लिक करें।

   > **ध्यान दें:** **Upgrade your edition of Windows** के नीचे वाले लिंक पर क्लिक न करें। वह
   > पैसे देकर होने वाला अपग्रेड है, और उससे S mode चालू ही रहता है।

4. Microsoft Store **Switch out of S mode** (S mode से बाहर निकलें) नाम का पेज खोलता है, या
   इससे मिलते-जुलते नाम का। **Get** पर क्लिक करें।
5. Store पुष्टि माँगे, तो पुष्टि करें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस
   कंप्यूटर की देखरेख करने वाले व्यक्ति उसे दर्ज कर सकते हैं। जब पेज पर लिखा आए कि काम पूरा हो
   गया है, तो आपने जो Clauseo इंस्टॉलर डाउनलोड किया था, उसे फिर से चलाएँ, और
   [Windows पर Clauseo इंस्टॉल करना](windows.md) में दिए तरीके से आगे बढ़ें।

Check on the laptop pass: the Windows 10 Activation page's wording, and whether Windows asks for an
administrator's password or a restart.

> Screenshot to come: Windows 10 Settings, Activation, with "Go to the Store".
<!-- img/windows-10-s-mode-activation.png -->

## अगर Get बटन न दिखे, या Windows बदलने न दे

**आपके अपने कंप्यूटर पर** आप S mode से बाहर निकल सकते हैं। अगर Store न खुले या उसमें **Get**
बटन न दिखे, तो देख लें कि इंटरनेट जुड़ा है, Store में अपने Microsoft खाते से साइन इन करें, और
फिर से कोशिश करें। अगर फिर भी न हो, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति से पूछें।

**फ़र्म के दिए कंप्यूटर पर** हो सकता है कि फ़र्म ने उसे S mode में लॉक कर रखा हो। तब Windows
आपको इसे बदलने नहीं देगा। आपकी फ़र्म की IT टीम, फ़र्म के कंप्यूटर संभालने वाले अपने टूल से, इसे
आपके लिए बदल सकती है। उन्हें यह पेज और [Clauseo for firm IT](../firm-it.md) भेजें, जिसमें
उनके लिए पूरा तरीका दिया गया है।

## किसे लिखें

<https://github.com/clauseo-ai/clauseo-acp/issues> पर एक issue खोलें। Issues सार्वजनिक होते हैं,
इसलिए उनमें किसी मामले या क्लाइंट के बारे में कुछ भी न लिखें।
