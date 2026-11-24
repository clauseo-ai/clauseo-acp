// The host's mark on a conversation whose Clauseo Case Law call was refused for lack of credit: a notice of kind
// "case_law_out_of_credit" (the generated NoticeKind's unit variant), no fields, logged at most once per turn in that
// thread. NoticeKind ends in `string` (a newer kind), so a rename there can't fail the typecheck; this is the one place
// the name is written.
import type { NoticeKind } from "../../transport/protocol.ts";

/** The kind's wire name. */
export const CASE_LAW_OUT_OF_CREDIT: NoticeKind = "case_law_out_of_credit";

/** Whether a notice's kind is the out-of-credit mark. */
export const isCaseLawOutOfCredit = (kind: NoticeKind | null | undefined): boolean =>
  kind === CASE_LAW_OUT_OF_CREDIT;
