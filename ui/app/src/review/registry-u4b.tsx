// U4b's copy review entries (the agent's question as a form, Send, Don't answer, Other, Required): apart from
// registry.tsx, as registry-u4.tsx is. Each string over the room's thread, in the state that shows it: the question the
// probe shows (elicitation-fixtures.ts), and forms the probe doesn't have: one with a required field, one whose
// default breaks a format, and one whose defaults each break a limit the schema names, so the problem lines can show. Send is pressed (RoomPressing) where a problem line shows.
import type { ViewEntry } from "../transport/protocol.ts";
import {
  askEntry,
  ONE_QUESTION,
  TWO_QUESTIONS,
  type AskForm,
} from "../room/elicitation-fixtures.ts";
import { userMessage, uuid } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver, RoomPressing } from "./states.tsx";
import { threadWords } from "./words.ts";

/** The lawyer's question the forms sit under. */
const QUESTION: ViewEntry = userMessage(2, 1, "Draft the petition to quash.");

/** A required text field, empty: "Required" beside its title, and "Answer this question." once Send is pressed. */
const REQUIRED: AskForm = {
  message: "Clauseo needs the bank's reference before it can draft the letter.",
  schema: {
    type: "object",
    properties: {
      reference: {
        type: "string",
        title: "Bank's reference",
        description: "As it is on the bank's letter.",
      },
    },
    required: ["reference"],
  },
};

/** An email field whose default isn't an email, a limit no line names: "This answer doesn't fit the question." once
 * Send is pressed. */
const OUT_OF_FORMAT: AskForm = {
  message: "Clauseo needs the bank's email for the notice.",
  schema: {
    type: "object",
    properties: {
      email: {
        type: "string",
        format: "email",
        title: "Bank's email",
        default: "legal at the bank",
      },
    },
  },
};

/** Fields whose defaults each break a limit the schema gives, so each names it once Send is pressed. */
const OVER_LIMITS: AskForm = {
  message: "Clauseo needs a few details for the notice.",
  schema: {
    type: "object",
    properties: {
      ref: {
        type: "string",
        title: "Notice number",
        maxLength: 8,
        default: "NOTICE/2001/0457",
      },
      grounds: {
        type: "string",
        title: "Grounds, in short",
        minLength: 20,
        default: "Cheque",
      },
      initial: { type: "string", title: "Bank's initial", minLength: 1, default: " " },
      branch: { type: "string", title: "Branch code letter", maxLength: 1, default: "DL" },
      papers: {
        type: "array",
        title: "Papers to annex",
        maxItems: 1,
        items: { enum: ["Notice", "Reply"] },
        default: ["Notice", "Reply"],
      },
      witnesses: {
        type: "array",
        title: "Witnesses",
        minItems: 2,
        items: { enum: ["Branch manager", "Clerk"] },
        default: ["Clerk"],
      },
      amount: {
        type: "number",
        title: "Amount, in rupees",
        minimum: 1,
        maximum: 100000,
        default: 500000,
      },
      days: { type: "integer", title: "Days to reply", minimum: 15, default: 7 },
      interest: { type: "number", title: "Interest, per cent", maximum: 24, default: 36 },
      copies: { type: "integer", title: "Copies", default: 2.5 },
    },
  },
};

/** A required property Clauseo can't draw (an object inside the form): the form has no fields, and the line says so. */
const UNSUPPORTED: AskForm = {
  message: "Clauseo needs the bank's address for the letter.",
  schema: {
    type: "object",
    properties: {
      address: {
        type: "object",
        title: "Address",
        properties: { street: { type: "string" } },
      },
    },
    required: ["address"],
  },
};

/** The thread: the lawyer's question, then `form` pending, the lawyer waiting on it. */
const asking = (...forms: [AskForm, ...AskForm[]]) =>
  threadHost({
    entries: [QUESTION, ...forms.map((form, i) => askEntry(3 + i, uuid(1 + i, 9), form))],
    status: "needs_you",
  });

/** The probe's two forms, the first question's choice, Other beneath it, and a multi-select: nothing filled in yet. */
const probe = () => <RoomOver host={() => asking(TWO_QUESTIONS, ONE_QUESTION)} />;
/** The required field, nothing filled in: Required, and the hint under the buttons. */
const required = () => <RoomOver host={() => asking(REQUIRED)} />;
/** The required field after Send is pressed with nothing in it. */
const requiredSent = () => <RoomPressing host={() => asking(REQUIRED)} press="elicitation-send" />;
/** The out-of-format field after Send is pressed. */
const invalidSent = () => (
  <RoomPressing host={() => asking(OUT_OF_FORMAT)} press="elicitation-send" />
);
/** The fields over their limits after Send is pressed. */
const limitsSent = () => <RoomPressing host={() => asking(OVER_LIMITS)} press="elicitation-send" />;

/** The question whose form can't be drawn: its message, the line that says so, and Don't answer and Stop the answer. */
const unsupported = () => <RoomOver host={() => asking(UNSUPPORTED)} />;

/** U4b's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U4B: Entry[] = [
  {
    id: "U4B-01",
    words: threadWords("Send"),
    screen: "room/thread",
    render: probe,
  },
  {
    id: "U4B-02",
    words: threadWords("Don't answer"),
    screen: "room/thread",
    render: probe,
  },
  {
    id: "U4B-03",
    words: threadWords("Other"),
    screen: "room/thread",
    render: probe,
  },
  {
    id: "U4B-04",
    words: threadWords("Required"),
    screen: "room/thread",
    render: required,
  },
  {
    id: "U4B-05",
    words: threadWords("Answer this question."),
    screen: "room/thread",
    render: requiredSent,
  },
  {
    id: "U4B-06",
    words: threadWords("This answer doesn't fit the question."),
    screen: "room/thread",
    render: invalidSent,
  },
  {
    id: "U4B-07",
    words: threadWords("Choose an answer to send, or 'Don't answer' to go on without one."),
    screen: "room/thread",
    render: probe,
  },
  {
    id: "U4B-08",
    words: threadWords(
      "This question can't be shown here. Choose 'Don't answer', or Stop the answer.",
    ),
    screen: "room/thread",
    render: unsupported,
  },
  ...(
    [
      ["U4B-09", "Use {n} characters or fewer."],
      ["U4B-10", "Use 1 character or fewer."],
      ["U4B-11", "Use at least {n} characters."],
      ["U4B-12", "Use at least 1 character."],
      ["U4B-13", "Choose up to {n}."],
      ["U4B-14", "Choose at least {n}."],
      ["U4B-15", "Use a number from {min} to {max}."],
      ["U4B-16", "Use a number of {min} or more."],
      ["U4B-17", "Use a number of {max} or less."],
      ["U4B-18", "Use a whole number."],
    ] as const
  ).map(([id, key]): Entry => ({
    id,
    words: threadWords(key),
    screen: "room/thread",
    render: limitsSent,
  })),
];
