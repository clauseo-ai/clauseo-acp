import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";

// The faces, by the sha256 of each file in public/fonts. A changed or added font file fails here until its hash is
// recorded on purpose. The files came from the Anek release the design was approved on, with the Devanagari
// face's metric fixes.
const FACES: Record<string, string> = {
  "AnekLatin-var-latin.woff2": "99c0f2f774792af80ebde21c1b83743f3a32761474eac25c261eea90e1b581f2",
  "AnekLatin-var-latin-ext.woff2":
    "8fa33be6be56691748703c3198cf09d1373c54d34f21b4980164049666ed9ef3",
  "AnekDevanagari-var-devanagari.woff2":
    "04c5b4f109c124a20aa45ec221ad5dcae085198c80f656a3ca6c4c2846ef3668",
};
/** The Anek licence, shipped with the faces so the built app carries it; LICENSES/ is its home. */
const NOTICE = "OFL.txt";
const FONTS = new URL("../../public/fonts/", import.meta.url);
const LICENCE = new URL("../../../../LICENSES/OFL-1.1-anek.txt", import.meta.url);
const sha = (url: URL) => createHash("sha256").update(readFileSync(url)).digest("hex");

test("public/fonts holds exactly the recorded faces and their licence", () => {
  expect(readdirSync(FONTS).sort()).toEqual([...Object.keys(FACES), NOTICE].sort());
});

test.each(Object.entries(FACES))("%s is the recorded file", (file, hash) => {
  expect(sha(new URL(file, FONTS))).toBe(hash);
});

test("fonts.css loads only the recorded faces, as files from the app's own origin", () => {
  const css = readFileSync(new URL("./fonts.css", import.meta.url), "utf8");
  const urls = [...css.matchAll(/url\(["']?([^"')]*)["']?\)/g)].map((m) => m[1]);
  expect(urls.length).toBe(5);
  for (const u of urls) expect(Object.keys(FACES).map((f) => `/fonts/${f}`)).toContain(u);
});

test("the Anek licence is recorded with the repo's other licences", () => {
  expect(readFileSync(LICENCE, "utf8")).toContain("SIL OPEN FONT LICENSE Version 1.1");
});

test("the licence shipped with the faces is LICENSES/' copy, byte for byte", () => {
  expect(sha(new URL(NOTICE, FONTS))).toBe(sha(LICENCE));
});
