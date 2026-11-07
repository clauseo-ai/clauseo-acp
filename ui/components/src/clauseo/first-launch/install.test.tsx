// The install's working line in both languages (install.tsx): the size the download says, about;
// the bytes as whole MB with the words of getting ready; and the set-up with no bytes. Each line's
// live words hear the state alone, so the download's words, which change every 250 ms, are never
// what the region says.
import { expect, test } from "vitest";
import { loadHindi } from "../../lib/words-core.ts";
import { installLine, type InstallActs, type InstallView } from "./install.tsx";

const ACTS: InstallActs = {
  retry: () => {},
  proxy: () => {},
  copy: () => {},
  copied: null,
  setCopied: () => {},
};

/** The words of the line for a view, in the language; the live words too, when they differ. */
function linesOf(lang: "en" | "hi", view: InstallView) {
  const line = installLine(lang, view, "Claude", ACTS);
  if (line === null || line.kind !== "wait") throw new Error("no wait line");
  return { words: line.words, said: line.said ?? line.words };
}

test("install lines: missing says the size, about, in English and Hindi", async () => {
  await loadHindi();
  expect(linesOf("en", { kind: "missing", mb: 160 }).words).toBe(
    "Needs a download of about 160 MB",
  );
  expect(linesOf("hi", { kind: "missing", mb: 160 }).words).toBe("लगभग 160 MB का डाउनलोड ज़रूरी है");
});

test("install lines: downloading says its progress in whole MB, and the live words hear the state only", async () => {
  await loadHindi();
  const view: InstallView = {
    kind: "downloading",
    received: 45_000_000,
    total: 160_000_000,
    doneMb: 45,
    totalMb: 160,
  };
  const en = linesOf("en", view);
  expect(en.words).toBe("Getting Claude ready… 45 MB of 160 MB");
  expect(en.said).toBe("Getting Claude ready…");
  const hi = linesOf("hi", view);
  expect(hi.words).toBe("Claude तैयार हो रहा है… 160 MB में से 45 MB");
  expect(hi.said).toBe("Claude तैयार हो रहा है…");
});

test("install lines: installing is the set-up, with no bytes, in English and Hindi", async () => {
  await loadHindi();
  expect(linesOf("en", { kind: "installing" })).toEqual({
    words: "Setting up Claude…",
    said: "Setting up Claude…",
  });
  expect(linesOf("hi", { kind: "installing" }).words).toBe("Claude सेट अप हो रहा है…");
});

test("install lines: missing with no size says nothing, so no line stands", async () => {
  await loadHindi();
  expect(installLine("en", { kind: "missing", mb: 0 }, "Claude", ACTS)).toBeNull();
  expect(installLine("hi", { kind: "missing", mb: 0 }, "Claude", ACTS)).toBeNull();
});

test("install lines: downloaded, every download already on disk, says only the set-up is left, in English and Hindi", async () => {
  await loadHindi();
  expect(linesOf("en", { kind: "downloaded" })).toEqual({
    words: "Already downloaded. Only the set-up is left.",
    said: "Already downloaded. Only the set-up is left.",
  });
  expect(linesOf("hi", { kind: "downloaded" }).words).toBe(
    "पहले ही डाउनलोड हो चुका है। बस सेट अप बाकी है।",
  );
});
