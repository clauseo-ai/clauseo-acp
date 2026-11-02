// U4b's copy review entries (the agent's question as a form, Send the answer, Decline, Other, Required): apart from
// registry.tsx, as registry-u4.tsx is. Each string over the room's thread, in the state that shows it: the question the
// probe shows (elicitation-fixtures.ts), and two forms the probe doesn't have, one with a required field and one whose
// default breaks its range, so the problem lines can show. Send is pressed (RoomPressing) where a problem line shows.
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

/** A number field whose default is out of its range: "This answer doesn't fit the question." once Send is pressed. */
const OUT_OF_RANGE: AskForm = {
  message: "Clauseo needs the amount the notice claims.",
  schema: {
    type: "object",
    properties: {
      amount: {
        type: "number",
        title: "Amount, in rupees",
        minimum: 1,
        maximum: 100000,
        default: 500000,
      },
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
/** The out-of-range field after Send is pressed. */
const invalidSent = () => (
  <RoomPressing host={() => asking(OUT_OF_RANGE)} press="elicitation-send" />
);

/** The question whose form can't be drawn: its message, the line that says so, and Decline and Stop the answer. */
const unsupported = () => <RoomOver host={() => asking(UNSUPPORTED)} />;

/** U4b's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U4B: Entry[] = [
  {
    id: "U4B-01",
    words: threadWords("Send the answer"),
    screen: "room/thread",
    render: probe,
  },
  {
    id: "U4B-02",
    words: threadWords("Decline"),
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
    words: threadWords("Choose an answer to send, or Decline to go on without one."),
    screen: "room/thread",
    render: probe,
  },
  {
    id: "U4B-08",
    words: threadWords("This question can't be shown here. Decline, or Stop the answer."),
    screen: "room/thread",
    render: unsupported,
  },
];
