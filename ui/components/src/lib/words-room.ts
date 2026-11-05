// The room's own strings, beyond the shared ones in words.ts. The key is the exact English, with {slots} where it's
// built from parts. None of them uses मैं, पेंसिल, स्याही, चरण or
// कदम, or "pencil", "ink" or "step".
import type { Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import { t, type Key } from "./words.ts";
import type { HI } from "./words-room.hi.ts";

export type RoomKey = keyof typeof HI;

const room = wordsOf("room");
/** Whether the room holds a key of its own. Only Hindi asks: in English every key says itself, whichever file holds it. */
const isRoomKey = (key: string): key is RoomKey => Object.hasOwn(room.hi, key);

/** A string in the language: the room's own first, then the language's. */
export function rt(lang: Lang, key: RoomKey | Key, slots?: Slots): string {
  return lang === "hi" && isRoomKey(key) ? room(lang, key, slots) : t(lang, key as Key, slots);
}
