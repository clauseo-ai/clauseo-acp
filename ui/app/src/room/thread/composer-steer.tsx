// The composer's line above the field while Clauseo works and the field holds words (U7, matters/steer-claude and
// steer-chatgpt): what Send now does with this agent, then Send now and Queue. Send now gives the words to the running
// answer where the agent takes them (H9's `steer_joins_turn`, claude-agent-acp's), and on Codex stops the answer and
// sends them first; on an agent whose steering isn't known the hint says nothing and its buttons still work. Queue sends
// the words as Enter does, and is disabled once the queue holds QUEUE_CAP. Each read is a boolean or a number, so a
// streamed chunk or a keystroke renders nothing here.
import { SteerHint } from "@clauseo/components/clauseo/steer-hint.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { StreamId } from "../../transport/protocol.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { pickFull } from "./composer-queue.ts";

const TEST_IDS = { row: "steer-hint", sendNow: "steer-send-now", queue: "queue" } as const;

/** H9's steering flag on the thread's header: true, false, or undefined before the host says. */
const pickJoins = (v: StreamView | undefined): boolean | undefined =>
  v?.header?.header?.steer_joins_turn;

export function ComposerSteer({
  stream,
  onSendNow,
  onQueue,
}: {
  stream: StreamId | null;
  /** Send now: the field's words with `delivery: "steer"`. */
  onSendNow: () => void;
  /** Queue: the field's words, as Enter queues them. */
  onQueue: () => void;
}) {
  const { lang } = useWords();
  const store = useViewStore();
  const joins = useStreamPart(store, stream, pickJoins);
  const full = useStreamPart(store, stream, pickFull);
  const codex = useAgentPlan()?.kind === "codex";
  const text =
    joins === true
      ? mt(lang, "Send now gives your words to Clauseo while it writes.")
      : joins === false && codex
        ? mt(
            lang,
            "ChatGPT can't take words mid-answer. Send now stops this answer, keeps what it wrote, and sends your words first.",
          )
        : "";
  return (
    <SteerHint text={text} onSendNow={onSendNow} onQueue={onQueue} full={full} testIds={TEST_IDS} />
  );
}
