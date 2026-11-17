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

### Shared reserve: UC-900 to UC-999

When a thread runs out of its block, it asks the app track, which hands out a sub-range of 900 to 999. Every hand-out gets a row here.

| Thread                                                                      | UC ids           | Handed out |
| --------------------------------------------------------------------------- | ---------------- | ---------- |
| app track (helper step: UC-906, UC-907; others given out as their PRs land) | UC-900 to UC-919 | 2026-11-17 |

## Other prefixes: closed to new ids

**Every new lawyer-facing string takes a UC id from its thread's block, whatever screen it's on.**

- The other prefixes (`U1` to `U17`, `U4B`, `U6F`, `U8`, `UBL`, `UD1`, `UD2`, `UEP`, `UET`, `UTC`, `AR`, `C117`, `FC`, `FR`, `H11`, `IT`, `ME`, `MN`, `ND`, `NM`, `PK`, `PL`, `R23`, `RO`, `SAC`, `TF`, `TN`, `WA`, `WEB`, …) are closed to new ids. They stay only for strings already on main, or already in an open pull request on 2026-11-15.
- If an unmerged branch picked a new id under one of those prefixes and another open pull request uses the same id, the later branch moves its string into its own UC block.
- `U3-err-<code>` is the one exception: it's made from the error code's name, one per `ErrorCode` (`registry.tsx`), so there's nothing to hand out. A new error code gets its entry under that id, in the branch's own file.
- No new prefixes.
