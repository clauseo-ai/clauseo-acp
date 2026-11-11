// A new conversation's line before its first question, at the reading column's top: what it reads, the matter's
// papers counted, then how many more it can't read because they are only online (wall-away.ts), by the service that
// syncs the folder where it has a name. With no paper to read, it says the matter's papers are all only online, or,
// with none of those, that Clauseo couldn't open its folders. The room shows it whenever the wall has a tile.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import type { Papers } from "./host-next.ts";
import { allAwayWords, cloudOnlyCount, moreWords } from "./wall-away.ts";

export function NewConversationLine({ papers }: { papers: Papers }) {
  const { lang } = useDisplay();
  const n = papers.papers.length;
  const reads =
    n === 1
      ? mt(lang, "A new conversation. It reads this matter's one paper.")
      : mt(lang, "A new conversation. It reads this matter's {n} papers.", { n });
  const away = cloudOnlyCount(papers);
  const more = moreWords(lang, papers.place, away);
  const line =
    n === 0
      ? allAwayWords(lang, papers.place, away, papers.unread.length)
      : more === null
        ? reads
        : `${reads} ${more}`;
  return (
    <p
      className="mx-auto box-content max-w-measure px-thread-pad pt-s7 pb-s4 text-center text-reply text-primary"
      data-testid="new-conversation-line"
    >
      <Say>{line}</Say>
    </p>
  );
}
