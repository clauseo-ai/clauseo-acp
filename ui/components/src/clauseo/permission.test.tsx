// @vitest-environment happy-dom
// The permission prompt: its answers and their order, its keys from the room's composer, the page and the prompt
// itself, and its folded lines. The prompt names the modifier key of the computer the display says it runs on: Cmd on
// the Mac, Ctrl on Windows and Linux. The OS comes from the display alone, never from the webview's navigator.
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Platform } from "../lib/display.ts";
import { th, type ThreadKey } from "../lib/words-thread.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { PermissionPrompt } from "./permission.tsx";
import type { PermissionAnswer } from "./permission-keys.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { useRoom } from "./room-scope.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | undefined;
let root: ReturnType<typeof createRoot> | undefined;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = undefined;
  showDisplay(DEFAULT_DISPLAY);
  vi.restoreAllMocks();
});

type Props = Partial<ComponentProps<typeof PermissionPrompt>>;

/** The composer as the room draws it: its field handed to the room's nodes, unless the room has none yet. */
function Composer({ registered }: { registered: boolean }) {
  const room = useRoom();
  return (
    <form data-composer="">
      <textarea ref={registered ? room.nodes.slot<HTMLTextAreaElement>("field") : undefined} />
    </form>
  );
}

/** Draws the same room again with new props, as the caller does when the answer is on its way or the prompt folds. */
let again: (props: Props) => Promise<void> = async () => {};

/** A room with the prompt, another button and field, and a composer, as the room lays them out. */
async function show(props: Props = {}, platform: Platform = "windows", registered = true) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced", platform });
  const onAnswer = vi.fn<(a: PermissionAnswer) => void>();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  again = (next: Props) =>
    act(async () =>
      root?.render(
        <TooltipProvider>
          <RoomScopeProvider>
            <PermissionPrompt
              paper="Notice to the bank"
              state="pending"
              onAnswer={onAnswer}
              keys
              {...next}
            />
            <button type="button" className="other">
              Other
            </button>
            <input className="other-field" />
            <Composer registered={registered} />
          </RoomScopeProvider>
        </TooltipProvider>,
      ),
    );
  await again(props);
  return onAnswer;
}

const $ = (selector: string) => {
  const n = host?.querySelector(selector);
  if (n === null || n === undefined) throw new Error(`no ${selector}`);
  return n;
};
const field = () => $("textarea") as HTMLTextAreaElement;

/** A keydown on an element, as the browser sends it; returns whether something prevented it. */
async function key(
  on: Element,
  k: string,
  init: KeyboardEventInit = {},
  keyCode?: number,
): Promise<boolean> {
  const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init });
  if (keyCode !== undefined) Object.defineProperty(e, "keyCode", { value: keyCode });
  await act(async () => {
    on.dispatchEvent(e);
  });
  return e.defaultPrevented;
}

async function alwaysButton(platform: Platform) {
  await show({}, platform);
  const button = host?.querySelectorAll(".perm button")[1];
  if (button === undefined) throw new Error("no second answer");
  return {
    shortcut: button.getAttribute("aria-keyshortcuts"),
    kbd: button.querySelector(".perm-key")?.textContent,
  };
}

test.each([
  ["mac", "Meta+Enter", "⌘ Enter"],
  ["windows", "Control+Enter", "Ctrl Enter"],
  ["linux", "Control+Enter", "Ctrl Enter"],
] as const)("on %s, allowing for the matter is %s", async (platform, shortcut, kbd) => {
  expect(await alwaysButton(platform)).toEqual({ shortcut, kbd });
});

const testIds = { root: "perm", answer: (a: PermissionAnswer) => `perm-${a}` };

test("each offered answer has its button, in kind order whatever order it came in; the first allow is the tone", async () => {
  await show({ offered: ["never", "deny", "always", "once"], testIds });
  const buttons = [...(host?.querySelectorAll(".perm button") ?? [])];
  expect(
    buttons.map((b) => [
      b.getAttribute("data-testid"),
      b.querySelector(".deva-nudge")?.textContent,
    ]),
  ).toEqual([
    ["perm-once", "Allow once"],
    ["perm-always", "Always for this matter"],
    ["perm-deny", "Don't allow"],
    ["perm-never", "Don't allow for this matter"],
  ]);
  expect(buttons.map((b) => b.querySelector(".perm-key") !== null)).toEqual([
    true,
    true,
    true,
    false,
  ]);
  expect($('[data-testid="perm-never"]').getAttribute("aria-keyshortcuts")).toBeNull();
});

test("the default answers are the three with keys; with only always and never, always is the tone", async () => {
  await show();
  expect(host?.querySelectorAll(".perm button")).toHaveLength(3);
  await act(async () => root?.unmount());
  host?.remove();
  await show({ offered: ["never", "always"], testIds });
  expect(
    [...(host?.querySelectorAll(".perm button") ?? [])].map((b) => b.getAttribute("data-testid")),
  ).toEqual(["perm-always", "perm-never"]);
  expect($('[data-testid="perm-always"]').outerHTML).not.toEqual(
    $('[data-testid="perm-never"]').outerHTML,
  );
});

test("Enter in the empty composer answers once, and the composer never sees it", async () => {
  const onAnswer = await show();
  const composer = vi.fn();
  $("form").addEventListener("keydown", composer);
  expect(await key(field(), "Enter")).toBe(true);
  expect(onAnswer.mock.calls).toEqual([["once"]]);
  expect(composer).not.toHaveBeenCalled();
});

test("Enter with words in the composer is the composer's, not prevented", async () => {
  const onAnswer = await show();
  field().value = "Draft a reply";
  expect(await key(field(), "Enter")).toBe(false);
  expect(await key(field(), "Escape")).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

test("blank words count as empty", async () => {
  const onAnswer = await show();
  field().value = "  \n ";
  await key(field(), "Escape");
  expect(onAnswer.mock.calls).toEqual([["deny"]]);
});

test("Shift+Enter and an Enter that commits an input method's candidate never answer", async () => {
  const onAnswer = await show();
  expect(await key(field(), "Enter", { shiftKey: true })).toBe(false);
  expect(await key(field(), "Enter", { altKey: true })).toBe(false);
  expect(await key(field(), "Enter", { isComposing: true })).toBe(false);
  expect(await key(field(), "Enter", {}, 229)).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

test.each([
  ["mac", { metaKey: true }],
  ["windows", { ctrlKey: true }],
] as const)("on %s, the modifier with Enter answers always", async (platform, mods) => {
  const onAnswer = await show({}, platform);
  await key(field(), "Enter", mods);
  expect(onAnswer.mock.calls).toEqual([["always"]]);
});

test("Escape answers deny from the empty composer, from the page with the field empty and from inside the prompt", async () => {
  const onAnswer = await show();
  await key(field(), "Escape");
  await key(document.body, "Escape");
  await key($(".perm button"), "Escape");
  await key($(".perm-ask"), "Escape");
  expect(onAnswer.mock.calls).toEqual([["deny"], ["deny"], ["deny"], ["deny"]]);
});

test("from the page, the keys answer only while the room's field is empty", async () => {
  let onAnswer = await show();
  await key(document.body, "Enter");
  expect(onAnswer.mock.calls).toEqual([["once"]]);
  onAnswer.mockClear();
  field().value = "Draft a reply";
  expect(await key(document.body, "Enter")).toBe(false);
  expect(await key(document.body, "Escape")).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
  await act(async () => root?.unmount());
  host?.remove();
  // a room with no field handed over: the page's keys answer nothing
  onAnswer = await show({}, "windows", false);
  expect(await key(document.body, "Enter")).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

test("a held key's repeats never answer", async () => {
  const onAnswer = await show();
  expect(await key(field(), "Enter", { repeat: true })).toBe(false);
  expect(await key($(".perm-ask"), "Escape", { repeat: true })).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

test("after a click answer, focus inside the prompt goes back to the room's field", async () => {
  const onAnswer = await show();
  const button = $(".perm button") as HTMLButtonElement;
  button.focus();
  await act(async () => button.click());
  expect(onAnswer.mock.calls).toEqual([["once"]]);
  expect(document.activeElement).toBe(field());
});

test("after a key answer from inside the prompt, focus goes back to the room's field", async () => {
  const onAnswer = await show();
  const button = $(".perm button:nth-child(3)") as HTMLButtonElement;
  button.focus();
  await key(button, "Escape");
  expect(onAnswer.mock.calls).toEqual([["deny"]]);
  expect(document.activeElement).toBe(field());
});

test("when the prompt goes busy or folds with focus inside, focus goes back to the room's field", async () => {
  await show();
  ($(".perm button") as HTMLButtonElement).focus();
  await again({ busy: true });
  expect(document.activeElement).toBe(field());
  await again({});
  ($(".perm button") as HTMLButtonElement).focus();
  await again({ state: "gone" });
  expect(document.activeElement).toBe(field());
});

test("a fold with focus elsewhere leaves focus where it is", async () => {
  await show();
  const other = $(".other") as HTMLButtonElement;
  other.focus();
  await again({ state: "once" });
  expect(document.activeElement).toBe(other);
});

test("a plain Enter on a focused answer is left to the button's own click", async () => {
  const onAnswer = await show();
  expect(await key($(".perm button:nth-child(3)"), "Enter")).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

test("a key whose answer isn't offered does nothing", async () => {
  const onAnswer = await show({ offered: ["once", "never"] });
  expect(await key(field(), "Escape")).toBe(false);
  expect(await key(field(), "Enter", { ctrlKey: true })).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
  await key(field(), "Enter");
  expect(onAnswer.mock.calls).toEqual([["once"]]);
});

test("Enter and Escape in another field or on another button do nothing", async () => {
  const onAnswer = await show();
  expect(await key($(".other"), "Enter")).toBe(false);
  expect(await key($(".other-field"), "Enter")).toBe(false);
  expect(await key($(".other-field"), "Escape")).toBe(false);
  expect(onAnswer).not.toHaveBeenCalled();
});

const captureKeydowns = (spy: { mock: { calls: unknown[][] } }) =>
  spy.mock.calls.filter(([type, , capture]) => type === "keydown" && capture === true).length;

test.each([
  ["busy", { busy: true }],
  ["confirming", { confirming: true }],
  ["answered", { state: "once" }],
  ["gone", { state: "gone" }],
  ["without keys", { keys: false }],
] as const)("%s, no key listener exists and no key answers", async (_name, props: Props) => {
  const add = vi.spyOn(document, "addEventListener");
  const onAnswer = await show(props);
  expect(captureKeydowns(add)).toBe(0);
  await key(field(), "Enter");
  await key(document.body, "Escape");
  expect(onAnswer).not.toHaveBeenCalled();
});

test.each([
  ["busy", { busy: true }],
  ["confirming", { confirming: true }],
] as const)(
  "%s, the answers are disabled, never inert, and a click on one answers nothing",
  async (_name, props: Props) => {
    const onAnswer = await show(props);
    expect($(".perm-actions").hasAttribute("inert")).toBe(false);
    const buttons = [...(host?.querySelectorAll<HTMLButtonElement>(".perm button") ?? [])];
    expect(buttons.map((b) => b.getAttribute("aria-disabled"))).toEqual(["true", "true", "true"]);
    await act(async () => buttons[0]?.click());
    expect(onAnswer).not.toHaveBeenCalled();
    await again({});
    expect([...(host?.querySelectorAll(".perm [aria-disabled]") ?? [])]).toHaveLength(0);
  },
);

test("Every change is kept shows by default and describes the prompt, and hides when kept is false", async () => {
  await show();
  const kept = $(".perm-kept");
  expect(kept.textContent).toBe("Every change is kept.");
  expect($(".perm").getAttribute("aria-describedby")).toBe(kept.id);
  await act(async () => root?.unmount());
  host?.remove();
  await show({ kept: false });
  expect(host?.querySelector(".perm-kept")).toBeNull();
  expect($(".perm").hasAttribute("aria-describedby")).toBe(false);
});

test("the question replaces the ask line, the title stands beneath it, and no paper asks plainly", async () => {
  await show({ question: "Clauseo wants to run a command.", title: "rm -rf build" });
  expect($(".perm-ask").textContent).toBe("Clauseo wants to run a command.");
  expect($(".perm-title bdi").textContent).toBe("rm -rf build");
  await act(async () => root?.unmount());
  host?.remove();
  await show({ paper: null });
  expect($(".perm-ask").textContent).toBe("Clauseo asks before going on.");
  expect(host?.querySelector(".perm-title")).toBeNull();
});

const BANK = "Notice to the bank";
test.each([
  ["once", undefined, BANK, "Allowed once. Clauseo saved a new version of Notice to the bank."],
  [
    "always",
    undefined,
    BANK,
    "Allowed for this matter. Clauseo saved a new version of Notice to the bank.",
  ],
  ["deny", undefined, BANK, "Not allowed. Clauseo left Notice to the bank as it was."],
  ["once", true, BANK, "Allowed once. Clauseo saved a new version of Notice to the bank."],
  ["once", false, BANK, "Allowed once."],
  ["once", true, null, "Allowed once."],
  ["once", false, null, "Allowed once."],
  [
    "always",
    true,
    BANK,
    "Allowed for this matter. Clauseo saved a new version of Notice to the bank.",
  ],
  ["always", false, BANK, "Allowed for this matter."],
  ["always", true, null, "Allowed for this matter."],
  ["always", false, null, "Allowed for this matter."],
  ["deny", false, BANK, "Not allowed. Clauseo left Notice to the bank as it was."],
  ["deny", true, BANK, "Not allowed."],
  ["deny", false, null, "Not allowed."],
  ["deny", true, null, "Not allowed."],
  ["never", true, BANK, "Not allowed for this matter."],
  ["never", false, null, "Not allowed for this matter."],
  ["never", undefined, BANK, "Not allowed for this matter."],
  ["gone", true, BANK, "No longer waiting for an answer."],
  ["gone", false, null, "No longer waiting for an answer."],
  ["gone", undefined, BANK, "No longer waiting for an answer."],
] as const)("answered %s, done %s, paper %s: %s", async (state, done, paper, line) => {
  await show({ state, done, paper });
  expect($(".perm-done").textContent).toBe(line);
});

test.each([
  ["mac", "macOS is asking you to confirm."],
  ["windows", "Windows is asking you to confirm."],
] as const)("confirming on %s says %s, in the room's own class", async (platform, line) => {
  await show({ confirming: true }, platform);
  const status = $('.perm [role="status"]');
  expect([status.className, status.textContent]).toEqual(["perm-confirming", line]);
  expect($(".perm").hasAttribute("data-confirming")).toBe(true);
});

test("confirming on linux names no dialog, Linux isn't a target yet", async () => {
  await show({ confirming: true }, "linux");
  expect(host?.querySelector('.perm [role="status"]')).toBeNull();
  expect($(".perm").hasAttribute("data-confirming")).toBe(true);
});

test("not confirming, the root carries no data-confirming", async () => {
  await show();
  expect($(".perm").hasAttribute("data-confirming")).toBe(false);
});

test("the paper glyph stands only when a paper is named", async () => {
  await show({ paper: null });
  expect(host?.querySelector(".perm-icon")).toBeNull();
  await act(async () => root?.unmount());
  host?.remove();
  await show();
  expect(host?.querySelector(".perm-icon")).not.toBeNull();
});

test("the folded line is never announced as live, and the glyph-less one is marked to line up", async () => {
  await show({ state: "gone" });
  expect($(".perm-done").getAttribute("aria-live")).toBe("off");
  expect($(".perm-done").getAttribute("data-glyph")).toBe("none");
  expect(host?.querySelector(".perm-done-icon")).toBeNull();
  await again({ state: "deny" });
  expect($(".perm-done").getAttribute("aria-live")).toBe("off");
  expect($(".perm-done").hasAttribute("data-glyph")).toBe(false);
});

test("an error stands under the answers as a status", async () => {
  await show({ error: "The answer didn't reach Claude." });
  expect($('.perm-actions + [role="status"]').textContent).toBe("The answer didn't reach Claude.");
  await act(async () => root?.unmount());
  host?.remove();
  await show({ error: null });
  expect(host?.querySelector('.perm [role="status"]')).toBeNull();
});

test("the test ids and the caller's data attributes stand on both forms", async () => {
  await show({ testIds, dataState: "waiting", dataAnswer: "none" });
  const pending = $('[data-testid="perm"]');
  expect([
    pending.className,
    pending.getAttribute("data-state"),
    pending.getAttribute("data-answer"),
  ]).toEqual(["perm", "waiting", "none"]);
  await act(async () => root?.unmount());
  host?.remove();
  await show({ testIds, state: "deny", dataState: "answered", dataAnswer: "deny" });
  const done = $('[data-testid="perm"]');
  expect([
    done.className,
    done.getAttribute("data-state"),
    done.getAttribute("data-answer"),
  ]).toEqual(["perm-done", "answered", "deny"]);
  await act(async () => root?.unmount());
  host?.remove();
  await show({ state: "always" });
  expect($(".perm-done").getAttribute("data-state")).toBe("always");
});

const NEW_WORDS: readonly ThreadKey[] = [
  "Don't allow for this matter",
  "Clauseo asks before going on.",
  "No longer waiting for an answer.",
  "Allowed once.",
  "Allowed for this matter.",
  "Not allowed.",
  "Not allowed for this matter.",
];
test("the new words have Hindi, and the short folded lines open the long ones' Hindi", () => {
  for (const k of NEW_WORDS) {
    const hi = th("hi", k);
    expect(hi, k).toMatch(/[\u0900-\u097F]/);
    if (k.endsWith(".")) expect(hi, k).toMatch(/।$/);
  }
  const p = { paper: BANK };
  expect(th("hi", "Allowed once. Clauseo saved a new version of {paper}.", p)).toMatch(
    new RegExp(`^${th("hi", "Allowed once.")}`),
  );
  expect(th("hi", "Allowed for this matter. Clauseo saved a new version of {paper}.", p)).toMatch(
    new RegExp(`^${th("hi", "Allowed for this matter.")}`),
  );
  expect(th("hi", "Not allowed. Clauseo left {paper} as it was.", p)).toMatch(
    new RegExp(`^${th("hi", "Not allowed.")}`),
  );
});
