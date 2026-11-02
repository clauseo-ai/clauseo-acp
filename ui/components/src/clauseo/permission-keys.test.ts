// The prompt's keys apart from the DOM: which keydown is which answer, and where a key may reach the prompt from.
import { expect, test } from "vitest";
import { ANSWER_ORDER, keyReachesPrompt, permissionKey, type KeyWhere } from "./permission-keys.ts";

const press = (
  key: string,
  mods: Partial<Record<"meta" | "ctrl" | "shift" | "alt", boolean>> = {},
) => ({
  key,
  metaKey: mods.meta ?? false,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
});

test.each([
  ["Enter", press("Enter"), "once"],
  ["Cmd+Enter", press("Enter", { meta: true }), "always"],
  ["Ctrl+Enter", press("Enter", { ctrl: true }), "always"],
  ["Escape", press("Escape"), "deny"],
  ["Shift+Enter", press("Enter", { shift: true }), null],
  ["Alt+Enter", press("Enter", { alt: true }), null],
  ["Alt+Escape", press("Escape", { alt: true }), null],
  ["a letter", press("a"), null],
  ["Enter while composing", { ...press("Enter"), isComposing: true }, null],
  ["Enter with keyCode 229", { ...press("Enter"), keyCode: 229 }, null],
  ["Escape while composing", { ...press("Escape"), isComposing: true }, null],
  ["a held Enter's repeat", { ...press("Enter"), repeat: true }, null],
  ["a held Escape's repeat", { ...press("Escape"), repeat: true }, null],
  ["Enter with keyCode 13", { ...press("Enter"), keyCode: 13, isComposing: false }, "once"],
] as const)("%s is %s", (_name, e, answer) => {
  expect(permissionKey(e)).toBe(answer);
});

test.each([
  ["prompt", false, true],
  ["prompt", true, true],
  ["field", true, true],
  ["field", false, false],
  ["page", false, false],
  ["page", true, true],
  ["other", true, false],
  ["other", false, false],
] as const)(
  "a key from %s (field empty: %s) reaches the prompt: %s",
  (where: KeyWhere, empty, reaches) => {
    expect(keyReachesPrompt(where, empty)).toBe(reaches);
  },
);

test("the answers stand in ACP's kind order", () => {
  expect(ANSWER_ORDER).toEqual(["once", "always", "deny", "never"]);
});
