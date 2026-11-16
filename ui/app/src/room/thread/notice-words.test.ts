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

test("a step the guard stopped on a paper open in Word names the agent and the paper, in English and Hindi", () => {
  const kind: NoticeKind = { held_open_blocked: { file: "Drafts/Lease.docx" } };
  const codex = { kind: "codex", name: "codex-acp" } as const;
  expect(noticeLine(kind, en, codex)).toBe(
    `Clauseo stopped a step: ChatGPT tried to change ${iso("Lease.docx")}, which is open in Word. Close it in Word, then ask again.`,
  );
  expect(noticeLine(kind, words("hi"), codex)).toBe(
    `Clauseo ने एक काम रोक दिया: ChatGPT ने ${iso("Lease.docx")} बदलने की कोशिश की, जो Word में खुला है। इसे Word में बंद करें, फिर दोबारा पूछें।`,
  );
  expect(noticeLine(kind, en)).toBe(
    `Clauseo stopped a step: the AI tried to change ${iso("Lease.docx")}, which is open in Word. Close it in Word, then ask again.`,
  );
  expect(noticeLine(kind, words("hi"))).toBe(
    `Clauseo ने एक काम रोक दिया: AI ने ${iso("Lease.docx")} बदलने की कोशिश की, जो Word में खुला है। इसे Word में बंद करें, फिर दोबारा पूछें।`,
  );
});

test("a step the guard stopped in the .codex folder names the agent's own settings, in English and Hindi", () => {
  const codex = { kind: "codex", name: "codex-acp" } as const;
  expect(noticeLine("codex_folder_blocked", en, codex)).toBe(
    "Clauseo stopped a step: ChatGPT tried to change its own settings in this matter's folder.",
  );
  expect(noticeLine("codex_folder_blocked", words("hi"), codex)).toBe(
    "Clauseo ने एक काम रोक दिया: ChatGPT ने इस मामले के फ़ोल्डर में अपनी सेटिंग्स बदलने की कोशिश की।",
  );
  expect(noticeLine("codex_folder_blocked", en)).toBe(
    "Clauseo stopped a step: the AI tried to change its own settings in this matter's folder.",
  );
  expect(noticeLine("codex_folder_blocked", words("hi"))).toBe(
    "Clauseo ने एक काम रोक दिया: AI ने इस मामले के फ़ोल्डर में अपनी सेटिंग्स बदलने की कोशिश की।",
  );
});

test("a step the guard stopped with no paper named, and the turn's count of more, in English and Hindi", () => {
  const codex = { kind: "codex", name: "codex-acp" } as const;
  expect(noticeLine("guard_step_blocked", en, codex)).toBe("Clauseo stopped a step.");
  expect(noticeLine("guard_step_blocked", words("hi"))).toBe("Clauseo ने एक काम रोक दिया।");
  expect(noticeLine({ guard_steps_more: { more: 1 } }, en, codex)).toBe(
    "Clauseo stopped one more step.",
  );
  expect(noticeLine({ guard_steps_more: { more: 1 } }, words("hi"))).toBe(
    "Clauseo ने एक और काम रोका।",
  );
  expect(noticeLine({ guard_steps_more: { more: 3 } }, en)).toBe("Clauseo stopped 3 more steps.");
  expect(noticeLine({ guard_steps_more: { more: 3 } }, words("hi"))).toBe(
    "Clauseo ने 3 और काम रोके।",
  );
  expect(noticeLine({ guard_steps_more: { more: 0 } }, en)).toBeNull();
});

test("a guard notice with no paper gives null, so the row shows its text", () => {
  expect(noticeLine({ held_open_blocked: {} } as unknown as NoticeKind, en)).toBeNull();
});

test("a refused model switch names both models, isolated; English in Hindi until its Hindi comes", () => {
  const kind: NoticeKind = { model_switch_refused: { picked: "Haiku", kept: "Opus" } };
  const line = `${iso("Haiku")} can't run in Claude's automatic mode, so Claude stays on ${iso("Opus")}. To use a different model, pick another one.`;
  expect(noticeLine(kind, en)).toBe(line);
  expect(noticeLine(kind, words("hi"))).toBe(line);
  // a name missing or blank gives null, so the row shows the host's text
  const blank = { model_switch_refused: { picked: " ", kept: "Opus" } } as NoticeKind;
  expect(noticeLine(blank, en)).toBeNull();
  const half = { model_switch_refused: { picked: "Haiku" } } as unknown as NoticeKind;
  expect(noticeLine(half, en)).toBeNull();
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
