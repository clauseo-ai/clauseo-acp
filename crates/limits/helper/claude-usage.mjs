// claude-usage.mjs: Clauseo's plan-limits probe for claude-agent-acp.
//
// Who runs it: the Clauseo Rust side, as `node <adapter install>/clauseo-claude-usage.mjs`, with no
// arguments, in an empty working folder and the agent's environment. It reads exactly one stdout line
// and kills the process tree after 25 s. The bare SDK import resolves to the adapter's own SDK
// (0.3.293), which sits next to this file.
//
// Line contract (stdout: exactly one line; exit code 0 in every case):
//   {"ok":true,"account":{...}|null,"usage":{"subscription_type","rate_limits_available","rate_limits"}}
//   {"ok":false,"stage":"init"|"usage","missing":<bool>,"message":"..."}
// Full errors go to stderr only.
//
// Credentials: this script reads no credential. The SDK starts its own Claude Code process, and that
// process reads the login, as it does for the adapter itself.

// G16's first real run confirms this regex. A usage error counts as "missing" when its message says the
// CLI does not know the control request, which is what an older CLI answers. A plain "unknown error"
// is a failed read, not a missing method.
const USAGE_UNSUPPORTED =
  /unsupported|not supported|not implemented|unknown (control )?(request|subtype|method)|no such method/i;

const USAGE_METHOD = "usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET";
const ACCOUNT_KEYS = ["email", "subscriptionType", "apiProvider"];
const USAGE_KEYS = ["subscription_type", "rate_limits_available", "rate_limits"];
const MESSAGE_MAX = 200;

const abort = new AbortController();
let query = null;
let printed = false;

// A rejection from the SDK's own promises must not kill the process before the line is out.
process.on("unhandledRejection", (err) => console.error(err));

// The prompt never yields: the query stays open until we abort it.
async function* idlePrompt() {
  await new Promise((resolve) => abort.signal.addEventListener("abort", resolve, { once: true }));
}

function messageOf(err) {
  try {
    return err instanceof Error || typeof err?.message === "string" ? err.message : String(err);
  } catch {
    return "unprintable error";
  }
}

function failure(stage, missing, detail) {
  // Cut by code point, so no lone surrogate reaches the line (Rust's JSON reader refuses one).
  const message = Array.from(messageOf(detail).replace(/\p{Cc}/gu, " "))
    .slice(0, MESSAGE_MAX)
    .join("");
  return JSON.stringify({ ok: false, stage, missing, message });
}

function pickAccount(init) {
  const account = init?.account;
  if (!account || typeof account !== "object") return null;
  const out = {};
  for (const key of ACCOUNT_KEYS) if (typeof account[key] === "string") out[key] = account[key];
  return out;
}

function pickUsage(usage) {
  const out = {};
  if (usage && typeof usage === "object") {
    for (const key of USAGE_KEYS) if (Object.hasOwn(usage, key)) out[key] = usage[key];
  }
  return out;
}

// Prints the one line, then aborts and closes the query. Exits once stdout has flushed.
function finish(line) {
  if (printed) return;
  printed = true;
  process.stdout.write(line + "\n", () => process.exit(0));
  abort.abort();
  try {
    Promise.resolve(query?.close?.()).catch(() => {});
  } catch {
    // closing is best effort
  }
}

async function main() {
  let sdk;
  try {
    sdk = await import("@anthropic-ai/claude-agent-sdk");
  } catch (err) {
    console.error(err);
    return finish(failure("init", false, err));
  }

  const options = {
    abortController: abort,
    persistSession: false,
    settingSources: [],
    allowedTools: [],
    mcpServers: {},
    strictMcpConfig: true,
    hooks: {},
    settings: { disableAllHooks: true },
    cwd: process.cwd(),
  };
  if (process.env.CLAUDE_CODE_EXECUTABLE) {
    options.pathToClaudeCodeExecutable = process.env.CLAUDE_CODE_EXECUTABLE;
  }
  query = sdk.query({ prompt: idlePrompt(), options });

  if (typeof query.initializationResult !== "function") {
    return finish(failure("init", true, "initializationResult method missing"));
  }
  let init;
  try {
    init = await query.initializationResult();
  } catch (err) {
    console.error(err);
    return finish(failure("init", false, err));
  }

  if (typeof query[USAGE_METHOD] !== "function") {
    return finish(failure("usage", true, "usage_EXPERIMENTAL method missing"));
  }
  let usage;
  try {
    usage = await query[USAGE_METHOD]({ skipBehaviors: true });
  } catch (err) {
    console.error(err);
    return finish(failure("usage", USAGE_UNSUPPORTED.test(messageOf(err)), err));
  }

  // A lone surrogate would print as an escape that Rust's JSON reader refuses, losing the read.
  const line = { ok: true, account: pickAccount(init), usage: pickUsage(usage) };
  return finish(JSON.stringify(line, (_key, value) => (typeof value === "string" ? value.toWellFormed() : value)));
}

main().catch((err) => {
  console.error(err);
  finish(failure("init", false, err));
});
