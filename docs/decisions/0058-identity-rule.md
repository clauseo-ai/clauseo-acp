# 0058: The assistant says it is Clauseo, names the files it saves, and makes new documents clean

## Context

Asked "who are you?", a real build answered "I'm ChatGPT, your AI assistant here", while the app
labels every reply "Clauseo". Nothing in the instructions named the assistant, so each agent fell
back on its own product name. The orchestrator asked for the change on 2026-11-11: the assistant
presents itself as Clauseo and, when asked, says which of the lawyer's plans it works through.

In the same session ChatGPT ended a draft with "Download the draft reply…" as a link that led
nowhere. A file the agent saves is in the matter, and the lawyer opens it from the papers list,
so a download, `file://` or `sandbox:` link or a full path only gets in the way. The
orchestrator asked for that rule in the same change.

Asked to draft a new Word document, ChatGPT copied a seed .docx and wrote everything through
stetdoc, so "Reply to legal notice.docx" opened as a mass of tracked changes. stetdoc 0.9.3 has no
command that creates a document (`pack --no-track` and `edit --no-track` both need an existing
.docx). Clauseo's Python runtime ships pandoc 3.13 on the agents' `PATH`, and stage F
checked on Linux, with the pinned pandoc and stetdoc, that `pandoc -f markdown -o` gives real Heading and numbering styles and no `w:ins` or
`w:del`. Windows PowerShell 5.1 pipes text to a native program as ASCII, which turns ₹, Hindi and
curly quotes into "?", so pandoc reads a file, never a pipe.

The first two rules need about 86 words, and 0031's budgets had 7 words left in `legal.md` (1,193 of 1,200)
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
- The stetdoc line (`instructions::STETDOC_CLI`, `stetdoc.txt`), which the tools part gets when
  the app found a working stetdoc, keeps its sentence and gains: "Tracked changes are only for
  changing a paper that already exists. Make a new document clean: write it as a Markdown file in
  `{work_folder}`, run `pandoc "<file>.md" -o "<Name>.docx"` into the matter (never pipe into
  pandoc; headings and numbered paragraphs become Word styles), then delete the Markdown file."
  The Word guidance stays in one place, so the rule goes only where stetdoc was found; pandoc
  itself comes with the Python runtime, which the profile does not see. The saved-file rule then
  names the new file.
- 0031's budgets are amended by what the three rules need plus about 10 words: `legal.md` at
  most 1,290 words, and the whole instructions at most 1,650, which also caps Codex's text with
  the skills index (`the_budgets_hold_for_a_long_macos_path_with_the_folders_filled`). The
  whole now counts the stetdoc line as the app launches with it; before, the budget tests
  measured a profile without it, so its 17 words were never counted. At this change: `legal.md`
  1,279 words, the stetdoc line 65, the whole 1,534 with a data folder at `/data`, and Codex's
  whole about 1,636 with the long macOS folder. The tools part with the stetdoc line is 255
  words, under R21's 300.

## Consequences

- Each agent's instructions grow by about 190 tokens.
- A later per-agent difference in the instructions follows the same path: a profile token filled
  in `instructions_for`, never a second copy of `legal.md`.
- `legal.md` has about 11 words of headroom left; the next rule that needs more makes room or
  amends the budget again.

## Status

accepted

## Date

2026-11-11
