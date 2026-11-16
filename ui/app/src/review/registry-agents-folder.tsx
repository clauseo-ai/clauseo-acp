// U3-err-matter.agents_folder.mac: the Mac's line for a ChatGPT send refused in a matter whose folder holds a .agents
// folder, which Finder hides, so the line says how to show it (words-errors.ts's AGENTS_FOLDER_MAC, picked on macOS
// only). The base line stays U3-err-matter.agents_folder (registry.tsx's ERRORS). Provisional; Hindi later.
import { AGENTS_FOLDER_MAC } from "@clauseo/components/lib/words-errors.ts";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomAsking } from "./states.tsx";
import { errorWords } from "./words.ts";

/** The Mac's line as the composer shows a refused send on the Mac: the question kept, the refusal beside it. */
export const AGENTS_FOLDER_MAC_ENTRIES: Entry[] = [
  {
    id: "U3-err-matter.agents_folder.mac",
    words: errorWords(AGENTS_FOLDER_MAC),
    screen: "room/composer",
    render: () => (
      <RoomAsking
        host={() =>
          threadHost({
            entries: [
              userMessage(5, 1, "What does clause nine of the lease say about notice?"),
              agentText(
                6,
                1,
                "Clause nine requires **notice in writing**, served at the tenant's registered office, at least thirty days ahead.",
              ),
              step(9),
            ],
            refuse: "matter.agents_folder",
            platform: "mac",
          })
        }
        question="And clause ten?"
      />
    ),
  },
];
