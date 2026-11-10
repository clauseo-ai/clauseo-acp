// The wall's tiles: one per paper the wall lists, a Word document's tile opening it in Word and any
// other file of a type the desktop opens with the computer's default app (word/open-in-word.ts), the
// press's words on the face under the name. A name too long for its tile loses letters from its
// middle, never its extension, and shows whole in the tile's tooltip. What a tile's open said stands
// under the tiles until the next press of a tile (word/wall-said.tsx, in the thread's chunk with the
// change note's lines): the Protected View line, or a refused open's words. A .docx the host found
// tracked changes or comments in has a second, quiet press under its face, Tracked changes, which
// puts the file's tracked changes in the wall's place (word/changes-pane.tsx, its own chunk, loaded
// on that press), with Back to the tiles. The room keys the wall by its matter, so another matter's
// wall starts with nothing under it.
import { fileTypeIcon } from "@clauseo/components/icons/file-type.ts";
import { PaperTile } from "@clauseo/components/clauseo/paper-tile.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { late } from "@clauseo/components/lib/late.ts";
import { LayerProvider } from "@clauseo/components/ui/layer.tsx";
import { lazy, Suspense, useRef, useState } from "react";
import type { Papers } from "./host-next.ts";
import type { WordOpen, WordOpened } from "./word/open-in-word.ts";

const WallSaid = lazy(() =>
  late(import("./thread/list.tsx")).then((m) => ({ default: m.WallSaid })),
);

type ChangesChunk = typeof import("./word/changes-chunk.ts");

/** The tracked changes' chunk and its sheet, loaded once; a load that failed is tried again at the next press. */
let changesChunk: Promise<ChangesChunk> | null = null;
const loadChanges = (): Promise<ChangesChunk> => {
  changesChunk ??= import("./word/changes-chunk.ts").catch((error: unknown) => {
    changesChunk = null;
    throw error;
  });
  return late(
    changesChunk.then(async (m) => {
      await m.loadSheet();
      return m;
    }),
  );
};

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** The tiles and their line, or one .docx's tracked changes in their place. Not memoised ("use no memo"): the room
 * memoises the wall's element on its papers and its opener, so it renders again only for those and its own state, and
 * the compiler's caches would cost the first load. */
export function PaperWall({ papers, word }: { papers: Papers; word: WordOpen }) {
  "use no memo";
  const { id } = useRoom();
  // the last tile's open, and of which paper
  const [said, setSaid] = useState<{ path: string; opened: WordOpened } | null>(null);
  // the .docx whose tracked changes stand in the tiles' place, with their panel once its chunk is in
  const [changes, setChanges] = useState<{
    path: string;
    file: string;
    Pane: ChangesChunk["ChangesPane"];
  } | null>(null);
  // Back gives the focus to the Tracked changes press it came from, once the tiles are back
  const back = useRef<string | null>(null);
  const saidBy = said && word.wallOpenerFor(said.path);
  const open = changes === null ? undefined : word.openerFor(changes.path)?.changes;
  if (changes !== null && open !== undefined) {
    const { Pane } = changes;
    return (
      <Pane
        transport={open.transport}
        matter={open.matter}
        path={changes.path}
        onBack={() => {
          back.current = changes.file;
          setChanges(null);
        }}
      />
    );
  }
  return (
    // a Word tile's tooltip mounts at the end of the wall, inside its landmark, not at the end of the body
    <LayerProvider>
      <ol
        className="facade facade-names facade-files"
        ref={(node) => {
          const from = back.current;
          if (node === null || from === null) return;
          back.current = null;
          document
            .getElementById(id(`paper-${from}`))
            ?.querySelector<HTMLElement>(".tile-act")
            ?.focus();
        }}
      >
        {papers.papers.map(({ file, path }) => {
          const opener = word.wallOpenerFor(path);
          const tracked = opener?.changes;
          const name = fileName(path);
          return (
            <PaperTile
              key={file}
              id={file}
              state="uncited"
              title={name}
              fileName
              tip={name}
              fileIcon={fileTypeIcon(path)}
              {...(opener === null
                ? {}
                : {
                    onTurn: () => {
                      setSaid(null);
                      void opener.onOpen().then((opened) => setSaid({ path, opened }));
                    },
                    cue: opener.label,
                    testIds: { face: word.openerFor(path) === null ? "open-file" : "open-in-word" },
                  })}
              {...(tracked === undefined
                ? {}
                : {
                    action: {
                      label: tracked.label,
                      testId: "tracked-changes",
                      onPress: () =>
                        void loadChanges().then(
                          (m) => {
                            setSaid(null);
                            setChanges({ path, file, Pane: m.ChangesPane });
                          },
                          (error: unknown) =>
                            console.warn(`the tracked changes did not load: ${String(error)}`),
                        ),
                    },
                  })}
            />
          );
        })}
      </ol>
      {said && saidBy && (
        <Suspense fallback={null}>
          <WallSaid
            opener={saidBy}
            opened={said.opened}
            onRefused={(refused) =>
              setSaid({ path: said.path, opened: { refused, protectedView: false } })
            }
          />
        </Suspense>
      )}
    </LayerProvider>
  );
}
