// @vitest-environment happy-dom
// "Copy details for IT" (copy-for-it.tsx): a press copies the note as given, the button says
// "Message copied" for 2 s then its label again; where the clipboard is refused (none, or writeText
// rejects) the note shows in a read-only field, selected, and the button never says it copied.
// itNote (words-first-launch.ts): the lead, then one line per fact given, in the page's language.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { itNote } from "../lib/words-first-launch.ts";
import { loadHindi } from "../lib/words-core.ts";
import { CopyForIt } from "./copy-for-it.tsx";

/** How long the button says "Message copied" (copy-for-it.tsx), in ms. */
const COPIED_MS = 2000;

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOTE = "Clauseo's Safe workspace is blocked on this computer.\nReason: policy";

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  Reflect.deleteProperty(navigator, "clipboard");
  vi.useRealTimers();
});

const clipboard = (writeText: (text: string) => Promise<void>) => {
  const fn = vi.fn(writeText);
  Object.defineProperty(navigator, "clipboard", { value: { writeText: fn }, configurable: true });
  return fn;
};
const button = () => host.querySelector<HTMLButtonElement>('[data-testid="copy-for-it"]')!;
const field = () => host.querySelector<HTMLTextAreaElement>('[data-testid="copy-for-it-text"]');
const press = async () => {
  await act(async () => button().click());
};

test("a press copies the note, says Message copied, then its label again after 2 s", async () => {
  vi.useFakeTimers();
  const writeText = clipboard(async () => {});
  await act(async () => root.render(<CopyForIt lang="en" text={NOTE} />));
  expect(button().textContent).toBe("Copy details for IT");
  await press();
  expect(writeText).toHaveBeenCalledWith(NOTE);
  expect(button().textContent).toBe("Message copied");
  expect(button().dataset["copied"]).toBe("");
  expect(field()).toBeNull();
  await act(async () => vi.advanceTimersByTime(COPIED_MS - 1));
  expect(button().textContent).toBe("Message copied");
  await act(async () => vi.advanceTimersByTime(1));
  expect(button().textContent).toBe("Copy details for IT");
});

test.each([
  ["writeText rejects", () => clipboard(() => Promise.reject(new Error("NotAllowedError")))],
  [
    "no clipboard",
    () => Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true }),
  ],
])(
  "the clipboard refused (%s): the note shows selected, and nothing says copied",
  async (_c, set) => {
    set();
    await act(async () => root.render(<CopyForIt lang="en" text={NOTE} />));
    await press();
    expect(button().textContent).toBe("Copy details for IT");
    expect(button().dataset["copied"]).toBeUndefined();
    const shown = field();
    expect(shown?.value).toBe(NOTE);
    expect(shown?.readOnly).toBe(true);
    expect(shown?.getAttribute("aria-label")).toBe("Copy details for IT");
    expect(document.activeElement).toBe(shown);
    expect(shown?.selectionStart).toBe(0);
    expect(shown?.selectionEnd).toBe(NOTE.length);
  },
);

test("in Hindi the button and its copied state say UD1-15's and UD1-16's words", async () => {
  await loadHindi();
  clipboard(async () => {});
  await act(async () => root.render(<CopyForIt lang="hi" text={NOTE} />));
  expect(button().textContent).toBe("IT के लिए विवरण कॉपी करें");
  await press();
  expect(button().textContent).toBe("संदेश कॉपी हो गया");
});

test("itNote: the lead, then each fact given, in order; facts left out say nothing", async () => {
  expect(
    itNote("en", "Lead.", {
      code: "sandbox.required",
      reason: "policy",
      site: "x.example",
      hosts: ["a.example", "b.example"],
      program: "C:\\Program Files\\Clauseo\\setup.exe",
      hash: "ab12",
      version: "3.0.4",
      link: "https://example.test/firm-it.md#x",
    }),
  ).toBe(
    [
      "Lead.",
      "Code: sandbox.required",
      "Reason: policy",
      "Site: x.example",
      "Addresses: a.example, b.example",
      "Program: C:\\Program Files\\Clauseo\\setup.exe",
      "Program hash: ab12",
      "Clauseo version: 3.0.4",
      "Clauseo for firm IT: https://example.test/firm-it.md#x",
    ].join("\n"),
  );
  expect(itNote("en", "Lead.", { version: "3.0.4", reason: "", link: "" })).toBe(
    "Lead.\nClauseo version: 3.0.4",
  );
  await loadHindi();
  expect(itNote("hi", "Lead.", { reason: "policy", version: "3.0.4", link: "u#x" })).toBe(
    "Lead.\nकारण: policy\nClauseo संस्करण: 3.0.4\nफ़र्म IT के लिए Clauseo: u#x",
  );
});
