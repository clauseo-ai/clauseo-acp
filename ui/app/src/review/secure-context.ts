// The review served over plain http from another machine (the tailnet's http://…:8791/…/review/review.html) isn't a
// secure context, and there browsers leave out crypto.randomUUID. The room's commands call it, so a live cell threw
// and rendered blank. main.tsx imports this first; the app never does (a Tauri webview is a secure context). The
// stand-in is a version 4 UUID from crypto.getRandomValues, which every context has.

type Uuid = `${string}-${string}-${string}-${string}-${string}`;

/** A version 4 UUID from 16 random bytes. */
export function uuidV4(random: (bytes: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>): Uuid {
  const b = random(new Uint8Array(16));
  // the version (4) in the high nibble of byte 6, the variant (10xx) in the high bits of byte 8
  b[6] = (b[6]! % 16) + 0x40;
  b[8] = (b[8]! % 64) + 0x80;
  const h = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

if (typeof globalThis.crypto.randomUUID !== "function") {
  Object.defineProperty(globalThis.crypto, "randomUUID", {
    configurable: true,
    value: () => uuidV4((bytes) => globalThis.crypto.getRandomValues(bytes)),
  });
}
