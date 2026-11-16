// noticeLine: the app's line for each notice kind it knows, and null for everything else, so the row shows the notice's
// text. Read by shape: an absent kind, an unknown string, and an object this version doesn't know give null.
import { t, type Key } from "@clauseo/components/lib/words.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { expect, test } from "vitest";
import type { NoticeKind, PackSkipReason } from "../../transport/protocol.ts";
import { noticeLine, type NoticeWords } from "./notice-words.ts";

const words = (lang: "en" | "hi"): NoticeWords => ({
  lang,
  th: (key: ThreadKey, slots) => th(lang, key, slots),
  t: (key: Key, slots) => t(lang, key, slots),
});
const en = words("en");

/** Papers as Versions names them: isolated, so a right-to-left mark can't reorder the line. */
const iso = (name: string) => `\u2068${name}\u2069`;

const CONTEXT =
  "Clauseo doesn't remember the earlier questions and answers in this conversation. Include what it needs from them in your next question.";

test("a known kind gives the app's line, in the page's words", () => {
  expect(noticeLine("context_not_restored", en)).toBe(CONTEXT);
});

test("Claude's auto mode unavailable gives the app's line, in English and Hindi", () => {
  expect(noticeLine("auto_mode_unavailable", en)).toBe(
    "Claude's automatic mode isn't available for this account, so Claude asks before running commands.",
  );
  expect(noticeLine("auto_mode_unavailable", words("hi"))).toBe(
    "इस खाते के लिए Claude का स्वचालित मोड उपलब्ध नहीं है, इसलिए Claude कमांड चलाने से पहले पूछता है।",
  );
});

test("Claude's commands kept out of some folders give the app's line (UC-382)", () => {
  expect(noticeLine("shell_limited", en)).toBe(
    "Claude's commands can't reach some folders right now; it can still read and edit this matter's files.",
  );
  expect(noticeLine("shell_limited", words("hi"))).toBe(
    "Claude की कमांड अभी कुछ फ़ोल्डर तक नहीं पहुँच सकतीं; वह इस मामले की फ़ाइलें अब भी पढ़ और संपादित कर सकता है।",
  );
});

test("a matter inside this one is named by its name now, else the host's, and says what to do (UC-387)", () => {
  const SUB = "0190a000-0000-7000-8000-000000000011";
  const kind = { matter_inside: { matter: SUB, name: "Sub (old)" } } as NoticeKind;
  expect(noticeLine(kind, en, undefined, (m) => (m === SUB ? "Sub" : undefined))).toBe(
    `${iso("Sub")}'s folder is inside this matter's folder. Move it out, or remove ${iso("Sub")} from the list. If you remove it, its papers count as part of this matter.`,
  );
  expect(noticeLine(kind, words("hi"))).toBe(
    `${iso("Sub (old)")} का फ़ोल्डर इस मामले के फ़ोल्डर के अंदर है। उसे बाहर ले जाएँ, या ${iso("Sub (old)")} को सूची से हटा दें। अगर आप उसे हटाते हैं, तो उसके दस्तावेज़ इस मामले का हिस्सा माने जाएँगे।`,
  );
  const nameless = { matter_inside: { matter: SUB } } as unknown as NoticeKind;
  expect(noticeLine(nameless, en)).toBeNull();
});

test("the data-fence refusal's line names the agent as the stop line does", () => {
  const claude = { kind: "claude", name: "claude-acp" } as const;
  expect(noticeLine("outside_matter_blocked", en, claude)).toBe(
    "Clauseo stopped a step: Claude tried to reach outside this matter.",
  );
  expect(noticeLine("outside_matter_blocked", en, { kind: "codex", name: "codex-acp" })).toBe(
    "Clauseo stopped a step: ChatGPT tried to reach outside this matter.",
  );
  expect(noticeLine("outside_matter_blocked", en, { kind: undefined, name: "my-agent" })).toBe(
    "Clauseo stopped a step: my-agent tried to reach outside this matter.",
  );
  expect(noticeLine("outside_matter_blocked", words("hi"), claude)).toBe(
    "Clauseo ने एक काम रोक दिया: Claude ने इस मामले के बाहर पहुँचने की कोशिश की।",
  );
});

test("with no agent name known, the data-fence line names the AI", () => {
  expect(noticeLine("outside_matter_blocked", en)).toBe(
    "Clauseo stopped a step: the AI tried to reach outside this matter.",
  );
  expect(noticeLine("outside_matter_blocked", words("hi"), { kind: undefined, name: " " })).toBe(
    "Clauseo ने एक काम रोक दिया: AI ने इस मामले के बाहर पहुँचने की कोशिश की।",
  );
});

test("a declined shell download's line names the agent, in English and Hindi", () => {
  const codex = { kind: "codex", name: "codex-acp" } as const;
  expect(noticeLine("shell_network_blocked", en, codex)).toBe(
    "Clauseo stopped a step: ChatGPT tried to go online by running a command. ChatGPT can use web search instead.",
  );
  expect(noticeLine("shell_network_blocked", words("hi"), codex)).toBe(
    "Clauseo ने एक काम रोक दिया: ChatGPT ने कमांड चलाकर इंटरनेट से जुड़ने की कोशिश की। इसके बजाय ChatGPT वेब खोज का उपयोग कर सकता है।",
  );
  expect(noticeLine("shell_network_blocked", en)).toBe(
    "Clauseo stopped a step: the AI tried to go online by running a command. It can use web search instead.",
  );
  expect(noticeLine("shell_network_blocked", words("hi"), { kind: undefined, name: " " })).toBe(
    "Clauseo ने एक काम रोक दिया: AI ने कमांड चलाकर इंटरनेट से जुड़ने की कोशिश की। इसके बजाय यह वेब खोज का उपयोग कर सकता है।",
  );
});

test("without web search, the declined download's line offers none, in English and Hindi", () => {
  const codex = { kind: "codex", name: "codex-acp" } as const;
  const kind = "shell_network_blocked_no_web_search";
  expect(noticeLine(kind, en, codex)).toBe(
    "Clauseo stopped a step: ChatGPT tried to go online by running a command.",
  );
  expect(noticeLine(kind, words("hi"), codex)).toBe(
    "Clauseo ने एक काम रोक दिया: ChatGPT ने कमांड चलाकर इंटरनेट से जुड़ने की कोशिश की।",
  );
  expect(noticeLine(kind, en)).toBe(
    "Clauseo stopped a step: the AI tried to go online by running a command.",
  );
  expect(noticeLine(kind, words("hi"), { kind: undefined, name: " " })).toBe(
    "Clauseo ने एक काम रोक दिया: AI ने कमांड चलाकर इंटरनेट से जुड़ने की कोशिश की।",
  );
  // with web search, the line still offers it
  expect(noticeLine("shell_network_blocked", en, codex)).toMatch(/can use web search instead\.$/);
});

test("no kind (an agent's notice) gives null, so the row shows its own text", () => {
  expect(noticeLine(undefined, en)).toBeNull();
});

test("a kind this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine("something_newer" as NoticeKind, en)).toBeNull();
});

test("an object kind this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine({ other: {} } as unknown as NoticeKind, en)).toBeNull();
  expect(noticeLine(null as unknown as NoticeKind, en)).toBeNull();
});

test("the Hindi line is its Hindi", () => {
  expect(noticeLine("context_not_restored", words("hi"))).toBe(
    "Clauseo को इस बातचीत के पहले के प्रश्न और उत्तर याद नहीं हैं। उनसे जो ज़रूरी हो, उसे अपने अगले प्रश्न में शामिल करें।",
  );
});

test("one paper that came back: its name, and the singular line", () => {
  const kind: NoticeKind = { files_came_back: { files: ["Pleadings/Reply.docx"], more: 0 } };
  expect(noticeLine(kind, en)).toBe(
    `1 paper came back after the undo: ${iso("Reply.docx")}. Clauseo didn't remove it again.`,
  );
});

test("several papers that came back, none more: each name, joined as Versions joins them", () => {
  const kind: NoticeKind = {
    files_came_back: { files: ["Drafts/Lease.docx", "Notice.docx", "Reply.docx"], more: 0 },
  };
  expect(noticeLine(kind, en)).toBe(
    `3 papers came back after the undo: ${iso("Lease.docx")}, ${iso("Notice.docx")} and ${iso("Reply.docx")}. Clauseo didn't remove them again.`,
  );
});

test("papers with more beyond the five: the shared '{n} more' ends the list, and n counts them all", () => {
  const kind: NoticeKind = {
    files_came_back: {
      files: ["A.docx", "B.docx", "C.docx", "D.docx", "E.docx"],
      more: 3,
    },
  };
  expect(noticeLine(kind, en)).toBe(
    `8 papers came back after the undo: ${iso("A.docx")}, ${iso("B.docx")}, ${iso("C.docx")}, ${iso("D.docx")}, ${iso("E.docx")} and 3 more. Clauseo didn't remove them again.`,
  );
});

test("two papers with one bare name in different folders: each shows its whole path", () => {
  const kind: NoticeKind = {
    files_came_back: {
      files: ["Drafts/Reply.docx", "Pleadings/Reply.docx", "Notice.docx"],
      more: 0,
    },
  };
  expect(noticeLine(kind, en)).toBe(
    `3 papers came back after the undo: ${iso("Drafts/Reply.docx")}, ${iso("Pleadings/Reply.docx")} and ${iso("Notice.docx")}. Clauseo didn't remove them again.`,
  );
});

test("an object kind with no files and no more gives null, so the row shows its text", () => {
  expect(noticeLine({ files_came_back: { files: [], more: 0 } }, en)).toBeNull();
});

const skipped = (reason: string): NoticeKind =>
  ({ skill_pack_not_loaded: { pack: "nda-kit", reason } }) as NoticeKind;

test.each<[PackSkipReason, string, string]>([
  [
    "not_turned_on",
    "Clauseo didn't load the skill pack 'nda-kit'. This matter is set to use it, but it isn't turned on.",
    "Clauseo ने स्किल पैक 'nda-kit' लोड नहीं किया। यह मामला इसे इस्तेमाल करने के लिए सेट है, पर यह चालू नहीं है।",
  ],
  [
    "not_admitted",
    "Clauseo didn't load the skill pack 'nda-kit'. The skill pack settings on this computer don't allow it.",
    "Clauseo ने स्किल पैक 'nda-kit' लोड नहीं किया। इस कंप्यूटर की स्किल पैक सेटिंग्स इसकी अनुमति नहीं देतीं।",
  ],
  [
    "changed",
    "Clauseo didn't load the skill pack 'nda-kit'. Its files changed after it was turned on. Turn it on again to use it.",
    "Clauseo ने स्किल पैक 'nda-kit' लोड नहीं किया। चालू किए जाने के बाद इसकी फ़ाइलें बदल गईं। इसे इस्तेमाल करने के लिए फिर से चालू करें।",
  ],
  [
    "missing",
    "Clauseo didn't load the skill pack 'nda-kit'. Clauseo couldn't find it on this computer.",
    "Clauseo ने स्किल पैक 'nda-kit' लोड नहीं किया। Clauseo को यह इस कंप्यूटर पर नहीं मिला।",
  ],
  [
    "invalid",
    "Clauseo didn't load the skill pack 'nda-kit'. Its files aren't a valid skill pack.",
    "Clauseo ने स्किल पैक 'nda-kit' लोड नहीं किया। इसकी फ़ाइलें मान्य स्किल पैक नहीं हैं।",
  ],
  [
    "too_many",
    "Clauseo didn't load the skill pack 'nda-kit'. Clauseo loads at most 16 skill packs at a time.",
    "Clauseo ने स्किल पैक 'nda-kit' लोड नहीं किया। Clauseo एक बार में अधिकतम 16 स्किल पैक लोड करता है।",
  ],
])("a skill pack left out for %s gives its line, with the pack named", (reason, line, hi) => {
  expect(noticeLine(skipped(reason), en)).toBe(line);
  expect(noticeLine(skipped(reason), words("hi"))).toBe(hi);
});

test("a skill pack left out for a reason this version does not know gives null, so the row shows its text", () => {
  expect(noticeLine(skipped("unknown"), en)).toBeNull();
  expect(noticeLine(skipped("something_newer"), en)).toBeNull();
});

test("a skill pack notice with no reason gives null, so the row shows its text", () => {
  expect(
    noticeLine({ skill_pack_not_loaded: { pack: "nda-kit" } } as unknown as NoticeKind, en),
  ).toBeNull();
});

test("a skill pack notice with no pack gives null, so the row shows its text", () => {
  expect(
    noticeLine({ skill_pack_not_loaded: { reason: "changed" } } as unknown as NoticeKind, en),
  ).toBeNull();
});

const SHARMA = "0190a000-0000-7000-8000-000000000007";
const JOSHI = "0190a000-0000-7000-8000-000000000008";
const KAPOOR = "0190a000-0000-7000-8000-000000000009";
const names = (id: string) => ({ [SHARMA]: "Sharma v. State", [JOSHI]: "Joshi v. Joshi" })[id];
const claude = { kind: "claude", name: "claude-acp" } as const;

test("another matter's files refused: the line names the agent and the matter as the app's list names it now", () => {
  const kind: NoticeKind = { other_matter_refused: { matter: SHARMA, name: "Sharma (old name)" } };
  expect(noticeLine(kind, en, claude, names)).toBe(
    `Clauseo stopped a step: Claude tried to open ${iso("Sharma v. State")}'s files. That's another matter.`,
  );
  expect(noticeLine(kind, words("hi"), claude, names)).toBe(
    `Clauseo ने एक काम रोक दिया: Claude ने ${iso("Sharma v. State")} की फ़ाइलें खोलने की कोशिश की। वह दूसरा मामला है।`,
  );
});

test("with no agent name known, the refusal names the AI; a matter the list doesn't hold keeps the host's name", () => {
  const kind: NoticeKind = { other_matter_refused: { matter: KAPOOR, name: "Kapoor" } };
  expect(noticeLine(kind, en, undefined, names)).toBe(
    `Clauseo stopped a step: the AI tried to open ${iso("Kapoor")}'s files. That's another matter.`,
  );
  expect(noticeLine(kind, en)).toBe(
    `Clauseo stopped a step: the AI tried to open ${iso("Kapoor")}'s files. That's another matter.`,
  );
});

test("a refusal with no name anywhere gives null, so the row shows its text", () => {
  const kind = { other_matter_refused: { matter: KAPOOR } } as unknown as NoticeKind;
  expect(noticeLine(kind, en, claude, names)).toBeNull();
});

test("matters Clauseo couldn't fully keep Claude out of: each named, then how many more", () => {
  const kind: NoticeKind = { matters_unfenced: { matters: [SHARMA, JOSHI], more: 3 } };
  expect(noticeLine(kind, en, claude, names)).toBe(
    `Clauseo couldn't fully keep Claude out of these matters: ${iso("Sharma v. State")}, ${iso("Joshi v. Joshi")} and 3 more.`,
  );
  expect(noticeLine(kind, words("hi"), claude, names)).toBe(
    `Clauseo Claude को इन मामलों से पूरी तरह बाहर नहीं रख सका: ${iso("Sharma v. State")}, ${iso("Joshi v. Joshi")} और 3 और।`,
  );
});

test("the unfenced line with no more, with no agent name, and with a matter the list doesn't hold", () => {
  const two: NoticeKind = { matters_unfenced: { matters: [SHARMA, JOSHI], more: 0 } };
  expect(noticeLine(two, en, undefined, names)).toBe(
    `Clauseo couldn't fully keep the AI out of these matters: ${iso("Sharma v. State")} and ${iso("Joshi v. Joshi")}.`,
  );
  // Kapoor isn't in the list: counted with the more
  const unknown: NoticeKind = { matters_unfenced: { matters: [SHARMA, KAPOOR], more: 0 } };
  expect(noticeLine(unknown, en, claude, names)).toBe(
    `Clauseo couldn't fully keep Claude out of these matters: ${iso("Sharma v. State")} and 1 more.`,
  );
  // none named: the host's text shows
  const none: NoticeKind = { matters_unfenced: { matters: [KAPOOR], more: 2 } };
  expect(noticeLine(none, en, claude, names)).toBeNull();
});
