// What each search sends (U15-52 to U15-57), always below Indian Kanoon's block: the facts of R24's "What goes off the
// machine" (tasks/stage-e/R24-connectors-in-host.md), in DESIGN's voice. {name} is the agents' names, as the plans
// the window knows name them (Claude, ChatGPT); both when it knows none, since either may run a case-law search.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { cw, type ConnectorsKey } from "@clauseo/components/lib/words-connectors.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import type { LimitsView } from "../../transport/protocol.ts";
import { agentNames } from "./connectors-model.ts";
import { Group, PlateRow } from "./parts.tsx";

const LINES: readonly ConnectorsKey[] = [
  "The words of each search go to Clauseo's server, which sends them on to Indian Kanoon.",
  "Papers aren't sent. Clauseo asks {name} to keep client names and confidential facts out of its searches.",
  "Clauseo's server deletes the words of each search after 7 days.",
  "Indian Kanoon sees Clauseo's server, never you.",
  "Each search and each judgment read costs credits.",
];

export function Sends({ lang, limits }: { lang: Lang; limits: LimitsView }) {
  const name = agentNames(lang, limits);
  return (
    <Group title={cw(lang, "What each search sends")}>
      <PlateRow>
        <ul className="set-cn-sends" data-testid="connectors-sends">
          {LINES.map((key) => (
            <li key={key} className="set-note">
              {say(cw(lang, key, { name }))}
            </li>
          ))}
        </ul>
      </PlateRow>
    </Group>
  );
}
