<!-- Translated from the English page as it reads after the copy review's edits, not from stage F's pasted text. If the English changes, update this page with it. Clauseo's own button is given as the Hindi app shows it (फिर से प्रयास करें), with the English in brackets. Windows' labels stay in English. The "For your firm's IT team" section stays in English on purpose: it is for IT staff, and its terms and paths are the ones their tools use. -->

# जब सुरक्षा सॉफ़्टवेयर किसी AI टूल का सेटअप रोक दे

[English version](security-software.md)

जब आप पहली बार Claude या ChatGPT जैसा कोई AI टूल चुनते हैं, तो Clauseo उसकी फ़ाइलें डाउनलोड करके
आपके कंप्यूटर पर सेट अप करता है। कभी-कभी डाउनलोड तो हो जाता है, पर सेटअप पूरा नहीं होता। तब
Clauseo एक संदेश दिखाता है, जो कुछ इस तरह शुरू होता है:

> Claude डाउनलोड हो गया, लेकिन उसका सेटअप पूरा नहीं हुआ।

ChatGPT के लिए संदेश में Codex का नाम होता है। Codex, ChatGPT का वह टूल है जिसे Clauseo इस्तेमाल
करता है।

Check on the laptop pass: the message's final wording, in English and Hindi.

नीचे दिए कदम इसी क्रम में आज़माएँ।

## ऐसा क्यों होता है

सुरक्षा सॉफ़्टवेयर, जैसे Windows Security (जिसे Microsoft Defender भी कहते हैं) या आपकी फ़र्म का
लगाया कोई एंटीवायरस, नई फ़ाइलों के आते ही उनकी जाँच करता है। जब तक जाँच चलती है, Clauseo उन
फ़ाइलों को उनकी जगह पर नहीं रख सकता। कभी-कभी सुरक्षा सॉफ़्टवेयर किसी फ़ाइल को बहुत देर तक रोके
रखता है, या उसे न पहचानने के कारण हटा देता है।

यह संदेश दिखाने से पहले Clauseo अपने आप तीन बार सेटअप की कोशिश कर चुका होता है।

## 1. एक मिनट रुकें, फिर "फिर से प्रयास करें" चुनें

अक्सर तब तक सुरक्षा सॉफ़्टवेयर अपनी जाँच पूरी कर चुका होता है। लगभग एक मिनट रुकें, फिर संदेश के
नीचे **फिर से प्रयास करें** (Try again) चुनें। Clauseo पहले से डाउनलोड और जाँची गई फ़ाइलें रखे
रहता है, इसलिए इस बार आम तौर पर पहली बार से कम समय लगता है।

## 2. कंप्यूटर रीस्टार्ट करें, फिर दोबारा कोशिश करें

रीस्टार्ट करने से वह सब बंद हो जाता है जो अब भी फ़ाइलों को रोके हुए हो। Clauseo फिर से खोलें और
**फिर से प्रयास करें** चुनें।

Check on the laptop pass: after a restart, whether the message and its **Try again** are still
there, or whether the lawyer has to choose the AI tool again.

## 3. अपने सुरक्षा सॉफ़्टवेयर में रोकी गई फ़ाइलों की सूची देखें

अगर सेटअप अब भी पूरा न हो, तो हो सकता है कि आपके सुरक्षा सॉफ़्टवेयर ने AI टूल की कोई फ़ाइल हटा
दी हो। ज़्यादातर सुरक्षा सॉफ़्टवेयर रोकी गई या हटाई गई फ़ाइलों की सूची रखते हैं, और किसी फ़ाइल
को वापस रखने का विकल्प देते हैं।

Clauseo, AI टूल की फ़ाइलें `agents` नाम के फ़ोल्डर में रखता है, जो `ai.clauseo.desktop` नाम
के फ़ोल्डर के अंदर है। ऐसी रोकी गई फ़ाइल ढूँढें जिसकी लोकेशन में `ai.clauseo.desktop` हो। यह
फ़ाइल अक्सर `node.exe` होती है, और Mac पर `node`।

### Windows पर, Windows Security के साथ

बीच में Windows किसी व्यवस्थापक का पासवर्ड माँग सकता है। इस कंप्यूटर की देखरेख करने वाले व्यक्ति
उसे दर्ज कर सकते हैं।

1. Start मेन्यू खोलें, **Windows Security** टाइप करें और उसे खोलें।
2. **Virus & threat protection** पर क्लिक करें।
3. **Current threats** के नीचे **Protection history** पर क्लिक करें।
4. उस समय के आसपास का कोई आइटम चुनें जब Clauseo ने AI टूल सेट अप किया था। उसके ब्योरे में
   फ़ाइल की लोकेशन दिखती है।
5. अगर लोकेशन में `ai.clauseo.desktop` हो, तो **Actions** पर क्लिक करें, फिर **Restore** या
   **Allow on device**, जो भी दिखे।
6. Clauseo पर लौटें और **फिर से प्रयास करें** चुनें।

अगर किसी आइटम की लोकेशन में `ai.clauseo.desktop` नहीं है, तो Windows Security ने Clauseo को नहीं
रोका। अगर आप कोई दूसरा एंटीवायरस इस्तेमाल करते हैं, तो नीचे बताए तरीके से उसमें देखें। अगर नहीं,
तो हमें लिखें ("किसे लिखें" देखें)।

Check on the laptop pass: the exact names of these pages and buttons on Windows 11 and Windows 10,
and whether opening an item's details or Restore asks for an administrator's password.

> Screenshot to come: Windows Security's Protection history, with a Clauseo file.
<!-- img/windows-protection-history.png -->

अगर Windows पर आप कोई दूसरा एंटीवायरस इस्तेमाल करते हैं, तो उसे खोलें और **Quarantine**,
**History** या **Recent activity** जैसी किसी सूची को देखें। वहाँ ऐसी फ़ाइल ढूँढें जिसकी लोकेशन
में `ai.clauseo.desktop` हो, उसे वापस रखें (restore करें), फिर Clauseo में **फिर से प्रयास करें**
चुनें।

### Mac पर

Mac की अपनी सुरक्षा ऐसी कोई सूची नहीं रखती जिसे आप देख सकें। अगर आपने या आपकी फ़र्म ने कोई
एंटीवायरस लगाया है, तो उसे खोलें और **Quarantine**, **History** या **Recent activity** जैसी किसी
सूची को देखें। ऐसी फ़ाइल ढूँढें जिसकी लोकेशन में `ai.clauseo.desktop` हो, उसे वापस रखें, फिर
Clauseo में **फिर से प्रयास करें** चुनें।

Check on the laptop pass: whether the Mac's own protection ever removes an AI tool's files, and
what the lawyer sees if it does.

### फ़र्म के दिए कंप्यूटर पर

सुरक्षा सॉफ़्टवेयर आपकी फ़र्म संभालती है, और हो सकता है कि वह आपको खुद फ़ाइलें वापस रखने न दे।
अगर आपको सूची न दिखे, या वह फ़ाइल वापस रखने न दे, तो यह पेज अपनी IT टीम को भेजें। अगले हिस्से
में उन्हें बताया गया है कि AI टूल की फ़ाइलों को अनुमति कैसे दें।

## For your firm's IT team

यह हिस्सा आपकी फ़र्म की IT टीम के लिए है, इसलिए अंग्रेज़ी में है।

The agent install downloads every file, checks each against its digest, then unpacks them with
`npm ci` and renames the result into place. `agent.install_setup_failed` means the downloads and
checks passed and that last part failed after three tries. On our Windows test machine, Defender
held the fresh files and the rename got `Access is denied`. Clauseo retries sharing and lock
errors in place before it counts a try.

Each user's agent files are under their Clauseo data folder (or under `CLAUSEO_DATA_DIR` when that
is set):

| System | Folder |
|---|---|
| Windows | `%LOCALAPPDATA%\ai.clauseo.desktop\agents\` |
| macOS | `~/Library/Application Support/ai.clauseo.desktop/agents/` |
| Linux | `$XDG_DATA_HOME/ai.clauseo.desktop/agents/`, by default `~/.local/share/...` |

Inside it:

- `runtime\node-v<version>-<os>-<arch>\`: the Node runtime Clauseo downloads when the computer has
  no suitable Node of its own. The program is `node.exe` on Windows, `bin/node` on macOS and Linux.
- `<agent>\install\<hash>\`: each agent's packages (`claude-acp` for Claude, `codex-acp` for
  ChatGPT), with their own programs under `node_modules`, such as Codex's `codex.exe` and its
  `rg.exe` on Windows.
- `<agent>\install\.tmp-<hash>-<n>\`: where `npm ci` unpacks a try before the rename.
- `downloads\`: the checked archives, removed once the agent is ready.

Check on the laptop pass: the programs in Claude's and Codex's packages on each system, with
their paths under `node_modules`.

**What to allow, from narrowest to widest.**

1. **Restore and allow that one detection.** In Windows Security's Protection history, or in
   Microsoft Defender for Endpoint, choose **Allow on device** for the detection, or your
   product's equivalent for that file. Scanning stays on for everything else.
2. **Allow by signer or hash, not by path.** Use Defender for Endpoint file or certificate
   indicators, or your EDR's certificate allow. To see a program's signer, open the file's
   **Properties**, then **Digital Signatures**, and allow that signer. Allow unsigned programs by
   their hash (`rg.exe` may be one). A hash changes with each new version Clauseo downloads, so a
   hash allow needs updating after an update; prefer the signer where there is one.

3. **A path exclusion only as a last resort.** An exclusion is not an allow: it turns scanning off
   for everything in that folder. These folders are user-writable, so files any program writes
   there are no longer scanned. If you need one, scope it to `agents\runtime\` and
   `agents\<agent>\install\`, never the whole data folder. Avoid process exclusions for
   `node.exe`: they stop scanning of every file it opens, including the ones the AI tool writes
   across the matter. At an administrator's prompt,
   `%LOCALAPPDATA%` is the administrator's folder, so name the user's own:
   `C:\Users\<user>\AppData\Local\ai.clauseo.desktop\agents\runtime\`.

[Clauseo for firm IT](../firm-it.md) has the rest: AppLocker and WDAC rules for this folder in
[section 3](../firm-it.md#3-applocker-and-wdac), and what to do when Windows security removes
Clauseo's own set-up program in
[section 10](../firm-it.md#10-if-windows-security-removed-clauseos-set-up-program).

## किसे लिखें

<https://github.com/clauseo-ai/clauseo-acp/issues> पर एक issue खोलें। Issues सार्वजनिक होते हैं,
इसलिए उनमें किसी मामले या क्लाइंट के बारे में कुछ भी न लिखें।
