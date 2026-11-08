// The install's working line in both languages (install.tsx): the size the download says, about;
// the bytes as whole MB with the words of getting ready; and the set-up with no bytes. Each line's
// live words hear the state alone, so the download's words, which change every 250 ms, are never
// what the region says.
import { isValidElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
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

test("install lines: a failed set-up says the download worked and has its own code; a failed download keeps its words", async () => {
  await loadHindi();
  const log = ["result: setup_failed after 3 tries"];
  const setup = installLine("en", { kind: "failed", log, setup: true }, "Claude", ACTS);
  if (setup === null || setup.kind !== "wait") throw new Error("no wait line");
  expect(setup.words).toBe(
    "Claude was downloaded, but setting it up didn't finish. Try again. If it keeps happening, ask your IT team to check your security software.",
  );
  expect(setup.code).toBe("agent.install_setup_failed");
  expect(linesOf("hi", { kind: "failed", log, setup: true }).words).toContain("डाउनलोड हो गया");
  const codex = installLine("en", { kind: "failed", log, setup: true }, "Codex", ACTS);
  if (codex === null || codex.kind !== "wait") throw new Error("no wait line");
  expect(codex.words).toMatch(/^Codex was downloaded, but setting it up didn't finish/);
  const download = installLine("en", { kind: "failed", log }, "Claude", ACTS);
  if (download === null || download.kind !== "wait") throw new Error("no wait line");
  expect(download.words).toBe("Claude couldn't be downloaded.");
  expect(download.code).toBe("agent.install_failed");
});

test("install lines: a failed set-up offers Try again and Details, not the system proxy; a failed download keeps it", () => {
  const actions = (setup: boolean) => {
    const line = installLine("en", { kind: "failed", log: ["x"], setup }, "Claude", ACTS);
    if (line === null || line.kind !== "wait") throw new Error("no wait line");
    return renderToStaticMarkup(<>{line.actions}</>);
  };
  const setup = actions(true);
  expect(setup).toContain("Try again");
  expect(setup).toContain("Details");
  expect(setup).not.toContain("Use the system proxy");
  // the proxy button alone goes: the card's other buttons stay (it had no Ask-IT button), and the
  // words keep the ask-your-IT path for a failure that keeps happening
  expect(setup.match(/<button/g)?.length).toBe(2);
  const words = installLine("en", { kind: "failed", log: ["x"], setup: true }, "Claude", ACTS);
  if (words === null || words.kind !== "wait") throw new Error("no wait line");
  expect(words.words).toContain("ask your IT team to check your security software");
  const download = actions(false);
  expect(download).toContain("Use the system proxy");
  expect(download.match(/<button/g)?.length).toBe(3);
});
