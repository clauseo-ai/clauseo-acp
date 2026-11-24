# Copy approval ids: who takes which

Every lawyer-facing string has an id registered in `ui/app/src/review/registry*.tsx` and an entry here. Its status is `approved`, `change` or `provisional`, and Hindi can carry its own `hi_status`. An id has the form `PREFIX-NN`, or `U3-err-<code>` for error codes (`status.ts`). An id in two files fails `copy:check`, and `copy:check` runs on every ui pull request. A `v*` release fails while any entry's English is not `approved` (`.github/scripts/copy-release-gate.mjs`); provisional Hindi only warns, with the count and the first ids. Rohan, 2026-11-15: hindi later; provisional Hindi warns.

## Rules for a new string

- Put a branch's new entries in a file of its own, `UC-<branch>.json` for UC ids, marked `"status": "provisional"` (and `"hi_status": "provisional"` for Hindi not yet read by a Hindi-speaking lawyer). Never append to `UC.json` or another thread's file. That way two branches never edit the same file.
- Take ids only from your thread's block below. Ids already on main keep their numbers, and an id in an open pull request may stay where it is. These blocks apply to new ids.
- Don't reuse a retired id for different words: retire it and take a new one.

## UC blocks (from 2026-11-15)

| Thread        | UC ids                                                                                        |
| ------------- | --------------------------------------------------------------------------------------------- |
| app track     | up to 303 (in use), 400 to 499 (in use; handed out below), and 800 to 899                     |
| host          | 304 to 399; from the app track: 424 to 429, 470 to 472, 491, 492                              |
| Indian Kanoon | from the app track: 450 to 469                                                                |
| stage C       | 700 to 749                                                                                    |
| stage D       | 600 to 699; from the app track: 420 to 423, 473, 484, 486, 487, 499                           |
| stage E       | 500 to 599; from the app track: 475 to 483, 488 to 490, 493 to 498                            |
| stage F       | 260 to 261 (reserved), and 750 to 799                                                         |
| shared        | 900 to 999, the reserve: the app track hands out sub-ranges to a thread that runs out (below) |

The app track's 400 to 499 is used up. Parts went to other threads, as their rows say: host follow-ups, the Indian Kanoon thread, stage D and stage E. The ids not handed out are the app track's own. New app-track ids take 800 to 899.

Kanoon: UC-450..UC-469, Indian Kanoon thread (macmini:ses_b07fd6e7771a40ae90ed): blackline PR 2 uses 450–464, the history tool 465–469. Blackline PR 2's eight part headings, UC-949..956, and its heading for a part of a kind the app doesn't know, UC-968, come from the app track's 947–989 block.

### Shared reserve: UC-900 to UC-999

When a thread runs out of its block, it asks the app track, which hands out a sub-range of 900 to 999. Every hand-out gets a row here.

| Thread                                                                      | UC ids           | Handed out |
| --------------------------------------------------------------------------- | ---------------- | ---------- |
| app track (helper step: UC-906, UC-907; others given out as their PRs land) | UC-900 to UC-919 | 2026-11-17 |
| online words (UC-online-only-read.json, within the app track's 900 to 919)  | UC-913 to UC-915 | 2026-11-17 |
| app track (blackline PR 2 uses UC-949 to UC-956 and UC-968)                 | UC-947 to UC-989 | 2026-11-17 |
| Settings › Skills, Add a skill (UC-skills.json)                             | UC-970 to UC-978 | 2026-11-17 |
| stage B automode (UC-codex-auto.json, within the app track's 947 to 989)    | UC-979 to UC-982 | 2026-11-17 |

### App track, round of 24 Nov: UC-1000 to UC-1299

The app track's block for Rohan's 15 issues of 24 Nov and the Windows tester's findings, one sub-range per writer.

| Part                                                                  | UC ids             | Handed out |
| --------------------------------------------------------------------- | ------------------ | ---------- |
| composer: model and effort (issues 1, 2, 11, the model menus)         | UC-1000 to UC-1019 | 2026-11-24 |
| sign-in line (issue 14)                                               | UC-1020 to UC-1024 | 2026-11-24 |
| sidebar buttons (issue 10)                                            | UC-1025 to UC-1029 | 2026-11-24 |
| New matter (issue 9)                                                  | UC-1030 to UC-1049 | 2026-11-24 |
| Skills and connectors (issues 6, 7, 8)                                | UC-1050 to UC-1089 | 2026-11-24 |
| home card for Clauseo Case Law (issue 8)                              | UC-1090 to UC-1095 | 2026-11-24 |
| step lines (issues 12, 13; pages that couldn't open)                  | UC-1100 to UC-1114 | 2026-11-24 |
| helpers and code mode (issues 12, 13)                                 | UC-1115 to UC-1129 | 2026-11-24 |
| switching agent: dividers and refusals (issue 4)                      | UC-1130 to UC-1144 | 2026-11-24 |
| answers at the same time (issue 15)                                   | UC-1145 to UC-1149 | 2026-11-24 |
| first-start permissions (issue 5)                                     | UC-1150 to UC-1169 | 2026-11-24 |
| plan running low                                                      | UC-1170 to UC-1179 | 2026-11-24 |
| conversation row label, paper tile names                              | UC-1180 to UC-1184 | 2026-11-24 |
| step lines, second block (pages that couldn't open, the safety check) | UC-1185 to UC-1199 | 2026-11-24 |
| agents' instructions (UC-claude-files.json)                           | UC-1200 to UC-1201 | 2026-11-24 |
| step lines' wait (Yield)                                              | UC-1202 to UC-1204 | 2026-11-24 |
| open each paper in its own program                                    | UC-1205 to UC-1214 | 2026-11-24 |
| Skills and connectors, second block (L1 to L5)                        | UC-1215 to UC-1259 | 2026-11-24 |
| not yet handed out                                                    | UC-1260 to UC-1299 | 2026-11-24 |

## Other prefixes: closed to new ids

**Every new lawyer-facing string takes a UC id from its thread's block, whatever screen it's on.**

- The other prefixes (`U1` to `U17`, `U4B`, `U6F`, `U8`, `UBL`, `UD1`, `UD2`, `UEP`, `UET`, `UTC`, `AR`, `C117`, `FC`, `FR`, `H11`, `IT`, `ME`, `MN`, `ND`, `NM`, `PK`, `PL`, `R23`, `RO`, `SAC`, `TF`, `TN`, `WA`, `WEB`, …) are closed to new ids. They stay only for strings already on main, or already in an open pull request on 2026-11-15.
- If an unmerged branch picked a new id under one of those prefixes and another open pull request uses the same id, the later branch moves its string into its own UC block.
- `U3-err-<code>` is the one exception: it's made from the error code's name, one per `ErrorCode` (`registry.tsx`), so there's nothing to hand out. A new error code gets its entry under that id, in the branch's own file. A new code whose line names the agent puts its other plans' lines under the same id with a suffix, `U3-err-<code>.chatgpt`, `.named` and `.nameless` (`agent.home_failed`, `registry-home-failed.tsx`).
- No new prefixes.
