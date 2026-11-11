// @vitest-environment happy-dom
// The finished answer's Copy and Try again (D-026, D-027), a code block's Copy, and a task's row and permission prompt
// in plain words with the command folded and cut to the matter (D-016, D-017, D-046), in the real room.
import { afterEach, expect, test, vi } from "vitest";
import {
  agentText,
  permission,
  reset,
  step,
  toolCall,
  upsert,
  userMessage,
} from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  Reflect.deleteProperty(navigator, "clipboard");
});

const FOLDER = "Documents/Clauseo/Sharma";
const ROOT = `/Users/lawyer/${FOLDER}`;
const ASKED = userMessage(5, 1, "What does the notice ask?");
const ANSWER = agentText(20, 1, "It asks for payment.\n\n```\nSection 138\n```");

const clipboard = () => {
  const writeText = vi.fn(() => Promise.resolve());
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  return writeText;
};

const python = (status: ToolCallStatus, over: Partial<ToolCallEntry> = {}): ViewEntry => ({
  tool_call: {
    ...toolCall(7, 201, status).tool_call!,
    kind: "execute",
    title: "",
    raw_input: {
      inline: JSON.stringify({
        command: ["bash", "-lc", `python3 read.py "${ROOT}/papers/Legal notice.pdf"`],
      }),
    },
    ...over,
  },
});

const mount = () => mountRoom({ matters: [{ ...THE_MATTER, folder_display: FOLDER }] });

test("a finished answer offers Copy, its words as written; no Try again after an answer that ended well", async () => {
  const writeText = clipboard();
  h = await mount();
  h.deliver([reset([ASKED, ANSWER, step(30)])]);
  await h.settle();
  expect(h.one("answer-try-again")).toBeNull();
  await h.click(h.one("answer-copy"));
  expect(writeText).toHaveBeenCalledWith("It asks for payment.\n\n```\nSection 138\n```");
  // the copy icon, named Copy; once pressed a polite live region beside it says Copied
  const answerCopy = h.one("answer-copy")!;
  expect(answerCopy.getAttribute("aria-label")).toBe("Copy");
  expect(answerCopy.textContent).toBe("");
  expect(answerCopy.parentElement?.querySelector('[role="status"]')?.textContent).toBe("Copied");
  // the code block's own Copy, its code alone, the same control
  const codeCopy = h.one("code-copy")!;
  expect(codeCopy.getAttribute("aria-label")).toBe("Copy");
  await h.click(codeCopy);
  expect(writeText).toHaveBeenLastCalledWith("Section 138");
  // it stands in the block's wrapper beside the pre, never inside the code that scrolls
  expect(codeCopy.closest('[data-clauseo="code-block"]')).not.toBeNull();
  expect(codeCopy.closest("pre")).toBeNull();
});

test("after a Stop, Try again sends the same question as a new turn; only on the latest answer", async () => {
  h = await mount();
  h.deliver([reset([ASKED, agentText(20, 1, "It asks"), step(30, { stop: "cancelled" })])]);
  await h.settle();
  await h.click(h.one("answer-try-again"));
  expect(h.sent("send_prompt").map((s) => s.content)).toEqual([
    [{ text: { text: "What does the notice ask?" } }],
  ]);
});

test("a failed answer offers Try again too; a running one offers neither", async () => {
  h = await mount();
  h.deliver([reset([ASKED, step(30, { stop: "failed", failure: "agent.exited" })])]);
  await h.settle();
  expect(h.one("answer-try-again")).not.toBeNull();
  // an answer with no words of its own has nothing to copy
  expect(h.one("answer-copy")).toBeNull();
  await h.unmount();
  h = await mount();
  h.deliver([reset([ASKED, agentText(20, 1, "It asks")])]);
  await h.settle();
  expect(h.one("answer-actions")).toBeNull();
});

test("a task's row says the program and the paper, folds its command cut to the matter, and a failure's why", async () => {
  h = await mount();
  const out = { inline: JSON.stringify({ stderr: "Error: the file is password-protected" }) };
  h.deliver([reset([ASKED, python("completed"), toolCall(8, 202, "completed"), ANSWER, step(30)])]);
  await h.settle();
  await h.click(h.one("step-group")?.querySelector("button"));
  const row = h.all("tool-call")[0]!;
  expect(row.querySelector(".step-row-text bdi")?.textContent).toBe(
    "Ran python3 on Legal notice.pdf.",
  );
  expect(row.querySelector("[data-testid=task-command]")).toBeNull();
  await h.click(row.querySelector("[data-testid=task-command-press]"));
  expect(row.querySelector("[data-testid=task-command]")?.textContent).toBe(
    'python3 read.py "papers/Legal notice.pdf"',
  );
  h.deliver([upsert(python("failed", { raw_output: out }))]);
  await h.settle();
  expect(h.all("tool-call")[0]!.querySelector("[data-testid=tool-call-why]")?.textContent).toBe(
    "Error: the file is password-protected",
  );
});

test("a task's permission prompt says what it would do in plain words, its command folded and cut", async () => {
  h = await mount();
  h.deliver([reset([ASKED, python("pending"), permission(8, 201)])]);
  await h.settle();
  const prompt = h.one("permission")!;
  expect(prompt.querySelector(".perm-title bdi")?.textContent).toBe(
    "Run python3 on Legal notice.pdf.",
  );
  expect(prompt.textContent).not.toContain(ROOT);
  await h.click(prompt.querySelector("[data-testid=task-command-press]"));
  expect(prompt.querySelector("[data-testid=task-command]")?.textContent).toBe(
    'python3 read.py "papers/Legal notice.pdf"',
  );
});
