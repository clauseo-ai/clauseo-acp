# 0064: A case-law step's cost as a logged event

## Context

Rohan decided (2 Nov) that nothing asks the lawyer before a paid case-law call. Instead, each case-law step's row says what that call cost. The Indian Kanoon server reports the cost on every result of `search_law`, `read_document`, `citator`, `check_citations` and `account` for a known account, as `result._meta["chat.clauseo/credits"] = {charged_credits, balance_credits}` on the `CallToolResult`. `balance_credits` is left out when the server doesn't know it. Unauthenticated and protocol errors carry no report. claude-agent-acp drops `_meta` before the ACP client sees it, but our relay reads every result for both agents. So the cost is read there, and only there. The relay's result and the agent's tool call meet only in the host, through the sources join (SHA-256 of the tool and its canonical arguments, `sources/case_law.rs`). A row must show the same thing on a replay, after a restart and in another window.

## Decision

- **A new thread kind, `tool_call_charged { turn, tool_call, credits, refused }`** (host). It is logged when a matched case-law call completes or fails. It is logged only when the call was charged (`credits` > 0), or when its result was an error that charged 0 (`refused`).
  - A free success, a result with no report (an older server) or a malformed one, and a call the host can't match log nothing. The row then shows its plain words, never a zero or a guess.
  - The numbers are the server's own, taken from that call's result. Nothing is worked out from the price list.
- **Matching:** relay results for one key are taken by completed calls in arrival order. When results with one key wait at the same time with different charges, neither call gets one.
- **The view:** `ToolCallEntry.charge: Option<ToolCallCharge { credits, refused }>`, left out when `None`. The reducer sets it from the event, and the matter fold ignores the event.
- **The balance is not logged.** The relay sends it to the connector state (`BalanceReported`). It shows there only when no higher than the balance shown, after the balance epoch moves.
- **Model input is unchanged:** the relay only reads the result, and forwarding stays byte for byte.

## Consequences

- An older Clauseo reads the kind as unknown, and its rows show no cost.
- A folded group of steps shows no sum. Summing would need its own decision about calls whose charge isn't known.
- `recharge` with resume is not matched to a step. Its balance still updates Settings.
- A later change to the server's report shape must keep the integers-only reading, so that a changed shape shows no number rather than a wrong one.

## Status

proposed

## Date

2026-11-16
