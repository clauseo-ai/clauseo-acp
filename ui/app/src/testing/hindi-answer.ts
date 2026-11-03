// A recorded Hindi answer (Hindi audit 5c): Claude Haiku 4.5's answer to the #137 format battery's p15 ("Answer in
// Hindi. Compare the limitation of liability clause in two draft supply agreements..."), run 1, through Claude Code
// 2.1.311, copied byte for byte from clauseo-acp-private
// measurements/137-markdown/format-battery/claude-code-2.1.311-claude-haiku-4-5/p15-r1.md: a Devanagari heading, a
// table with bold row heads and right-aligned ₹ amounts, a rule, and a paragraph citing an English case name.
// fake_agent's claude-hindi-answer streams the same text in chunks (hindi-answer.test.ts holds the two equal); the
// probe's ?thread=hindi shows it finished.
import HINDI_ANSWER from "./hindi-answer.md?raw";

/** The question asked in Hindi: compare the limitation of liability clause in two draft supply agreements. */
export const HINDI_QUESTION = "दो मसौदा आपूर्ति समझौतों में दायित्व सीमा खंड की तुलना कीजिए।";

export { HINDI_ANSWER };
