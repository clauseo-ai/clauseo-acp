// The wall's tiles: one per paper the wall lists, a Word document's tile opening it in Word (word/open-in-word.ts).
// What a tile's open said stands under the tiles until the next press of a tile (word/wall-said.tsx, in the thread's
// chunk with the change note's lines): the Protected View line, or a path too long for Word with Show in folder. The
// room keys the wall by its matter, so another matter's wall starts with nothing under it.
import { fileTypeIcon } from "@clauseo/components/icons/file-type.ts";
import { PaperTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import { late } from "@clauseo/components/lib/late.ts";
import { LayerProvider } from "@clauseo/components/ui/layer.tsx";
import { lazy, Suspense, useState } from "react";
import type { Papers } from "./host-next.ts";
import type { WordOpen, WordOpened } from "./word/open-in-word.ts";

const WallSaid = lazy(() =>
  late(import("./thread/list.tsx")).then((m) => ({ default: m.WallSaid })),
);

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** The tiles and their line. Not memoised ("use no memo"): the room memoises the wall's element on its papers and its
 * opener, so it renders again only for those and its own line, and the compiler's caches would cost the first load. */
export function PaperWall({ papers, word }: { papers: Papers; word: WordOpen }) {
  "use no memo";
  // the last tile's open, and of which paper
  const [said, setSaid] = useState<{ path: string; opened: WordOpened } | null>(null);
  const saidBy = said && word.openerFor(said.path);
  return (
    // a Word tile's tooltip mounts at the end of the wall, inside its landmark, not at the end of the body
    <LayerProvider>
      <ol className="facade facade-names facade-files">
        {papers.papers.map(({ file, path }) => {
          const opener = word.openerFor(path);
          return (
            <PaperTile
              key={file}
              id={file}
              state="uncited"
              title={fileName(path)}
              fileIcon={fileTypeIcon(path)}
              {...(opener === null
                ? {}
                : {
                    onTurn: () => {
                      setSaid(null);
                      void opener.onOpen().then((opened) => setSaid({ path, opened }));
                    },
                    tip: opener.label,
                    label: opener.label,
                    testIds: { face: "open-in-word" },
                  })}
            />
          );
        })}
      </ol>
      {said && saidBy && (
        <Suspense fallback={null}>
          <WallSaid opener={saidBy} opened={said.opened} />
        </Suspense>
      )}
    </LayerProvider>
  );
}
