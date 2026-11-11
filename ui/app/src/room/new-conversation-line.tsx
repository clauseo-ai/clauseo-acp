// A new conversation's line before its first question, at the reading column's top: what it reads, the matter's
// papers counted, then how many more it can't read because they are only online (wall-away.ts), by the service that
// syncs the folder where it has a name. The room shows it only once there is a paper to read.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import type { Papers } from "./host-next.ts";
import { cloudOnlyCount, moreWords } from "./wall-away.ts";

export function NewConversationLine({ papers }: { papers: Papers }) {
  const { lang } = useDisplay();
  const n = papers.papers.length;
  const reads =
    n === 1
      ? mt(lang, "A new conversation. It reads this matter's one paper.")
      : mt(lang, "A new conversation. It reads this matter's {n} papers.", { n });
  const more = moreWords(lang, papers.place, cloudOnlyCount(papers));
  return (
    <p
      className="mx-auto box-content max-w-measure px-thread-pad pt-s7 pb-s4 text-center text-reply text-primary"
      data-testid="new-conversation-line"
    >
      <Say>{more === null ? reads : `${reads} ${more}`}</Say>
    </p>
  );
}
