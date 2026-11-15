# Copy approval ids: who takes which

Every lawyer-facing string has an id registered in `ui/app/src/review/registry*.tsx` and an entry here. Its status is `approved`, `change` or `provisional`, and Hindi can carry its own `hi_status`. An id has the form `PREFIX-NN`, or `U3-err-<code>` for error codes (`status.ts`). An id in two files fails `copy:check`, and `copy:check` runs on every ui pull request. A `v*` release fails while any entry is not `approved` or has provisional Hindi (`.github/scripts/copy-release-gate.mjs`).

## Rules for a new string

- Put a branch's new entries in a file of its own, `UC-<branch>.json` for UC ids, marked `"status": "provisional"` (and `"hi_status": "provisional"` for Hindi not yet read by a Hindi-speaking lawyer). Never append to `UC.json` or another thread's file. That way two branches never edit the same file.
- Take ids only from your thread's block below. Ids already on main keep their numbers, and an id in an open pull request may stay where it is. These blocks apply to new ids.
- Don't reuse a retired id for different words: retire it and take a new one.

## UC blocks (from 2026-11-15)

| Thread    | UC ids                                |
| --------- | ------------------------------------- |
| app track | up to 303 (in use), and 400 to 499    |
| host      | 304 to 399                            |
| stage C   | 700 to 749                            |
| stage D   | 600 to 699                            |
| stage E   | 500 to 599                            |
| stage F   | 260 to 261 (reserved), and 750 to 799 |

## Other prefixes

- **Brief prefixes** (`U1` to `U17`, `U4B`, `U6F`, `UBL`, `UD1`, `UD2`, `UEP`, `UET`, `UTC`, `AR`, `C117`, `FC`, `FR`, `H11`, `IT`, `ME`, `MN`, `ND`, `NM`, `PK`, `PL`, `R23`, `RO`, `SAC`, `TF`, `TN`, `WA`, `WEB`):
  - Each belongs to the brief or area that made it, and its file is `<PREFIX>.json`.
  - The thread that owns that area adds the next free number in that file, and only that thread does.
  - For example, `MN` is the app's menu bar (app track), `FR` is the folder-rights words, and `U3` is the conversation's lines.
  - If you need a line in another thread's prefix, ask that thread, or the app track, which owns the copy system, for the next number. Don't guess.
- **`U3-err-<code>`:** made from the error code's name, one per `ErrorCode` (`registry.tsx`). A new error code gets its entry under that id, in the branch's own file, so there's nothing to hand out.
- **A new prefix** (a new brief or area): ask the app track first, so two threads never start the same one.
