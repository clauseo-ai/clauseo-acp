// Claude's AskUserQuestion as claude-agent-acp 0.89.1 sends it (dist/elicitation.js, askUserQuestionsToCreateRequest),
// byte for byte as it reaches the host's ElicitationRequest: one question (its words in the message, no field
// description), and two (a single choice and a multi-select, each question's words in its field's description), each
// question followed by its `_custom` Other. Nothing is required. For the tests and the probe page only.
import type { ElicitationEntry, RawPayload, ViewEntry } from "../transport/protocol.ts";

/** A form elicitation's request. */
export type AskForm = { message: string; schema: RawPayload };

const SINGLE_OTHER =
  "Type your own answer, or add a note to the option you chose above (optional).";
const MULTI_OTHER = "Type your own answer to add to your selection above (optional).";

/** One question: "Which court should the petition be filed in?" */
export const ONE_QUESTION: AskForm = {
  message: "Which court should the petition be filed in?",
  schema: {
    type: "object",
    properties: {
      question_0: {
        type: "string",
        title: "Court",
        oneOf: [
          {
            const: "Delhi High Court",
            title: "Delhi High Court",
            description: "The bank's registered office is in New Delhi.",
          },
          {
            const: "Sessions Court, Saket",
            title: "Sessions Court, Saket",
            description: "Where the complaint under section 138 is pending.",
          },
          { const: "Supreme Court", title: "Supreme Court" },
        ],
      },
      question_0_custom: { type: "string", title: "Other", description: SINGLE_OTHER },
    },
  },
};

/** Two questions: a single choice, then a multi-select. */
export const TWO_QUESTIONS: AskForm = {
  message: "Please answer the following questions.",
  schema: {
    type: "object",
    properties: {
      question_0: {
        type: "string",
        title: "Relief",
        description: "Which relief should the petition ask for first?",
        oneOf: [
          {
            const: "Quash the complaint",
            title: "Quash the complaint",
            description: "Under section 482, on the ground of limitation.",
          },
          {
            const: "Stay the proceedings",
            title: "Stay the proceedings",
            description: "Until the civil suit is decided.",
          },
        ],
      },
      question_0_custom: { type: "string", title: "Other", description: SINGLE_OTHER },
      question_1: {
        type: "array",
        title: "Annexures",
        description: "Which papers should be annexed?",
        items: {
          anyOf: [
            {
              const: "Notice to the bank",
              title: "Notice to the bank",
              description: "Sent on 12 March 2001.",
            },
            { const: "Reply to the notice", title: "Reply to the notice" },
            {
              const: "Bank statement, March 2001",
              title: "Bank statement, March 2001",
              description: "Shows the cheque's return.",
            },
          ],
        },
      },
      question_1_custom: { type: "string", title: "Other", description: MULTI_OTHER },
    },
  },
};

/** A pending form elicitation in a thread. */
export const askEntry = (
  order: number,
  elicitation: string,
  form: AskForm,
  state: ElicitationEntry["state"] = "pending",
): ViewEntry => ({
  elicitation: {
    order,
    elicitation,
    request: { form: { message: form.message, schema: form.schema } },
    state,
  },
});
