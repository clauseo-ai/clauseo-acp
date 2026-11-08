// UC: which body the Not protected note says for a cause (components' protection.tsx's NoteWhy), apart from note.tsx so
// that file exports its component alone.
import type { NoteWhy } from "@clauseo/components/clauseo/protection.tsx";
import { blockOf } from "./blocked.ts";
import type { Cause } from "./causes.ts";
import { whyOf } from "./detail.ts";

/** The note's `why` for a cause: a missing admin copy reads as not set up yet; a broken set-up and a cause this
 * version doesn't know have their own refusal (R17 PR-E's M4). */
export const noteWhy = (cause: Cause): NoteWhy =>
  cause.name === "pool_owned" || cause.name === "set_up_failed" || cause.name === "unknown"
    ? cause.name
    : blockOf(cause) === "admin_copy_missing"
      ? "needs_set_up"
      : blockOf(cause) !== null
        ? "blocked"
        : cause.group === "setup"
          ? (cause.name as NoteWhy)
          : cause.group === "off"
            ? "off"
            : cause.group === "pool"
              ? "pool"
              : whyOf(cause) === "not_built"
                ? "not_built"
                : "cant";
