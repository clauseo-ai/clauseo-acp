# Reader fixtures

Each file under `claude/` is one line the Claude usage helper (`helper/claude-usage.mjs`) could
print: JSON, one line, ending in `\n`. The unit tests in `src/readers/claude/tests.rs` load each
one with `include_str!` and check the exact `AccountLimits` or outcome. Emails use
`example.com` only.

All are synthetic for now, written from the types of `@anthropic-ai/claude-agent-sdk` 0.3.293's
`sdk.d.ts` (`SDKControlGetUsageResponse`, lines 4266-4477; `AccountInfo`, lines 20-33) and the
helper's line contract (the comment at the top of `helper/claude-usage.mjs`). G16 replaces them
with recordings from real accounts and marks each row `recorded`.

| Fixture | Covers | Source | Kind |
|---|---|---|---|
| `claude/max.json` | Max plan: `five_hour`, `seven_day`, one `model_scoped` entry (id `seven_day_overage_included`), `extra_usage` enabled, null fixed windows ignored, an RFC 3339 time with a fraction and `+00:00` | `SDKControlGetUsageResponse.rate_limits` (sdk.d.ts 4266-4477), `AccountInfo` (20-33) | synthetic |
| `claude/pro.json` | Pro plan: an offset reset time, a null `resets_at`, `utilization` at 100, `seven_day_sonnet`, `extra_usage` off | as above | synthetic |
| `claude/team.json` | Team plan with `account: null` (ref `plan`), all fixed windows, a window with `utilization: null` left out, two `model_scoped` entries (ids `model_scoped:0`, `model_scoped:1`), the sort (session, weekly, model, other), `extra_usage: null` | as above | synthetic |
| `claude/api-key.json` | API key, pay as you go via Anthropic (`apiProvider: firstParty`), ref `api-key` | `subscription_type` and `rate_limits_available` docs (sdk.d.ts 4266-4477), `AccountInfo.apiProvider` | synthetic |
| `claude/bedrock.json` | Bedrock, pay as you go, ref `bedrock` | as above | synthetic |
| `claude/vertex.json` | Vertex, pay as you go, ref `vertex` | as above | synthetic |
| `claude/missing-scope.json` | A subscription with `rate_limits_available: false`: `Unavailable { MissingScope }`, no windows | `rate_limits_available` doc ("missing profile scope") | synthetic |
| `claude/method-missing.json` | `ok:false`, `stage: usage`, `missing: true`: `MethodMissing` | helper line contract | synthetic |
| `claude/init-failed.json` | `ok:false`, `stage: init`, `missing: false`: `Exited { code: Some(0), stderr_tail }`; the line's own `message` is never used | helper line contract | synthetic |

The Codex rows are added with the Codex reader (after R14).

## Checks for G16's recordings

- **`model_scoped` ids.** With exactly one `model_scoped` entry the reader names its window
  `seven_day_overage_included`, where the live `_claude/rateLimit` signal of that
  `rateLimitType` lands. With two or more, the ids are `model_scoped:<n>` (the array index), and
  a live `seven_day_overage_included` signal has no row and is dropped. G16 checks, on a real
  account, which `rateLimitType` the live signal sends for a per-model window, and whether one
  entry is what real accounts get.
- **`USAGE_UNSUPPORTED`.** The helper's regex for an older CLI's answer (`missing: true`) is a
  guess until a real old CLI answers.
