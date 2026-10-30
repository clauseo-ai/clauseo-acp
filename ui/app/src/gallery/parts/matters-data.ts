// The matters' sample data for the gallery: matters, conversations, queued messages and the plan's windows. Invented.
// The names are as a lawyer might file them; the people are "A. Rao"-style; no court or judgment here is real.
import type { PlanWindow } from "@clauseo/components/clauseo/plan-card.tsx";
import type { ConversationRowProps } from "@clauseo/components/clauseo/rail-conversations.tsx";
import type { QueuedMessage } from "@clauseo/components/clauseo/queue-model.ts";
import type { MatterRowProps } from "@clauseo/components/clauseo/rail-matter.tsx";

const none = () => {};

/** The folder Clauseo makes for a new matter, as the form shows it. */
export const folderFor = (name: string) => ["Documents", "Clauseo", name].join("/");

export const ACME: MatterRowProps = {
  name: "Acme Ltd v. State",
  note: "Recovery suit, for the plaintiff",
  hint: "Acme Ltd v. State",
};

export const BLUEBELL: MatterRowProps = {
  name: "Bluebell Mills Ltd v. Union of India",
  note: "Customs writ",
  hint: "Bluebell Mills Ltd v. Union of India",
};

export const TRUST: MatterRowProps = {
  name: "Rao Family Trust",
  note: "Trust deed",
  hint: "Rao Family Trust",
};

/** The current matter's conversations: one raised, one with its other plan to continue with. */
export const CONVERSATIONS: readonly ConversationRowProps[] = [
  {
    id: "reply",
    title: "Reply to the notice",
    agent: "Claude",
    current: true,
    other: { agent: "ChatGPT", onContinue: none },
  },
  { id: "plaint", title: "Draft plaint", agent: "Claude", state: "question" },
  { id: "limits", title: "Limitation", agent: "ChatGPT", state: "working", at: "4:41 pm" },
  { id: "annex", title: "Annexures", agent: "Claude", state: "waiting", behind: "Draft plaint" },
  { id: "facts", title: "Summary of facts", agent: "Claude", state: "ready" },
  { id: "interest", title: "Interest claim", agent: "Claude", state: "stopped", at: "4:20 pm" },
];

export const QUEUED: readonly QueuedMessage[] = [
  { id: "q1", text: "Check annexure 4 against the notice." },
  { id: "q2", text: "Add the bank's reply to the bundle." },
  { id: "q3", text: "Shorten the limitation paragraph." },
];

/** Ten messages, the queue's cap. */
export const FULL_QUEUE: readonly QueuedMessage[] = Array.from({ length: 10 }, (_, i) => ({
  id: `f${i + 1}`,
  text: `Check annexure ${i + 1} against the notice.`,
}));

/** The plan's two windows, with the share left and the reset in words. */
export const WINDOWS: readonly PlanWindow[] = [
  {
    name: "5-hour limit",
    left: 18,
    leftText: "18% left",
    reset: "Resets 6:10 pm",
    valueText: "18% left",
  },
  {
    name: "Weekly limit",
    left: 30,
    leftText: "30% left",
    reset: "Resets Tuesday 9:00 am",
    valueText: "30% left",
  },
];
