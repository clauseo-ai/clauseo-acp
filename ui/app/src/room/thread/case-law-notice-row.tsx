// The case_law_out_of_credit notice as a quiet row at its point in the conversation, where a Clauseo Case Law search
// was refused: the rule's words from the CURRENT status (store/case-law-credit.ts), "Not enough credits for a search.
// Add credits to keep searching." or at 0 "No credits. Add credits to search.", no figure unless the status carries
// one. Once credit is back, what happened there: "Clauseo Case Law had no credits for this search.". In the thread's
// chunk, with its words.
import { use } from "react";
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { connectorsStoreOf, useCaseLawCredit } from "../../store/connectors.ts";
import type { CaseLawCredit } from "../../store/case-law-credit.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId } from "../../transport/protocol.ts";
import { SourcesTransport } from "./sources.ts";

function Live({
  transport,
  children,
}: {
  transport: HostTransport;
  children: (c: CaseLawCredit | null) => string;
}) {
  return <>{children(useCaseLawCredit(connectorsStoreOf(transport)))}</>;
}

export function CaseLawNoticeRow({ id, className }: { id: EntryId; className: string }) {
  const { lang } = useDisplay();
  const transport = use(SourcesTransport);
  const past = cr(lang, "Clauseo Case Law had no credits for this search.");
  const say = (credit: CaseLawCredit | null) =>
    credit?.kind !== "out"
      ? past
      : credit.credits === 0
        ? cr(lang, "No credits. Add credits to search.")
        : cr(lang, "Not enough credits for a search. Add credits to keep searching.");
  return (
    <p data-testid="case-law-notice" data-entry-id={id} className={className} dir="auto">
      {transport === null ? past : <Live transport={transport}>{say}</Live>}
    </p>
  );
}
