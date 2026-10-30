// The gallery's choices: the display bar's and the specimens'. Labels without a key in the words files are the
// builder's, in English.
import {
  TEXT_SIZES,
  type Lang,
  type Platform,
  type Theme,
} from "@clauseo/components/lib/display.ts";

export const THEMES = [
  { value: "day", label: "Day" },
  { value: "night", label: "Night" },
] as const satisfies readonly { value: Theme; label: string }[];

export const LANGS = [
  { value: "en", label: "English", lang: "en" },
  { value: "hi", label: "हिन्दी", lang: "hi" },
] as const satisfies readonly { value: Lang; label: string; lang: Lang }[];

export const PLATFORMS = [
  { value: "mac", label: "Mac" },
  { value: "windows", label: "Windows" },
] as const satisfies readonly { value: Platform; label: string }[];

export const SIZES = TEXT_SIZES.map((s) => ({ value: String(s), label: String(s) }));

export const VIEWS = [
  { value: "list", label: "List" },
  { value: "grid", label: "Grid" },
  { value: "pages", label: "Pages", disabled: true },
] as const;

export type View = (typeof VIEWS)[number]["value"];

export type LiteChoice = "auto" | "on" | "off";
