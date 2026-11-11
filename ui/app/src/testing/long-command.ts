// A finished turn with one long command line, wider than the thread: the agent ran a pandoc command with long paths
// (its row, the command folded, and what came back), then answered with the same command in a fenced shell block. The
// probe's ?thread=long-command shows it (probe-main.tsx), for screenshots of a block's Copy beside a line that
// scrolls sideways. Fake paths and words only. Not shipped.
import type { ViewEntry } from "../transport/protocol.ts";
import { agentText, step, toolCall, userMessage } from "./stream-script.ts";

/** The command, one line, longer than any block is wide. */
export const LONG_COMMAND =
  'pandoc "/Users/lawyer/Documents/Clauseo/Sharma v Sunrise Builders/papers/Reply to the legal notice (final, 14 March).docx" --from=docx --to=markdown --wrap=none --extract-media="/Users/lawyer/Documents/Clauseo/Sharma v Sunrise Builders/papers/media" -o "/Users/lawyer/Documents/Clauseo/Sharma v Sunrise Builders/papers/Reply to the legal notice.md"';

/** The question, the command's row with its output, the answer quoting the command, and the turn's step. */
export const longCommandEntries = (): ViewEntry[] => [
  userMessage(2, 1, "Turn the reply to the legal notice into plain text I can paste."),
  {
    tool_call: {
      ...toolCall(3, 201, "completed").tool_call!,
      kind: "execute",
      title: "",
      raw_input: { inline: JSON.stringify({ command: ["bash", "-lc", LONG_COMMAND] }) },
      raw_output: { inline: "Converted 1 file: Reply to the legal notice.md (4,812 words)" },
    },
  },
  agentText(
    4,
    100,
    `I converted the reply with this command:\n\n\`\`\`bash\n${LONG_COMMAND}\n\`\`\`\n\nThe text is in **Reply to the legal notice.md**, beside the original.`,
  ),
  step(5),
];
