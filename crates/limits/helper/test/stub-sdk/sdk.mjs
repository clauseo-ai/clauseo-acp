// Stub of @anthropic-ai/claude-agent-sdk for the claude-usage.mjs tests. Never the real SDK.
// STUB_MODE picks the behaviour (see the cases below). Env outputs, when set:
//   STUB_OPTIONS_OUT: JSON {"prompt": <type>, "options": <the options query() got>}
//   STUB_CALLS_OUT:   JSON array of {"method", "args"} for each method call, in order.
import { writeFileSync } from "node:fs";

const mode = process.env.STUB_MODE ?? "ok";

// Only when the helper imports it: node --test also loads this file, since it sits under test/.
const imported = process.argv[1]?.endsWith("clauseo-claude-usage.mjs") ?? false;
if (imported && mode === "import-fails") throw new Error("stub import failure");

const initResult = {
  account: {
    email: "pat@example.com",
    subscriptionType: "max",
    apiProvider: "firstParty",
    organization: "Example LLP",
    tokenSource: "claude.ai",
  },
  commands: [],
  models: [],
};

const usageResult = {
  subscription_type: "max",
  rate_limits_available: true,
  rate_limits: {
    five_hour: { utilization: 42, resets_at: "2026-11-02T18:00:00Z" },
    seven_day: { utilization: 13, resets_at: "2026-11-07T00:00:00Z" },
  },
  session: { total_cost_usd: 0.5 },
  behaviors: { transcripts_scanned: 3 },
};

// Functions and the AbortController are written as strings, so the options survive JSON.
function encode(key, value) {
  if (typeof value === "function") return "<function>";
  if (value instanceof AbortController) return "<AbortController>";
  return value;
}

export function query({ prompt, options }) {
  const calls = [];
  const record = (method, args) => {
    calls.push({ method, args });
    if (process.env.STUB_CALLS_OUT) {
      writeFileSync(process.env.STUB_CALLS_OUT, JSON.stringify(calls));
    }
  };

  if (process.env.STUB_OPTIONS_OUT) {
    const promptType =
      prompt && typeof prompt[Symbol.asyncIterator] === "function" ? "<async-iterable>" : typeof prompt;
    writeFileSync(
      process.env.STUB_OPTIONS_OUT,
      JSON.stringify({ prompt: promptType, options }, encode),
    );
  }

  const q = {
    close() {
      record("close", []);
    },
  };

  if (mode !== "no-init") {
    q.initializationResult = async () => {
      record("initializationResult", []);
      if (mode === "init-throws") throw new Error("not logged in\nline two");
      if (mode === "lone-surrogate") {
        return { ...initResult, account: { ...initResult.account, email: "pat\uD800@example.com" } };
      }
      return initResult;
    };
  }

  if (mode !== "no-usage") {
    q.usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET = async (opts) => {
      record("usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET", [opts]);
      if (mode === "usage-unknown") {
        throw new Error("Unsupported control request subtype: get_usage");
      }
      if (mode === "usage-throws") throw new Error("x".repeat(500));
      if (mode === "lone-surrogate") {
        const scoped = [{ display_name: "Fa\uD800ble", utilization: 5, resets_at: null }];
        return { ...usageResult, rate_limits: { ...usageResult.rate_limits, model_scoped: scoped } };
      }
      return usageResult;
    };
  }

  return q;
}
