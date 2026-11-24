// The agents' models for the pickers' review cells, as the host's probe fills AgentSummary.models: Claude's twelve
// (claude-agent-acp's session/options, each model's efforts from its AIR metadata) and ChatGPT's ten (codex's
// model/list, named as codex-acp names them). The names and descriptions are the agents' own, never Clauseo's words,
// so they are made up here in the agents' style.
import type { AgentModels, ModelEntry, NamedChoice } from "../transport/protocol.ts";

const level = (id: string, name: string): NamedChoice => ({ id, name });
const LOW = level("low", "Low");
const MEDIUM = level("medium", "Medium");
const HIGH = level("high", "High");
const XHIGH = level("x_high", "Xhigh");
const MAX = level("max", "Max");

const model = (
  id: string,
  name: string,
  efforts: NamedChoice[],
  description?: string,
  default_effort = efforts.length === 0 ? undefined : "medium",
): ModelEntry => ({
  id,
  name,
  efforts,
  ...(description === undefined ? {} : { description }),
  ...(default_effort === undefined ? {} : { default_effort }),
});

const FULL = [LOW, MEDIUM, HIGH, XHIGH, MAX];
const THREE = [LOW, MEDIUM, HIGH];

/** Claude's: the latest first, a description where the adapter gives one; the small models take no effort. */
export const CLAUDE_MODELS: AgentModels = {
  model_option: "model",
  effort_option: "effort",
  current_model: "opus-5-5",
  current_effort: "medium",
  models: [
    model("opus-5-5", "Opus 5.5", FULL, "Most capable for complex work"),
    model("sonnet-5-5", "Sonnet 5.5", THREE, "Best for everyday tasks"),
    model("haiku-5-5", "Haiku 5.5", [], "Fastest for quick answers"),
    model("fable-5-2", "Fable 5.2", THREE),
    model("opus-5-1", "Opus 5.1", FULL),
    model("sonnet-5-1", "Sonnet 5.1", THREE),
    model("haiku-5-1", "Haiku 5.1", []),
    model("opus-5", "Opus 5", THREE),
    model("sonnet-5", "Sonnet 5", THREE),
    model("haiku-5", "Haiku 5", []),
    model("opus-4-7", "Opus 4.7", THREE),
    model("sonnet-4-6", "Sonnet 4.6", [LOW, MEDIUM]),
  ],
};

const XH = level("xhigh", "Xhigh");
const GPT = [LOW, MEDIUM, HIGH, XH];

/** ChatGPT's: gpt-6.2-sol the default, each model its own efforts and default effort. */
export const CODEX_MODELS: AgentModels = {
  model_option: "model",
  effort_option: "reasoning_effort",
  current_model: "gpt-6.2-sol",
  current_effort: "medium",
  models: [
    model("gpt-6.2-sol", "6.2 Sol", GPT, "Frontier model for long, careful work"),
    model("gpt-6.2-astra", "6.2 Astra", GPT, "Faster frontier model"),
    model("gpt-6.1-terra", "6.1 Terra", THREE),
    model("gpt-6.1-sol", "6.1 Sol", GPT),
    model("gpt-6-astra", "6 Astra", THREE),
    model("gpt-6-sol", "6 Sol", THREE),
    model("gpt-6-luna", "6 Luna", [LOW, MEDIUM], "Small and quick", "low"),
    model("gpt-5.6-sol", "5.6 Sol", THREE),
    model("gpt-5.6-terra", "5.6 Terra", THREE),
    model("gpt-5.6-luna", "5.6 Luna", [LOW, MEDIUM], undefined, "low"),
  ],
};
