// The host's mark on a conversation whose Clauseo Case Law call was refused for lack of credit (orchestrator r7462): a
// notice of kind "case_law_out_of_credit", no fields, logged at most once per turn in that thread. Its binding hasn't
// landed yet; NoticeKind already decodes an unknown kind as its wire string, so its final name is checked here, in
// this one place, and the binding's arrival changes nothing else.
import type { NoticeKind } from "../../transport/protocol.ts";

/** The kind's wire name. */
export const CASE_LAW_OUT_OF_CREDIT = "case_law_out_of_credit";

/** Whether a notice's kind is the out-of-credit mark. */
export const isCaseLawOutOfCredit = (kind: NoticeKind | null | undefined): boolean =>
  kind === CASE_LAW_OUT_OF_CREDIT;
