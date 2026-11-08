// The install's working line in both languages (install.tsx): the size the download says, about;
// the bytes as whole MB with the words of getting ready; and the set-up with no bytes. Each line's
// live words hear the state alone, so the download's words, which change every 250 ms, are never
// what the region says.
import { isValidElement, type ReactElement } from "react";
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

/** The one control in a missing line's actions, or null. */
function controlOf(line: ReturnType<typeof installLine>) {
  if (line === null || line.kind !== "wait") throw new Error("no wait line");
  const wrap = line.actions as ReactElement<{
    children: ReactElement<{ onClick: () => void; "data-testid": string; children: string }> | null;
  }>;
  return wrap.props.children;
}

test("install lines: missing offers Sign in only where the caller gives the press, and it calls that press", () => {
  expect(controlOf(installLine("en", { kind: "missing", mb: 130 }, "Claude", ACTS))).toBeNull();
  let pressed = 0;
  const button = controlOf(
    installLine(
      "en",
      { kind: "missing", mb: 130 },
      "Claude",
      { ...ACTS, signIn: () => pressed++ },
      { signIn: "sign" },
    ),
  );
  if (button === null || !isValidElement(button)) throw new Error("no control");
  expect(button.props["data-testid"]).toBe("sign");
  expect(button.props.children).toBe("Sign in");
  button.props.onClick();
  expect(pressed).toBe(1);
});
