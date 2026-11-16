// The parts copy the protocol's ErrorCode, since ui/components can't import the app. The two unions must hold the same
// codes: a code on the wire with no words, or words for a code the wire never sends, fails typecheck here, both ways.
// ERROR_WORDS is a Record over the parts' union less its WordlessCode (retired words, read as "unknown"), so every
// other code on the wire has words. The parts copy the protocol's PolicySetting the same way (words-errors-policy.ts),
// for the policy line's {setting} slot.
import { expect, test } from "vitest";
import { ERROR_WORDS, type ErrorCode as WordsCode } from "@clauseo/components/lib/words-errors.ts";
import type { PolicySetting as WordsSetting } from "@clauseo/components/lib/words-errors-policy.ts";
import type {
  ErrorCode as WireCode,
  PolicySetting as WireSetting,
} from "../../transport/protocol.ts";

/** true only when every A is a B and every B an A. */
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

const wireInWords: Same<WireCode, WordsCode> = true;
const wordsInWire: Same<WordsCode, WireCode> = true;
const settingsSame: Same<WireSetting, WordsSetting> = true;

test("every code on the wire has words, and the words name no code the wire lacks", () => {
  expect([wireInWords, wordsInWire]).toEqual([true, true]);
  expect(Object.keys(ERROR_WORDS)).toContain("unknown");
});

test("the policy line's settings are the wire's PolicySetting, both ways", () => {
  expect(settingsSame).toBe(true);
});
