; Tauri's installer messages in Hindi (bundle.windows.nsis.customLanguageFiles in
; crates/desktop/tauri.conf.json): provisional Hindi from the app thread, pending copy review
; (clauseo-acp-private measurements/installer-hindi/pairs.md). Tauri 2.12.2 has no Hindi file of
; its own, so this one holds all of Tauri's messages, the same keys as English.nsh in the same
; order (tests hold them equal): Clauseo's wording first, then Tauri's stock messages. The
; installer shows it when Windows' display language is Hindi; the uninstaller when the lawyer
; had chosen Hindi in the app at the last install or update (windows/hooks.nsh).
; No byte order mark: the bundler adds one when it copies the file.

; Clauseo's wording: the one line whose words differ from Tauri's (approved by Rohan, 2026-11-03),
; then the four lines windows/hooks.nsh shows after the app deletes its data (S20, 2026-11-05;
; deleteAppDataFailedUnnamed 2026-11-06), then the two the per-machine installer shows about the
; firm-policy folder (2026-11-06), and the two the uninstaller shows about S15's removal: when it
; outlasts its wait (removeStillRunning, 2026-11-07) and when a silent per-user uninstall skips it
; (removeSkippedSilent, 2026-11-07).
; deleteAppData labels the uninstaller's box (unticked) that removes the whole data folder, the
; store included; papers stay in their matter folders. The box is one line, 400 x 25 px: keep it
; short (a test holds it to 60 characters).
LangString deleteAppData ${LANG_HINDI} "बातचीत, सहेजे गए संस्करण और सेटिंग्स भी हटाएँ"
; deleteAppDataFailed: something in the data folder could not be deleted (or the app was missing)
; and no kept folder is left in it; $R9 is the local data folder. deleteAppDataKept: a folder that
; stays, the sandbox's (S15's records) or the files tier B set aside; $R8 is that folder. Each path
; stands alone after a blank line. Both in a message box, skipped by /S. Provisional (app thread,
; 2026-11-05).
LangString deleteAppDataFailed ${LANG_HINDI} "Clauseo अपना पूरा डेटा नहीं हटा सका। आप यह फ़ोल्डर खुद हटा सकते हैं।$\n$\n$R9"
LangString deleteAppDataKept ${LANG_HINDI} "Clauseo ने यह फ़ोल्डर रहने दिया, क्योंकि इसमें ऐसी फ़ाइलें हो सकती हैं जिनकी आपको अब भी ज़रूरत है। इसे हटाने से पहले एक बार देख लें।$\n$\n$R8"
; deleteAppDataKeptFailed: a folder stays ($R8, as deleteAppDataKept) and something else in the
; data folder ($R9) could not be deleted: one box for both. Provisional (app thread, 2026-11-05).
LangString deleteAppDataKeptFailed ${LANG_HINDI} "Clauseo अपना पूरा डेटा नहीं हटा सका। उसने एक फ़ोल्डर रहने दिया, क्योंकि इसमें ऐसी फ़ाइलें हो सकती हैं जिनकी आपको अब भी ज़रूरत है। कुछ भी हटाने से पहले उस फ़ोल्डर को एक बार देख लें।$\n$\nजो फ़ोल्डर रहने दिया:$\n$R8$\n$\nबाकी डेटा:$\n$R9"
; deleteAppDataFailedUnnamed: as deleteAppDataFailed, but the data folder is itself a link (or
; could not be read), so no path is shown: one would lead through the link. The folder is named
; by its name instead. Provisional (app thread, 2026-11-06).
LangString deleteAppDataFailedUnnamed ${LANG_HINDI} "Clauseo अपना पूरा डेटा नहीं हटा सका। यह आपके लोकल AppData फ़ोल्डर में ai.clauseo.desktop फ़ोल्डर में है। अपना कंप्यूटर फिर से शुरू करें, फिर वह फ़ोल्डर हटा दें।"
; policyFolderExists and policyFolderFailed: the per-machine installer's lines in its details list
; (DetailPrint, windows/hooks.nsh NSIS_HOOK_POSTINSTALL) about the firm-policy folder;
; $APPDATA is C:\ProgramData there. Exists: something was already at that path, left as it
; is. Failed: the folder could not be created. Provisional (stage F, 2026-11-06).
LangString policyFolderExists ${LANG_HINDI} "Clauseo ने पहले से मौजूद फ़ोल्डर $APPDATA\Clauseo को जैसा था वैसा ही छोड़ दिया। वहाँ रखी फ़र्म की नीति तभी पढ़ी जाती है जब इस फ़ोल्डर के मालिक व्यवस्थापक हों।"
LangString policyFolderFailed ${LANG_HINDI} "Clauseo फ़र्म की नीति के लिए $APPDATA\Clauseo नहीं बना सका। इस कंप्यूटर पर यह फ़ोल्डर आपकी फ़र्म प्रबंधित करती है।"
; removeStillRunning: S15's removal of Clauseo's protection (clauseo-sandbox-setup.exe --remove)
; was still running when the uninstaller stopped waiting for it, after 2 minutes; it carries on
; and finishes by itself. In the uninstaller's details list (DetailPrint), never a box.
; Provisional (stage F, 2026-11-07).
LangString removeStillRunning ${LANG_HINDI} "Clauseo अभी सुरक्षित कार्यक्षेत्र हटा रहा है। यह अपने आप पूरा हो जाएगा।"
; removeSkippedSilent: a silent per-user uninstall (/S) can't ask for the UAC prompt that removing
; Clauseo's protection needs, so it skips the removal and keeps the sandbox folder. In the
; uninstaller's details list (DetailPrint), never a box. Provisional (stage F, 2026-11-07).
LangString removeSkippedSilent ${LANG_HINDI} "Clauseo ने सुरक्षित कार्यक्षेत्र नहीं हटाया, क्योंकि यह अनइंस्टॉल साइलेंट है। इसे हटाने के लिए Clauseo फिर से इंस्टॉल करें और /S के बिना अनइंस्टॉल करें।"
; removeExeMissing: the uninstaller found S15's set-up program (clauseo-sandbox-setup.exe) gone,
; most likely quarantined by Windows security, while the machine's record shows a set-up, so it
; couldn't run --remove. A box; under /S a details line and exit code 21. setupExeRemoved: an
; installer found that program gone right after writing it; the install stays. A box; under /S a
; details line and exit code 20. Provisional (app thread, stage C's fact check, 2026-11-07).
LangString removeExeMissing ${LANG_HINDI} "Clauseo सुरक्षित कार्यक्षेत्र नहीं हटा सका, क्योंकि Windows सुरक्षा ने उसे सेट अप करने वाला प्रोग्राम हटा दिया। सुरक्षित कार्यक्षेत्र के छिपे हुए Windows खाते अभी भी इस कंप्यूटर पर हैं। उस प्रोग्राम को वापस लाने के लिए Clauseo का इंस्टॉलर फिर से चलाएँ, फिर Clauseo को दोबारा अनइंस्टॉल करें। अगर इससे काम न बने, तो Clauseo के IT नोट्स में उन्हें हटाने का तरीका बताया गया है।"
; Provisional (app thread, 2026-11-07): stage C's admin-copy lines.
LangString removeAdminCopyRefused ${LANG_HINDI} "Clauseo सुरक्षित कार्यक्षेत्र नहीं हटा सका, क्योंकि वह यह पुष्टि नहीं कर सका कि Program Files में रखा, इसे तैयार करने वाला प्रोग्राम वही है जो Clauseo ने इंस्टॉल किया था, और उसे केवल व्यवस्थापक बदल सकते हैं। सुरक्षित कार्यक्षेत्र के छिपे हुए Windows खाते अभी भी इस कंप्यूटर पर हैं। उस प्रोग्राम को वापस लाने के लिए Clauseo का इंस्टॉलर फिर से चलाएँ, फिर Clauseo को दोबारा अनइंस्टॉल करें। अगर इससे काम न बने, तो Clauseo के IT नोट्स में उन्हें हटाने का तरीका बताया गया है।"
LangString setupExeRemoved ${LANG_HINDI} "इंस्टॉल होते समय Windows सुरक्षा ने Clauseo का एक हिस्सा हटा दिया: वह प्रोग्राम जो सुरक्षित कार्यक्षेत्र सेट अप करता है। जब तक इसे वापस नहीं लाया जाता, Clauseo सुरक्षित कार्यक्षेत्र में Claude या ChatGPT शुरू नहीं करेगा। इसे वापस लाने के लिए यह इंस्टॉलर फिर से चलाएँ। अगर Windows सुरक्षा इसे फिर से हटा दे, तो Clauseo के IT नोट्स में बताया गया है कि किसे अनुमति देनी है।"
; Provisional (app thread, 2026-11-07): stage C's admin-copy lines.
LangString adminCopyAsk ${LANG_HINDI} "अब Windows, सुरक्षित कार्यक्षेत्र तैयार करने के लिए अनुमति माँगेगा। वह प्रकाशक को अज्ञात बताएगा। यह सामान्य है: अनुमति यही इंस्टॉलर माँग रहा है। Yes चुनें।"
LangString adminCopyWorking ${LANG_HINDI} "सुरक्षित कार्यक्षेत्र तैयार हो रहा है। इसमें कुछ मिनट लग सकते हैं।"
LangString adminCopyDone ${LANG_HINDI} "सुरक्षित कार्यक्षेत्र तैयार है।"
LangString adminCopyDeclined ${LANG_HINDI} "Clauseo इंस्टॉल हो गया है, पर सुरक्षित कार्यक्षेत्र तैयार नहीं है, क्योंकि Windows के पूछने पर अनुमति नहीं दी गई। Clauseo इसके बिना भी काम करता है। इसे तैयार करने के लिए यह इंस्टॉलर फिर से चलाएँ और Yes चुनें। अगर Windows किसी व्यवस्थापक का पासवर्ड माँगे, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति से उसे दर्ज करने को कहें।"
LangString adminCopyBlocked ${LANG_HINDI} "Clauseo इंस्टॉल हो गया है, पर सुरक्षित कार्यक्षेत्र तैयार नहीं है, क्योंकि आपके Windows खाते को यह बदलाव स्वीकृत करने की अनुमति नहीं है। इसे तैयार करने के लिए इस कंप्यूटर की देखरेख करने वाले व्यक्ति से कहें कि वे यह इंस्टॉलर किसी व्यवस्थापक खाते से चलाएँ, या Clauseo सभी उपयोगकर्ताओं के लिए इंस्टॉल करें।"
LangString adminCopyFailed ${LANG_HINDI} "Clauseo इंस्टॉल हो गया है, पर सुरक्षित कार्यक्षेत्र तैयार नहीं हो सका। Clauseo इसके बिना भी काम करता है। फिर से प्रयास करने के लिए यह इंस्टॉलर दोबारा चलाएँ। अगर फिर भी यह तैयार न हो, तो इस कंप्यूटर की देखरेख करने वाले व्यक्ति से पूछें।"
LangString adminCopyOtherAccount ${LANG_HINDI} "Clauseo इंस्टॉल हो गया है, पर सुरक्षित कार्यक्षेत्र तैयार नहीं है, क्योंकि इस कंप्यूटर पर किसी दूसरे Windows खाते के पास यह पहले से है। Clauseo इसके बिना भी काम करता है।"
LangString adminCopyMachineFailed ${LANG_HINDI} "Clauseo इंस्टॉल हो गया है, पर इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र तैयार नहीं हो सका। Clauseo इसके बिना भी काम करता है। इस कंप्यूटर की देखरेख करने वाले व्यक्ति इसे तैयार करने के लिए यह इंस्टॉलर फिर से चला सकते हैं।"
LangString adminCopyNewerKept ${LANG_HINDI} "इस कंप्यूटर पर सुरक्षित कार्यक्षेत्र तैयार करने वाले प्रोग्राम का जो नया संस्करण पहले से है, Clauseo ने उसे ही रहने दिया।"
LangString adminCopyAppRunning ${LANG_HINDI} "Clauseo इंस्टॉल हो गया है, पर सुरक्षित कार्यक्षेत्र तैयार नहीं है, क्योंकि Clauseo चल रहा है। Clauseo बंद करें, फिर यह इंस्टॉलर दोबारा चलाएँ।"
LangString adminCopyStillRunning ${LANG_HINDI} "हो सकता है Clauseo अभी भी सुरक्षित कार्यक्षेत्र तैयार कर रहा हो। यह तैयार हुआ या नहीं, यह देखने के लिए Clauseo की सेटिंग्स खोलें और सुरक्षित कार्यक्षेत्र देखें।"
LangString adminCopySkippedSilent ${LANG_HINDI} "सुरक्षित कार्यक्षेत्र तैयार नहीं है, क्योंकि यह इंस्टॉल साइलेंट है। इसे तैयार करने के लिए यह इंस्टॉलर /S के बिना फिर से चलाएँ।"
LangString installTempFailed ${LANG_HINDI} "Clauseo इंस्टॉल नहीं हुआ, क्योंकि इंस्टॉलर Program Files में अपनी अस्थायी फ़ाइलों के लिए ऐसा फ़ोल्डर नहीं बना सका जिसे केवल व्यवस्थापक बदल सकें। इंस्टॉलर फिर से चलाएँ।"
LangString uninstallTempFailed ${LANG_HINDI} "Clauseo अनइंस्टॉल नहीं हुआ, क्योंकि अनइंस्टॉलर Program Files में अपनी अस्थायी फ़ाइलों के लिए ऐसा फ़ोल्डर नहीं बना सका जिसे केवल व्यवस्थापक बदल सकें। कुछ भी नहीं हटाया गया। अनइंस्टॉलर फिर से चलाएँ।"
LangString uninstallNotInPlace ${LANG_HINDI} "Clauseo अनइंस्टॉल नहीं हुआ, क्योंकि अनइंस्टॉलर Clauseo के अपने फ़ोल्डर से शुरू नहीं किया गया। कुछ भी नहीं हटाया गया। सेटिंग्स > ऐप्स से Clauseo अनइंस्टॉल करें।"
; nativeArm64Notice: the x64 per-machine installer's details line on an ARM64 computer
; (windows/hooks.nsh, CLAUSEO_NATIVE_ARM64_NOTICE); details list only. Provisional (2026-11-09).
LangString nativeArm64Notice ${LANG_HINDI} "इस कंप्यूटर में ARM64 प्रोसेसर है: Clauseo_${VERSION}_arm64-machine-setup.exe इसका मूल प्रति-मशीन इंस्टॉलर है।"

; webview2TooOld: a per-user install found WebView2 older than Clauseo needs and, so as never to
; ask for an administrator, leaves it to Windows to update (installer.nsi block
; webview2-no-elevation); the per-machine installer says it when its update fails. Details list
; only; the install goes on. Provisional (2026-11-09).
LangString webview2TooOld ${LANG_HINDI} "इस कंप्यूटर पर WebView2 का संस्करण Clauseo की ज़रूरत से पुराना है। Windows इसे बैकग्राउंड में अपने-आप अपडेट करता रहता है। अगर Clauseo खोलते समय भी यह पुराना हो, तो Clauseo आपको बताएगा कि नया संस्करण कैसे पाएँ।"

; Tauri's stock English, as @tauri-apps/cli 2.12.2 ships it (crates/tauri-bundler/src/bundle/
; windows/nsis/languages/English.nsh at tauri-cli-v2.12.2), deleteAppData aside, with one fix:
; Tauri writes {{product_name}} in appRunning, appRunningOkKill and failedToKillApp, which NSIS
; shows as it is (language files are included, not rendered); here they use ${PRODUCTNAME}. On a
; Tauri upgrade, copy that file's lines here again and keep the fix; a key missing here has no
; English text.
LangString addOrReinstall ${LANG_HINDI} "घटक जोड़ें/फिर से इंस्टॉल करें"
LangString alreadyInstalled ${LANG_HINDI} "पहले से इंस्टॉल है"
LangString alreadyInstalledLong ${LANG_HINDI} "${PRODUCTNAME} ${VERSION} पहले से इंस्टॉल है। आप क्या करना चाहते हैं, चुनें और जारी रखने के लिए आगे पर क्लिक करें।"
LangString appRunning ${LANG_HINDI} "${PRODUCTNAME} चल रहा है। कृपया पहले इसे बंद करें, फिर से प्रयास करें।"
LangString appRunningOkKill ${LANG_HINDI} "${PRODUCTNAME} चल रहा है।$\nइसे बंद करने के लिए OK पर क्लिक करें"
LangString chooseMaintenanceOption ${LANG_HINDI} "आप क्या करना चाहते हैं, चुनें।"
LangString choowHowToInstall ${LANG_HINDI} "चुनें कि आप ${PRODUCTNAME} कैसे इंस्टॉल करना चाहते हैं।"
LangString createDesktop ${LANG_HINDI} "डेस्कटॉप शॉर्टकट बनाएँ"
LangString dontUninstall ${LANG_HINDI} "अनइंस्टॉल न करें"
LangString dontUninstallDowngrade ${LANG_HINDI} "अनइंस्टॉल न करें (इस इंस्टॉलर में अनइंस्टॉल किए बिना पुराना संस्करण इंस्टॉल नहीं किया जा सकता)"
LangString failedToKillApp ${LANG_HINDI} "${PRODUCTNAME} बंद नहीं हो सका। कृपया पहले इसे बंद करें, फिर से प्रयास करें"
LangString installingWebview2 ${LANG_HINDI} "WebView2 इंस्टॉल हो रहा है..."
LangString newerVersionInstalled ${LANG_HINDI} "${PRODUCTNAME} का नया संस्करण पहले से इंस्टॉल है। पुराना संस्करण इंस्टॉल न करें। अगर आपको यही पुराना संस्करण चाहिए, तो पहले मौजूदा संस्करण अनइंस्टॉल कर लें। आप क्या करना चाहते हैं, चुनें और जारी रखने के लिए आगे पर क्लिक करें।"
LangString older ${LANG_HINDI} "पुराना"
LangString olderOrUnknownVersionInstalled ${LANG_HINDI} "आपके कंप्यूटर पर ${PRODUCTNAME} का $R4 संस्करण इंस्टॉल है। इंस्टॉल करने से पहले मौजूदा संस्करण अनइंस्टॉल कर लें। आप क्या करना चाहते हैं, चुनें और जारी रखने के लिए आगे पर क्लिक करें।"
LangString silentDowngrades ${LANG_HINDI} "इस इंस्टॉलर में पुराना संस्करण इंस्टॉल नहीं किया जा सकता, इसलिए साइलेंट इंस्टॉल आगे नहीं बढ़ सकता। कृपया सामान्य इंस्टॉलर इस्तेमाल करें।$\n"
LangString unableToUninstall ${LANG_HINDI} "अनइंस्टॉल नहीं हो सका।"
LangString uninstallApp ${LANG_HINDI} "${PRODUCTNAME} अनइंस्टॉल करें"
LangString uninstallBeforeInstalling ${LANG_HINDI} "इंस्टॉल करने से पहले अनइंस्टॉल करें"
LangString unknown ${LANG_HINDI} "अज्ञात"
LangString webview2AbortError ${LANG_HINDI} "WebView2 इंस्टॉल नहीं हो सका। इसके बिना ऐप नहीं चल सकता। इंस्टॉलर फिर से शुरू करके देखें।"
LangString webview2DownloadError ${LANG_HINDI} "त्रुटि: WebView2 डाउनलोड नहीं हो सका - $0"
LangString webview2DownloadSuccess ${LANG_HINDI} "WebView2 बूटस्ट्रैपर डाउनलोड हो गया"
LangString webview2Downloading ${LANG_HINDI} "WebView2 बूटस्ट्रैपर डाउनलोड हो रहा है..."
LangString webview2InstallError ${LANG_HINDI} "त्रुटि: WebView2 इंस्टॉल नहीं हो सका, एग्ज़िट कोड $1"
LangString webview2InstallSuccess ${LANG_HINDI} "WebView2 इंस्टॉल हो गया"
