// @vitest-environment happy-dom
// The agent's question as a form in the room's live thread (elicitation-form.tsx), over FakeTransport: Claude's
// AskUserQuestion as claude-agent-acp sends it, the answer_elicitation each sends, Don't answer, the gates on Send and
// the lines naming a broken limit, refusals, a radio cleared, an entry that changes under the form, and a schema the
// form can't show. Focus, keys and groups are in elicitation-form-a11y.test.tsx, over the same rig.
import { act } from "react";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { expect, test } from "vitest";
import { upsert } from "../testing/stream-script.ts";
import { askEntry, ONE_QUESTION, TWO_QUESTIONS } from "./elicitation-fixtures.ts";
import { ELICITATION_TEST_IDS as T } from "./elicitation-schema.ts";
import { ask, ID, rig } from "./elicitation-form-rig.ts";
import type { Harness } from "./thread/harness.ts";

let h: Harness | null = null;
const { el, answered, fill, fillBad, held, keyOn } = rig(
  () => h,
  (next) => {
    h = next;
  },
);

test("one question renders as radios with their lines, and Other beneath them inside its group", async () => {
  h = await ask(ONE_QUESTION);
  const form = el<HTMLElement>(T.form);
  expect(form.querySelector("bdi")?.textContent).toBe(
    "Which court should the petition be filed in?",
  );
  const group = el<HTMLFieldSetElement>(T.field("question_0"));
  expect(group.tagName).toBe("FIELDSET");
  // the question is a group named by its legend; the radios alone are the radiogroup, Other beside it
  expect(group.getAttribute("role")).toBeNull();
  const legend = group.querySelector("legend")!;
  expect(legend.textContent).toBe("Court");
  const radiogroup = group.querySelector<HTMLElement>('[role="radiogroup"]')!;
  expect(radiogroup.getAttribute("aria-labelledby")).toBe(legend.id);
  expect(radiogroup.getAttribute("aria-required")).toBeNull();
  const radios = [...radiogroup.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
  expect(radios).toHaveLength(3);
  // one name: the arrows move within the group
  expect(new Set(radios.map((r) => r.name)).size).toBe(1);
  expect(radios[0]?.closest("label")?.textContent).toBe(
    "Delhi High CourtThe bank's registered office is in New Delhi.",
  );
  const line = document.getElementById(radios[0]!.getAttribute("aria-describedby")!);
  expect(line?.textContent).toBe("The bank's registered office is in New Delhi.");
  const other = el<HTMLTextAreaElement>(T.input("question_0_custom"));
  expect(other.tagName).toBe("TEXTAREA");
  expect(group.contains(other)).toBe(true);
  expect(radiogroup.contains(other)).toBe(false);
  expect(other.labels?.[0]?.textContent).toBe(th("en", "Other"));
  expect(document.getElementById(other.getAttribute("aria-describedby")!)?.textContent).toBe(
    "Type your own answer, or add a note to the option you chose above (optional).",
  );
  // nothing filled: Send waits, looks it (data-disabled), and the line says Don't answer goes on without an answer
  expect(held(T.send)).toBe(true);
  expect(el<HTMLElement>(T.send).hasAttribute("data-disabled")).toBe(true);
  expect(el<HTMLElement>(T.send).textContent).toBe(th("en", "Send"));
  expect(el<HTMLElement>(T.decline).textContent).toBe(th("en", "Don't answer"));
  expect(el<HTMLElement>(T.decline).getAttribute("aria-keyshortcuts")).toBe("Escape");
  expect(el<HTMLElement>(T.decline).hasAttribute("data-disabled")).toBe(false);
  expect(el<HTMLElement>(T.hint).textContent).toBe(
    th("en", "Choose an answer to send, or 'Don't answer' to go on without one."),
  );
  expect(h.one(T.decline)).not.toBeNull();
  expect(h.one(T.stop)).not.toBeNull();
});

test("one question: a pick sends its const; with an Other as well, both go", async () => {
  h = await ask(ONE_QUESTION);
  await h.click(el(T.choice("question_0", 1)));
  expect(held(T.send)).toBe(false);
  expect(el<HTMLElement>(T.send).hasAttribute("data-disabled")).toBe(false);
  expect(h.one(T.hint)).toBeNull();
  await fill(T.input("question_0_custom"), "File by Friday");
  await h.click(el(T.send));
  expect(answered()).toEqual([
    {
      thread: expect.any(String),
      elicitation: ID,
      response: {
        accept: {
          values: { question_0: "Sessions Court, Saket", question_0_custom: "File by Friday" },
        },
      },
    },
  ]);
  // one answer at a time: busy until the entry folds, and both answers look it
  expect(held(T.send)).toBe(true);
  expect(held(T.decline)).toBe(true);
  expect(el<HTMLElement>(T.send).hasAttribute("data-disabled")).toBe(true);
  expect(el<HTMLElement>(T.decline).hasAttribute("data-disabled")).toBe(true);
  await h.click(el(T.send));
  expect(answered()).toHaveLength(1);
  h.deliver([
    upsert(
      askEntry(3, ID, ONE_QUESTION, { resolved: { outcome: "accepted", by: "local_desktop" } }),
    ),
  ]);
  expect(h.one(T.form)).toBeNull();
  expect(h.one("elicitation")?.dataset["state"]).toBe("resolved");
  expect(h.one("elicitation")?.textContent).toBe(ONE_QUESTION.message);
});

test("two questions: radios and checkboxes, each with its Other; the values in schema order, a custom answer alone", async () => {
  h = await ask(TWO_QUESTIONS);
  const first = el<HTMLFieldSetElement>(T.field("question_0"));
  const second = el<HTMLFieldSetElement>(T.field("question_1"));
  expect(first.querySelectorAll('input[type="radio"]')).toHaveLength(2);
  expect(second.querySelectorAll('input[type="checkbox"]')).toHaveLength(3);
  expect(second.getAttribute("role")).toBeNull();
  // each question's words describe its group
  expect(document.getElementById(second.getAttribute("aria-describedby")!)?.textContent).toBe(
    "Which papers should be annexed?",
  );
  expect(first.contains(el(T.input("question_0_custom")))).toBe(true);
  expect(second.contains(el(T.input("question_1_custom")))).toBe(true);
  await h.click(el(T.choice("question_1", 2)));
  await h.click(el(T.choice("question_1", 0)));
  await fill(T.input("question_1_custom"), "Affidavit of service");
  await fill(T.input("question_0_custom"), "Both, quash first");
  await h.click(el(T.send));
  expect(answered().map((a) => a.response)).toEqual([
    {
      accept: {
        values: {
          question_0_custom: "Both, quash first",
          question_1: ["Notice to the bank", "Bank statement, March 2001"],
          question_1_custom: "Affidavit of service",
        },
      },
    },
  ]);
});

test("Don't answer sends decline", async () => {
  h = await ask(TWO_QUESTIONS);
  await h.click(el(T.choice("question_0", 0)));
  await h.click(el(T.decline));
  expect(answered().map((a) => a.response)).toEqual(["decline"]);
});

test("a required field holds Send, says so, and once filled Send goes", async () => {
  const schema = {
    type: "object",
    properties: {
      name: { type: "string", title: "Client's name" },
      note: { type: "string", title: "Note" },
    },
    required: ["name"],
  };
  h = await ask({ message: "Who is the client?", schema });
  const name = el(T.input("name"));
  expect(name.required).toBe(true);
  expect(name.getAttribute("aria-required")).toBe("true");
  expect(name.labels?.[0]?.textContent).toBe(`Client's name${th("en", "Required")}`);
  // something filled, but not the required field: Send still waits; a press says what's missing
  await fill(T.input("note"), "urgent");
  expect(held(T.send)).toBe(true);
  await h.click(el(T.send));
  expect(answered()).toEqual([]);
  const problem = el<HTMLElement>(T.problem("name"));
  expect(problem.textContent).toBe(th("en", "Answer this question."));
  expect(name.getAttribute("aria-invalid")).toBe("true");
  expect(name.getAttribute("aria-describedby")).toContain(problem.id);
  await fill(T.input("name"), "Kaveri Agro");
  expect(held(T.send)).toBe(false);
  await h.click(el(T.send));
  expect(answered().map((a) => a.response)).toEqual([
    { accept: { values: { name: "Kaveri Agro", note: "urgent" } } },
  ]);
});

{
  const code = "elicitation.invalid_values";
  test(`a refusal (${code}) says why under the answers and keeps the form as it was`, async () => {
    h = await ask(TWO_QUESTIONS, code);
    await h.click(el(T.choice("question_0", 1)));
    await fill(T.input("question_1_custom"), "Affidavit");
    // the live region stands before the refusal, empty, so its words are announced
    expect(el<HTMLElement>(T.error).getAttribute("role")).toBe("status");
    expect(el<HTMLElement>(T.error).textContent).toBe("");
    await h.click(el(T.send));
    expect(answered()).toHaveLength(1);
    expect(el<HTMLElement>(T.error).textContent).toBe(errorWords("en", code));
    expect(el(T.choice("question_0", 1)).checked).toBe(true);
    expect(el<HTMLTextAreaElement>(T.input("question_1_custom")).value).toBe("Affidavit");
    // the answers are free again
    expect(held(T.send)).toBe(false);
    await h.click(el(T.send));
    expect(answered()).toHaveLength(2);
  });
}

test("a refusal (elicitation.not_pending): the question can't be answered anymore, so only Stop is left beside its words", async () => {
  const code = "elicitation.not_pending";
  h = await ask(TWO_QUESTIONS, code);
  await h.click(el(T.choice("question_0", 1)));
  el<HTMLElement>(T.send).focus();
  await h.click(el(T.send));
  expect(answered()).toHaveLength(1);
  expect(el<HTMLElement>(T.error).textContent).toBe(errorWords("en", code));
  // the focused Send is gone: focus goes to Stop
  expect(document.activeElement).toBe(el(T.stop));
  expect(h.one(T.send)).toBeNull();
  expect(h.one(T.decline)).toBeNull();
  expect(h.one(T.hint)).toBeNull();
  expect(h.one(T.stop)).not.toBeNull();
  // nor do the keys answer
  await keyOn(el(T.choice("question_0", 1)), { key: "Enter" });
  await keyOn(el(T.choice("question_0", 1)), { key: "Escape" });
  expect(answered()).toHaveLength(1);
});

test("a chosen radio is cleared by a click or Space on it, so Other goes alone", async () => {
  h = await ask(ONE_QUESTION);
  const radio = el(T.choice("question_0", 1));
  await h.click(radio);
  expect(radio.checked).toBe(true);
  // a click on it again clears it
  await h.click(radio);
  expect(radio.checked).toBe(false);
  expect(held(T.send)).toBe(true);
  // Space on a focused radio is a click on it in the browser (happy-dom doesn't turn the key into one, so the click
  // stands for it): chosen, then cleared, and focus stays on the radio, whose state change the screen reader says
  await act(async () => radio.focus());
  await act(async () => radio.click());
  expect(radio.checked).toBe(true);
  await act(async () => radio.click());
  expect(radio.checked).toBe(false);
  expect(document.activeElement).toBe(radio);
  await fill(T.input("question_0_custom"), "Patiala House");
  await h.click(el(T.send));
  const sent = answered()[0]!.response as { accept: { values: Record<string, unknown> } };
  const values = sent.accept.values;
  expect(values).toEqual({ question_0_custom: "Patiala House" });
  expect("question_0" in values).toBe(false);
});

test("a pending entry delivered again as a new object keeps what the lawyer put in", async () => {
  h = await ask(TWO_QUESTIONS);
  await h.click(el(T.choice("question_0", 1)));
  await fill(T.input("question_1_custom"), "Affidavit");
  h.deliver([upsert(askEntry(3, ID, structuredClone(TWO_QUESTIONS)))]);
  await h.settle();
  expect(el(T.choice("question_0", 1)).checked).toBe(true);
  expect(el<HTMLTextAreaElement>(T.input("question_1_custom")).value).toBe("Affidavit");
  expect(held(T.send)).toBe(false);
});

for (const [how, state] of [
  ["expires", { expired: { reason: "turn_ended" } }],
  ["is answered elsewhere", { resolved: { outcome: "accepted", by: "local_desktop" } }],
] as const)
  test(`an entry that ${how} while the lawyer types drops the form, Send with it, and focus goes to the composer`, async () => {
    h = await ask(ONE_QUESTION);
    const other = el<HTMLTextAreaElement>(T.input("question_0_custom"));
    await act(async () => other.focus());
    await fill(T.input("question_0_custom"), "Saket");
    h.deliver([upsert(askEntry(3, ID, ONE_QUESTION, state))]);
    await h.settle();
    expect(h.one(T.form)).toBeNull();
    expect(h.one(T.send)).toBeNull();
    expect(h.one("elicitation")?.dataset["state"]).toBe(
      "resolved" in state ? "resolved" : "expired",
    );
    expect(document.activeElement).toBe(el<HTMLTextAreaElement>("composer"));
  });

test("a field over a limit the schema gives names it; with none to name, the general line", async () => {
  const schema = {
    type: "object",
    properties: {
      ref: { type: "string", title: "Ref", maxLength: 4 },
      grounds: { type: "string", title: "Grounds", minLength: 10 },
      initial: { type: "string", title: "Initial", minLength: 1, maxLength: 1 },
      papers: { type: "array", title: "Papers", maxItems: 1, items: { enum: ["A", "B"] } },
      witnesses: { type: "array", title: "Witnesses", minItems: 2, items: { enum: ["C", "D"] } },
      amount: { type: "number", title: "Amount", minimum: 1, maximum: 100 },
      days: { type: "integer", title: "Days", minimum: 15 },
      rate: { type: "number", title: "Rate", maximum: 24 },
      copies: { type: "integer", title: "Copies" },
      email: { type: "string", title: "Email", format: "email" },
    },
  };
  h = await ask({ message: "Details", schema });
  await fill(T.input("ref"), "NOTICE/1");
  await fill(T.input("grounds"), "Cheque");
  await fill(T.input("initial"), "AB");
  await h.click(el(T.choice("papers", 0)));
  await h.click(el(T.choice("papers", 1)));
  await h.click(el(T.choice("witnesses", 0)));
  await fill(T.input("amount"), "500");
  await fill(T.input("days"), "7");
  await fill(T.input("rate"), "36");
  await fill(T.input("copies"), "2.5");
  await fill(T.input("email"), "legal at the bank");
  await h.click(el(T.send));
  expect(answered()).toEqual([]);
  const line = (key: string) => el<HTMLElement>(T.problem(key)).textContent;
  expect(line("ref")).toBe(th("en", "Use {n} characters or fewer.", { n: 4 }));
  expect(line("grounds")).toBe(th("en", "Use at least {n} characters.", { n: 10 }));
  expect(line("initial")).toBe(th("en", "Use 1 character or fewer."));
  expect(line("papers")).toBe(th("en", "Choose up to {n}.", { n: 1 }));
  expect(line("witnesses")).toBe(th("en", "Choose at least {n}.", { n: 2 }));
  expect(line("amount")).toBe(th("en", "Use a number from {min} to {max}.", { min: 1, max: 100 }));
  expect(line("days")).toBe(th("en", "Use {min} or more.", { min: 15 }));
  expect(line("rate")).toBe(th("en", "Use {max} or less.", { max: 24 }));
  expect(line("copies")).toBe(th("en", "Use a whole number."));
  expect(line("email")).toBe(th("en", "This answer doesn't fit the question."));
  expect(line("ref")).toBe("Use 4 characters or fewer.");
});

test("a schema the form can't show: the agent's words, Don't answer and Stop, no fields and no Send", async () => {
  const schema = {
    type: "object",
    properties: { where: { type: "object", properties: {} } },
    required: ["where"],
  };
  h = await ask({ message: "Where is the office?", schema });
  const form = el<HTMLElement>(T.form);
  expect(form.dataset["supported"]).toBe("false");
  expect(form.querySelector("bdi")?.textContent).toBe("Where is the office?");
  expect(form.querySelector("input, textarea")).toBeNull();
  expect(h.one(T.send)).toBeNull();
  expect(el<HTMLElement>(T.unsupported).textContent).toBe(
    th("en", "This question can't be shown here. Choose 'Don't answer', or Stop the answer."),
  );
  await h.click(el(T.decline));
  expect(answered().map((a) => a.response)).toEqual(["decline"]);
  await h.click(el(T.stop));
  expect(h.sent("cancel")).toHaveLength(1);
});

test("a double click on Send, or Enter twice in one frame, sends one answer", async () => {
  h = await ask(ONE_QUESTION);
  await h.click(el(T.choice("question_0", 0)));
  const send = el<HTMLElement>(T.send);
  await act(async () => {
    send.click();
    send.click();
  });
  expect(answered()).toHaveLength(1);
  await h.unmount();
  h = await ask(ONE_QUESTION);
  const radio = el(T.choice("question_0", 2));
  await h.click(radio);
  await act(async () => {
    for (let i = 0; i < 2; i++)
      radio.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
  });
  expect(answered().map((a) => a.response)).toEqual([
    { accept: { values: { question_0: "Supreme Court" } } },
  ]);
});

test("numbers: an integer and a number go as Numbers; words the input can't read hold Send", async () => {
  const schema = {
    type: "object",
    properties: {
      copies: { type: "integer", title: "Copies", minimum: 1 },
      fee: { type: "number", title: "Fee" },
    },
  };
  h = await ask({ message: "How many copies, and the fee?", schema });
  const copies = el(T.input("copies"));
  expect(copies.type).toBe("number");
  expect(copies.step).toBe("1");
  expect(el(T.input("fee")).step).toBe("any");
  await fillBad(T.input("fee"));
  expect(held(T.send)).toBe(true);
  await act(async () => el(T.input("fee")).focus());
  await act(async () => el(T.input("copies")).focus());
  expect(el<HTMLElement>(T.problem("fee")).textContent).toBe(
    th("en", "This answer doesn't fit the question."),
  );
  Reflect.deleteProperty(el(T.input("fee")), "validity");
  await fill(T.input("fee"), "2500.5");
  await fill(T.input("copies"), "2.5");
  expect(held(T.send)).toBe(true);
  await fill(T.input("copies"), "3");
  await h.click(el(T.send));
  expect(answered().map((a) => a.response)).toEqual([
    { accept: { values: { copies: 3, fee: 2500.5 } } },
  ]);
});

test("a form of one box: Send waits until it's touched, then sends false as well as true; required shows its marker", async () => {
  const schema = {
    type: "object",
    properties: { urgent: { type: "boolean", title: "Urgent" } },
    required: ["urgent"],
  };
  h = await ask({ message: "Is it urgent?", schema });
  const box = el(T.input("urgent"));
  expect(box.closest("label")?.textContent).toBe(`Urgent${th("en", "Required")}`);
  expect(box.getAttribute("aria-required")).toBe("true");
  expect(held(T.send)).toBe(true);
  await h.click(box);
  await h.click(box);
  expect(box.checked).toBe(false);
  expect(held(T.send)).toBe(false);
  await h.click(el(T.send));
  expect(answered().map((a) => a.response)).toEqual([{ accept: { values: { urgent: false } } }]);
});

test("minItems and maxItems: too few or too many ticks hold Send and say so", async () => {
  const schema = {
    type: "object",
    properties: {
      papers: {
        type: "array",
        title: "Papers",
        minItems: 2,
        maxItems: 2,
        items: { enum: ["Notice", "Reply", "Minutes"] },
      },
    },
  };
  h = await ask({ message: "Which two papers?", schema });
  await h.click(el(T.choice("papers", 0)));
  expect(held(T.send)).toBe(true);
  expect(h.one(T.problem("papers"))).not.toBeNull();
  await h.click(el(T.choice("papers", 2)));
  expect(held(T.send)).toBe(false);
  expect(h.one(T.problem("papers"))).toBeNull();
  await h.click(el(T.choice("papers", 1)));
  expect(held(T.send)).toBe(true);
  await h.click(el(T.choice("papers", 1)));
  await h.click(el(T.send));
  expect(answered().map((a) => a.response)).toEqual([
    { accept: { values: { papers: ["Notice", "Minutes"] } } },
  ]);
});
