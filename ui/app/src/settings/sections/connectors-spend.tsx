// Connectors' recent spend (U15): today's credits, then each recent day the account lists, newest first, credits
// only, since the wire carries no rupee figure for spend. Today's is "Today", the others "Wednesday 4 November", as
// the thread's day lines read (limit-note.tsx's DayLine); a day the app can't read is left out (connectors-model.ts).
import type { Lang } from "@clauseo/components/lib/display.ts";
import { count } from "@clauseo/components/lib/money.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { useNow } from "../../store/limits.ts";
import type { RecentSpend } from "../../transport/protocol.ts";
import { dateOf, spendRows } from "./connectors-model.ts";
import { Group, PlateRow } from "./parts.tsx";

export function Spend({ spend, lang }: { spend: RecentSpend; lang: Lang }) {
  const now = useNow();
  const rows = spendRows(lang, spend, dateOf(now));
  const none = spend.today_credits === 0 && spend.days.length === 0;
  return (
    <Group title={cw(lang, "Recent spend")}>
      {none ? (
        <PlateRow>
          <p className="set-note" data-testid="spend-none">
            {say(cw(lang, "Nothing spent yet."))}
          </p>
        </PlateRow>
      ) : (
        rows.map((row) => (
          <PlateRow key={row.day} className="set-cn-spend">
            <p className="text-ui text-primary">{say(row.label)}</p>
            <p className="set-cn-figure">
              {say(cw(lang, "{credits} credits", { credits: count(row.credits) }))}
            </p>
          </PlateRow>
        ))
      )}
    </Group>
  );
}
