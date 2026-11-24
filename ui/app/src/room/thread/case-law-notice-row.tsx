// The case_law_out_of_credit notice as a quiet row at its point in the conversation, where a Clauseo Case Law search
// was refused: what happened, in the past tense, with no button, "Clauseo Case Law couldn't search here: out of
// credits.". It stays as history after credits arrive. The composer's strip carries the action and Add credits, so
// the two never say the same sentence twice. In the thread's chunk, with its words.
import { cr } from "@clauseo/components/lib/words-connectors-room.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import type { EntryId } from "../../transport/protocol.ts";

export function CaseLawNoticeRow({ id, className }: { id: EntryId; className: string }) {
  const { lang } = useDisplay();
  return (
    <p data-testid="case-law-notice" data-entry-id={id} className={className} dir="auto">
      {cr(lang, "Clauseo Case Law couldn't search here: out of credits.")}
    </p>
  );
}
