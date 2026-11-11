// ?pickers=default (probe-main.tsx): both plans set up and signed in, each agent's model and effort at its own defaults
// as it reports them, for the composer's pickers. Claude's are Default rows: the model's names what it stands for in
// its description (claude-agent-acp's session-model.js), the effort's names nothing (Claude Code settles it). ChatGPT's
// values are all concrete, its defaults among them (codex-acp's ModelConfigOption.ts). The names are fake. Not shipped.
import type { ConfigOption, SetupEvent } from "../transport/protocol.ts";
import type { AgentSummary } from "./host-next.ts";

const select = (
  id: string,
  category: string,
  current: string,
  values: [string, string, string?][],
): ConfigOption =>
  ({
    id,
    name: id,
    category,
    kind: {
      select: {
        current_value: current,
        options: values.map(([value, name, description]) => ({
          value,
          name,
          ...(description === undefined ? {} : { description }),
        })),
      },
    },
  }) as ConfigOption;

const CLAUDE: ConfigOption[] = [
  select("model", "model", "default", [
    ["default", "Default (recommended)", "Sonnet"],
    ["opus", "Opus", "For the hardest questions"],
    ["sonnet", "Sonnet", "For everyday work"],
    ["haiku", "Haiku", "For quick answers"],
  ]),
  select("effort", "thought_level", "default", [
    ["default", "Default"],
    ["low", "Low"],
    ["medium", "Medium"],
    ["high", "High"],
  ]),
];

const CHATGPT: ConfigOption[] = [
  select("model", "model", "frontier", [
    ["frontier", "Frontier", "For the hardest questions"],
    ["mini", "Frontier mini", "For quick answers"],
  ]),
  select("reasoning_effort", "thought_level", "medium", [
    ["low", "Low"],
    ["medium", "Medium"],
    ["high", "High"],
  ]),
];

const signed = (agent: string): SetupEvent => ({
  sign_in_changed: {
    agent,
    state: {
      signed_in: { email: "lawyer@example.com", method: "plan", source: "own", at: 1 },
    },
  },
});

/** The page's setup and agents for ?pickers=default; null otherwise. */
export function pickersProbe(
  name: string | null,
): { setup: { first: () => SetupEvent[] }; agents: AgentSummary[] } | null {
  if (name !== "default") return null;
  return {
    setup: {
      first: () => [
        { agent_install_progress: { agent: "claude-acp", state: { ready: { version: "0.1.0" } } } },
        { agent_install_progress: { agent: "codex-acp", state: { ready: { version: "0.1.0" } } } },
        signed("claude-acp"),
        signed("codex-acp"),
      ],
    },
    agents: [
      { id: "claude-acp", kind: "claude", config_options: CLAUDE },
      { id: "codex-acp", kind: "codex", config_options: CHATGPT },
    ],
  };
}
