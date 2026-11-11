# 0058: The assistant says it is Clauseo and names the files it saves

## Context

Asked "who are you?", a real build answered "I'm ChatGPT, your AI assistant here", while the app
labels every reply "Clauseo". Nothing in the instructions named the assistant, so each agent fell
back on its own product name. The orchestrator asked for the change on 2026-11-11: the assistant
presents itself as Clauseo and, when asked, says which of the lawyer's plans it works through.

In the same session ChatGPT ended a draft with "Download the draft reply…" as a link that led
nowhere. A file the agent saves is in the matter, and the lawyer opens it from the papers list,
so a download, `file://` or `sandbox:` link or a full path only gets in the way. The
orchestrator asked for that rule in the same change.

The two rules need about 86 words, and 0031's budgets had 7 words left in `legal.md` (1,193 of 1,200)
and about 19 in Codex's whole text with a long data folder (about 1,481 of 1,500).

## Decision

- `legal.md`'s "Identity and audience" section gets a second paragraph after the senior-lawyer
  one, which is unchanged: "You are Clauseo, working through the lawyer's {plan} account. Only
  when asked who you are or what AI you run on, say so plainly: "I'm Clauseo. I'm using your
  {plan} account to work on this matter." Never claim to be another product or hide which account
  you use."
- `{plan}` is the profile's token, not the host's (`instructions::PLAN`). The profile fills it
  when it builds each agent's text (`ClauseoProfile::instructions_for`), with the plan's name as
  the window shows it (`instructions::plan_name`): `Claude` for Claude, `ChatGPT` for Codex.
  `ClauseoProfile::instructions` keeps the token, and no agent gets that text.
- It says "account", not "plan": a lawyer can sign in with an API key or the firm's endpoint
  instead of a plan, and the app tells an API-key user "Clauseo is using your API key".
- "Conduct"'s bullet on what never goes in text for the lawyer gains one sentence, for both
  agents alike: "Name a file you saved or changed plainly ("I saved it as Reply to legal
  notice.docx in the matter."), with no download link, `file://` or `sandbox:` link or full
  path: the lawyer opens it from the papers list."
- 0031's budgets are amended by what the two rules need plus about 10 words: `legal.md` at most
  1,290 words, and the whole instructions at most 1,580, which also caps Codex's text with the
  skills index (`the_budgets_hold_for_a_long_macos_path_with_the_folders_filled`). At this
  change: `legal.md` 1,279 words, the whole 1,469 with a data folder at `/data`, and Codex's
  whole about 1,569 with the long macOS folder.

## Consequences

- Each agent's instructions grow by about 120 tokens.
- A later per-agent difference in the instructions follows the same path: a profile token filled
  in `instructions_for`, never a second copy of `legal.md`.
- `legal.md` has about 11 words of headroom left; the next rule that needs more makes room or
  amends the budget again.

## Status

accepted

## Date

2026-11-11
