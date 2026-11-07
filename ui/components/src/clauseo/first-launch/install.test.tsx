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

test("install lines: unreachable names the hosts for IT, and offline stays the offline line, in English and Hindi", async () => {
  await loadHindi();
  const unreachable: InstallView = {
    kind: "unreachable",
    hosts: ["registry.npmjs.org", "nodejs.org"],
  };
  const line = installLine("en", unreachable, "Claude", ACTS);
  expect(line?.kind === "wait" && line.code).toBe("agent.install_unreachable");
  expect(linesOf("en", unreachable).words).toBe(
    "Clauseo can't reach its download server (registry.npmjs.org, nodejs.org). If your internet is working, ask your IT team to allow these addresses. Clauseo keeps trying.",
  );
  expect(linesOf("hi", unreachable).words).toBe(
    "Clauseo अपने डाउनलोड सर्वर (registry.npmjs.org, nodejs.org) तक नहीं पहुँच पा रहा है। अगर आपका इंटरनेट चल रहा है, तो अपनी IT टीम से इन पतों को अनुमति देने के लिए कहें। Clauseo कोशिश करता रहेगा।",
  );
  const offline = installLine("en", { kind: "offline" }, "Claude", ACTS);
  expect(offline?.kind === "wait" && offline.code).toBe("agent.install_offline");
  expect(linesOf("en", { kind: "offline" }).words).toBe(
    "You're offline. Setting up waits for the connection.",
  );
});
