// @vitest-environment happy-dom
// The agent's question form's focus, keys and groups (elicitation-form.tsx), over FakeTransport: where focus goes as the
// form appears or goes, Enter, Escape, an IME's and a held key, the fieldset and legend that name each question, and
// two forms' ids. Answering, sending and refusals are in elicitation-form.test.tsx, over the same rig.
import { act } from "react";
import { expect, test } from "vitest";
import { header, reset, upsert, uuid } from "../testing/stream-script.ts";
import { askEntry, ONE_QUESTION, TWO_QUESTIONS } from "./elicitation-fixtures.ts";
import { ELICITATION_TEST_IDS as T } from "./elicitation-schema.ts";
import { ask, ID, room, rig } from "./elicitation-form-rig.ts";
import { mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
const { el, answered, fill, keyOn } = rig(
  () => h,
  (next) => {
    h = next;
  },
);

test("focus left outside the form stays where it is as the form goes", async () => {
  h = await ask(ONE_QUESTION);
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  await act(async () => elsewhere.focus());
  h.deliver([upsert(askEntry(3, ID, ONE_QUESTION, { expired: { reason: "turn_ended" } }))]);
  await h.settle();
  expect(document.activeElement).toBe(elsewhere);
});

test("Escape outside a text field is Don't answer; in one it stays the field's", async () => {
  h = await ask(ONE_QUESTION);
  expect(await keyOn(el(T.input("question_0_custom")), { key: "Escape" })).toBe(false);
  expect(answered()).toEqual([]);
  // with Alt, neither Escape nor Enter answers, as in the permission prompt
  await h.click(el(T.choice("question_0", 0)));
  await keyOn(el(T.choice("question_0", 0)), { key: "Escape", altKey: true });
  await keyOn(el(T.choice("question_0", 0)), { key: "Enter", altKey: true });
  expect(answered()).toEqual([]);
  expect(await keyOn(el(T.choice("question_0", 0)), { key: "Escape" })).toBe(true);
  expect(answered().map((a) => a.response)).toEqual(["decline"]);
  // busy: a second Escape sends nothing
  await keyOn(el(T.stop), { key: "Escape" });
  expect(answered()).toHaveLength(1);
});

test("a question with no title is named by its description, else by the agent's message", async () => {
  const schema = {
    type: "object",
    properties: {
      court: { type: "string", description: "Which court?", enum: ["Saket", "Tis Hazari"] },
      bench: { type: "string", enum: ["Single", "Division"] },
    },
    required: ["bench"],
  };
  h = await ask({ message: "Where and before whom?", schema });
  const court = el<HTMLFieldSetElement>(T.field("court"));
  expect(court.querySelector("legend")?.textContent).toBe("Which court?");
  // said once, as the name
  expect(court.getAttribute("aria-describedby")).toBeNull();
  const bench = el<HTMLFieldSetElement>(T.field("bench"));
  expect(bench.textContent).not.toContain("bench");
  const msg = el<HTMLElement>(T.form).getAttribute("aria-describedby")!;
  expect(bench.getAttribute("aria-labelledby")).toBe(msg);
  const radiogroup = bench.querySelector<HTMLElement>('[role="radiogroup"]')!;
  expect(radiogroup.getAttribute("aria-labelledby")).toBe(msg);
  expect(radiogroup.getAttribute("aria-required")).toBe("true");
});

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
