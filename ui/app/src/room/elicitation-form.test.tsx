// @vitest-environment happy-dom
// The agent's question as a form in the room's live thread (elicitation-form.tsx), over FakeTransport: Claude's
// AskUserQuestion as claude-agent-acp sends it, the answer_elicitation each sends, Decline, the gates on Send,
// refusals, the keys, where focus goes, and a schema the form can't show.
import { act } from "react";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset, upsert, uuid } from "../testing/stream-script.ts";
import type { HostCommand, HostError, ViewEntry } from "../transport/protocol.ts";
import { askEntry, ONE_QUESTION, TWO_QUESTIONS, type AskForm } from "./elicitation-fixtures.ts";
import { ELICITATION_TEST_IDS as T } from "./elicitation-schema.ts";
import { defaultOutcome, mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ID = uuid(1, 9);
const hostError = (code: string): HostError =>
  ({ code, message: "refused", retryable: false }) as HostError;

/** The room, its turn waiting on the lawyer at these entries; answer_elicitation refused with `refuse` when given. */
async function room(entries: ViewEntry[], refuse?: string, blur = false): Promise<Harness> {
  const harness = await mountRoom({
    dispatch: (command: HostCommand) => {
      if (refuse !== undefined && command.answer_elicitation !== undefined) throw hostError(refuse);
      return defaultOutcome(command);
    },
  });
  if (blur) await act(async () => (document.activeElement as HTMLElement | null)?.blur());
  harness.deliver([reset(entries, header("needs_you"))]);
  await harness.settle();
  return harness;
}
const ask = (form: AskForm, refuse?: string) => room([askEntry(3, ID, form)], refuse);

const el = <E extends HTMLElement = HTMLInputElement>(id: string): E => {
  const found = h!.one(id);
  if (found === null) throw new Error(`no ${id}`);
  return found as E;
};
const answered = () => h!.sent("answer_elicitation");

async function fill(id: string, text: string) {
  const node = el<HTMLInputElement | HTMLTextAreaElement>(id);
  await act(async () => {
    const proto = node instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
    Reflect.set(proto.prototype, "value", text, node);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function keyOn(node: Element, init: KeyboardEventInit): Promise<boolean> {
  const e = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  await act(async () => void node.dispatchEvent(e));
  await h!.settle();
  return e.defaultPrevented;
}
const held = (id: string) => el<HTMLElement>(id).getAttribute("aria-disabled") === "true";

test("one question renders as radios with their lines, and Other beneath them inside its group", async () => {
  h = await ask(ONE_QUESTION);
  const form = el<HTMLElement>(T.form);
  expect(form.querySelector("bdi")?.textContent).toBe(
    "Which court should the petition be filed in?",
  );
  const group = el<HTMLFieldSetElement>(T.field("question_0"));
  expect(group.tagName).toBe("FIELDSET");
  expect(group.getAttribute("role")).toBe("radiogroup");
  expect(group.querySelector("legend")?.textContent).toBe("Court");
  const radios = [...group.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
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
  expect(other.labels?.[0]?.textContent).toBe(th("en", "Other"));
  expect(document.getElementById(other.getAttribute("aria-describedby")!)?.textContent).toBe(
    "Type your own answer, or add a note to the option you chose above (optional).",
  );
  // nothing filled: Send waits and the line says Decline goes on without an answer
  expect(held(T.send)).toBe(true);
  expect(el<HTMLElement>(T.hint).textContent).toBe(
    th("en", "Choose an answer to send, or Decline to go on without one."),
  );
  expect(h.one(T.decline)).not.toBeNull();
  expect(h.one(T.stop)).not.toBeNull();
});

test("one question: a pick sends its const; with an Other as well, both go", async () => {
  h = await ask(ONE_QUESTION);
  await h.click(el(T.choice("question_0", 1)));
  expect(held(T.send)).toBe(false);
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
  // one answer at a time: busy until the entry folds
  expect(held(T.send)).toBe(true);
  expect(held(T.decline)).toBe(true);
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

test("Decline sends decline", async () => {
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

for (const code of ["elicitation.invalid_values", "elicitation.not_pending"]) {
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

test("Enter on a radio sends; Enter in Other is a new line, and Cmd or Ctrl with Enter sends", async () => {
  h = await ask(ONE_QUESTION);
  const other = el<HTMLTextAreaElement>(T.input("question_0_custom"));
  await fill(T.input("question_0_custom"), "Saket, if the bank agrees");
  expect(await keyOn(other, { key: "Enter" })).toBe(false);
  expect(answered()).toEqual([]);
  expect(await keyOn(other, { key: "Enter", metaKey: true })).toBe(true);
  expect(answered().map((a) => a.response)).toEqual([
    { accept: { values: { question_0_custom: "Saket, if the bank agrees" } } },
  ]);
  await h.unmount();
  h = await ask(TWO_QUESTIONS);
  const radio = el(T.choice("question_0", 0));
  // nothing filled: Enter sends nothing
  await keyOn(radio, { key: "Enter" });
  expect(answered()).toEqual([]);
  await h.click(radio);
  expect(await keyOn(radio, { key: "Enter" })).toBe(true);
  expect(answered().map((a) => a.response)).toEqual([
    { accept: { values: { question_0: "Quash the complaint" } } },
  ]);
});

test("focus: the form takes it from the page, onto its first field; with two, the first question keeps it", async () => {
  h = await room(
    [askEntry(3, ID, TWO_QUESTIONS), askEntry(4, uuid(2, 9), ONE_QUESTION)],
    undefined,
    true,
  );
  expect(document.activeElement).toBe(el(T.choice("question_0", 0)));
  expect(h.all(T.form)[0]?.contains(document.activeElement)).toBe(true);
});

test("focus: the form takes it from the thread too", async () => {
  h = await mountRoom();
  h.deliver([reset([], header("needs_you"))]);
  const thread = h.host.querySelector<HTMLElement>('[role="log"]')!;
  thread.tabIndex = -1;
  await act(async () => thread.focus());
  expect(document.activeElement).toBe(thread);
  h.deliver([upsert(askEntry(3, ID, ONE_QUESTION))]);
  await h.settle();
  expect(document.activeElement).toBe(el(T.choice("question_0", 0)));
});

test("focus: never taken from the composer, nor from any other field", async () => {
  h = await mountRoom();
  const composer = el<HTMLTextAreaElement>("composer");
  await act(async () => composer.focus());
  h.deliver([reset([askEntry(3, ID, TWO_QUESTIONS)], header("needs_you"))]);
  await h.settle();
  expect(h.one(T.form)).not.toBeNull();
  expect(document.activeElement).toBe(composer);
  await h.unmount();
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  h = await mountRoom();
  await act(async () => elsewhere.focus());
  h.deliver([reset([askEntry(3, ID, ONE_QUESTION)], header("needs_you"))]);
  await h.settle();
  expect(h.one(T.form)).not.toBeNull();
  expect(document.activeElement).toBe(elsewhere);
});

test("a schema the form can't show: the agent's words, Decline and Stop, no fields and no Send", async () => {
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
    th("en", "This question can't be shown here. Decline, or Stop the answer."),
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

test("an IME's Enter (composing, or keyCode 229) and a held Enter send nothing", async () => {
  h = await ask(ONE_QUESTION);
  const radio = el(T.choice("question_0", 0));
  await h.click(radio);
  expect(await keyOn(radio, { key: "Enter", isComposing: true })).toBe(false);
  expect(await keyOn(radio, { key: "Enter", keyCode: 229 })).toBe(false);
  expect(await keyOn(radio, { key: "Enter", repeat: true })).toBe(false);
  const other = el<HTMLTextAreaElement>(T.input("question_0_custom"));
  expect(await keyOn(other, { key: "Enter", metaKey: true, keyCode: 229 })).toBe(false);
  expect(answered()).toEqual([]);
});

/** Puts words in a number field the input can't read: the browser reports "" with validity.badInput. */
async function fillBad(id: string) {
  const node = el(id);
  Object.defineProperty(node, "validity", { configurable: true, get: () => ({ badInput: true }) });
  await fill(id, "");
}

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

test("two forms keep their ids and their radios' names apart", async () => {
  h = await room([askEntry(3, ID, TWO_QUESTIONS), askEntry(4, uuid(2, 9), TWO_QUESTIONS)]);
  const ids = [...h.host.querySelectorAll("[id]")].map((n) => n.id);
  expect(new Set(ids).size).toBe(ids.length);
  const [a, b] = h.all(T.form);
  const names = (f: HTMLElement | undefined) =>
    new Set(
      [...(f?.querySelectorAll<HTMLInputElement>('input[type="radio"]') ?? [])].map((r) => r.name),
    );
  expect(names(a).size).toBe(1);
  expect([...names(a)].some((n) => names(b).has(n))).toBe(false);
  // every reference points at an element of its own form
  for (const f of [a, b])
    for (const n of f?.querySelectorAll("[aria-describedby]") ?? [])
      for (const ref of n.getAttribute("aria-describedby")!.split(" "))
        expect(f?.contains(document.getElementById(ref))).toBe(true);
});

test("first focus goes to the first field, never to a later question's chosen option", async () => {
  const schema = {
    type: "object",
    properties: {
      name: { type: "string", title: "Name" },
      court: { type: "string", enum: ["Saket", "Patiala House"], default: "Saket" },
    },
  };
  h = await room([askEntry(3, ID, { message: "Details", schema })], undefined, true);
  expect(document.activeElement).toBe(el(T.input("name")));
});
