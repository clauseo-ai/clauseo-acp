// UC's entries for the composer's status line on real models (stage B's B2): the quiet line in the menus' place until
// the agent's models are in (UC-1293), or once the host couldn't read them (UC-1370); "Code mode on" at the foot of the
// open model menu (UC-1378; the per-answer tag, UC-1294 and UC-1295, retired). Each in the real room over
// registry-pk.tsx's fake host, the agents' models from pick-models.ts.
import type { ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { agentText, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type { ViewEntry } from "../../transport/protocol.ts";
import type { Entry } from "../entry.ts";
import { BOTH, CLAUDE, CODEX, ONGOING, renderPick } from "../registry-pk.tsx";
import { threadWords } from "../words.ts";

const screen = "room/composer-pickers";

/** What the latest turn ran with, as the header holds it after its start. */
const RAN = {
  model: { id: "opus-5-5", name: "Opus 5.5" },
  effort: { id: "medium", name: "Medium" },
};

const QUESTION = userMessage(
  2,
  1,
  "Go through the six supply agreements in this folder and list, for each, the notice period for termination for convenience, who may give notice, and whether notice by email counts.",
);
const PARTIAL = agentText(3, 1, "Reading the supply agreements: four of six so far.");
const ANSWER = agentText(
  3,
  1,
  "Five of the six agreements allow termination for convenience. The notice periods run from 30 to 90 days; only the Mehra Foods agreement accepts notice by email (clause 14.2).",
);

/** An answer running on Opus 5.5 at Medium. */
const RUNNING = {
  events: BOTH,
  threads: ONGOING,
  head: { ...RAN, status: "running" as const },
  entries: [QUESTION, PARTIAL],
};

/** The same answer done, in code mode, as its step says. */
const ENDED = {
  events: BOTH,
  threads: ONGOING,
  head: RAN,
  entries: [QUESTION, ANSWER, step(4, { code_mode: true })],
};

/** An answer whose model pick Claude couldn't use: the host's notice before its turn, naming what it ran with. */
const notUsed = (model?: string, effort?: string): ViewEntry => ({
  notice: {
    order: 3,
    event: uuid(3, 7),
    level: "info",
    text: "Clauseo couldn't switch to the model or effort you picked for this answer.",
    kind: {
      pick_not_used: {
        ...(model === undefined ? {} : { model }),
        ...(effort === undefined ? {} : { effort }),
      },
    },
    ts: 3000,
  },
});

const PICK_FORMS: [string, ThreadKey, (string | undefined)?, (string | undefined)?][] = [
  [
    "UC-1371",
    "{name} couldn't use the model or effort you picked for this answer, so it used {model} at {effort} effort.",
    "Opus 5.5",
    "Medium",
  ],
  [
    "UC-1373",
    "{name} couldn't use the model or effort you picked for this answer, so it used {model}.",
    "Opus 5.5",
  ],
  [
    "UC-1374",
    "{name} couldn't use the model or effort you picked for this answer, so it used {effort} effort.",
    undefined,
    "Medium",
  ],
  [
    "UC-1375",
    "{name} couldn't use the model or effort you picked for this answer, so it kept the settings it had.",
  ],
];

/** Each form of the notice, a quiet row at the top of the answer. */
const PICK_NOT_USED: Entry[] = PICK_FORMS.map(([id, key, model, effort]) => ({
  id,
  words: threadWords(key),
  screen: "room/thread",
  render: renderPick({
    events: BOTH,
    threads: ONGOING,
    head: RAN,
    entries: [QUESTION, notUsed(model, effort), ANSWER, step(4)],
  }),
}));

const UC_COMPOSER_MENUS: Entry[] = [
  // a new conversation before the host's probe has filled either agent's models
  {
    id: "UC-1293",
    words: threadWords("Loading models…"),
    screen,
    render: renderPick({ events: BOTH, models: { [CLAUDE]: null, [CODEX]: null } }),
  },
  // the same, once the host's probe of Claude's models failed: Ask still works
  {
    id: "UC-1370",
    words: threadWords("Couldn't read the models yet. You can still ask."),
    screen,
    render: renderPick({
      events: BOTH,
      models: { [CLAUDE]: null, [CODEX]: null },
      unread: [CLAUDE],
    }),
  },
  // the same where the firm requires the sandbox: no probe runs, the models come with the first session
  {
    id: "UC-1372",
    words: threadWords("Models show after your first message."),
    screen,
    render: renderPick({ events: BOTH, models: { [CLAUDE]: null, [CODEX]: null }, required: true }),
  },
  ...PICK_NOT_USED,
  // an answer running on Opus 5.5 at Medium, Low picked for the next message: the menus name the record, the pick after
  {
    id: "UC-1376",
    words: threadWords("Next message: {settings}"),
    screen,
    render: renderPick({
      ...RUNNING,
      head: { ...RAN, status: "running" as const },
      steps: [
        { press: '[data-testid="pick-effort"]' },
        { press: '[role="menuitemradio"]:first-child' },
      ],
    }),
  },
  // a Claude conversation whose last answer ran in code mode, its model menu open: the quiet line at the menu's foot
  {
    id: "UC-1378",
    words: threadWords("Code mode on"),
    screen,
    render: renderPick({ ...ENDED, steps: [{ press: '[data-testid="pick-model"]' }] }),
  },
];

export default UC_COMPOSER_MENUS;
