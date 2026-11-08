// The desktop's folder chooser, scripted for tests, the review page and screenshots: a Windows tree of drives and
// folders, and one session over it that answers `chooser_starts`, `chooser_list`, `chooser_choose` and
// `chooser_cancel` as the desktop does (crates/desktop/src/chooser.rs), each held as long as the script says: moving by
// start key, by an index into the last listing, up to a drive's root,
// or to a typed path; refused folders listed but not opened; a folder that doesn't answer; one that isn't there.
import type { ChooserWire } from "./chooser.ts";
import type {
  ChooserStart,
  Go,
  HostError,
  Listing,
  ListState,
  PickedFolder,
  StartKind,
  StartPoint,
} from "./protocol.ts";

/** A folder in the scripted tree. */
export type FakeFolder = {
  /** Its folders, in the order a listing gives them. */
  folders?: Readonly<Record<string, FakeFolder>>;
  /** Listed, marked refused, and never opened (`refused_folders`). */
  refused?: boolean;
  /** Doesn't answer in time: always, or this many times before it lists (each Try again counts one). */
  slow?: true | number;
  /** More folders than the listing shows. */
  more?: boolean;
};

/** A path in the tree: its drive (`C:`), then each folder's name. */
export type FakePath = readonly string[];

/** The scripted chooser: its drives, where home, the Desktop and the last place are, and the native confirm. */
export type FakeChooser = {
  drives: Readonly<Record<string, FakeFolder>>;
  home: FakePath;
  desktop?: FakePath | null;
  /** The folder of the last pick, for Last place; none when left out. */
  last?: FakePath | null;
  /** The native confirm for the folder shown, its full path given: the folder registered, or null when the lawyer
   * declines. Left out, every folder is confirmed and registered as `chosen:<path>`. */
  choose?: (path: string) => PickedFolder | null | Promise<PickedFolder | null>;
  /** Held before `chooser_starts` or `chooser_list` answers, as the desktop's reads of the drives and of a folder
   * take their time; left out, both answer at once. */
  hold?: (call: "starts" | "list") => Promise<void>;
};

/** What a session saw, in order, for a test to read. */
export type FakeChooserCall =
  | { call: "chooser_starts"; chooser: number }
  | { call: "chooser_list"; chooser: number; go: Go }
  | { call: "chooser_choose"; chooser: number }
  | { call: "chooser_cancel"; chooser: number };

const invalid = (message: string): HostError => ({
  code: "command.invalid",
  message,
  retryable: false,
});

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** A start point's kind and path, in key order: the drives, home, the Desktop, the last place. */
function startsOf(tree: FakeChooser): { kind: StartKind; path: FakePath }[] {
  return [
    ...Object.keys(tree.drives).map((drive) => ({ kind: "drive" as const, path: [drive] })),
    { kind: "home" as const, path: tree.home },
    ...(tree.desktop ? [{ kind: "desktop" as const, path: tree.desktop }] : []),
    ...(tree.last ? [{ kind: "last_pick" as const, path: tree.last }] : []),
  ];
}

/** The path as the page shows it: below home, the parts below it (empty for home itself); otherwise in full. */
function displayOf(tree: FakeChooser, path: FakePath): { display: string; crumbs: string[] } {
  const home = tree.home;
  const underHome =
    path.length >= home.length && home.every((part, i) => same(part, path[i] ?? ""));
  if (underHome) {
    const rest = path.slice(home.length);
    return { display: rest.join("\\"), crumbs: ["", ...rest] };
  }
  const [drive = "", ...rest] = path;
  return {
    display: rest.length === 0 ? `${drive}\\` : [drive, ...rest].join("\\"),
    crumbs: [...path],
  };
}

/** The folder at `path`, by the names as the tree has them (any case), or null. */
function find(tree: FakeChooser, path: FakePath): { folder: FakeFolder; path: string[] } | null {
  const [drive, ...rest] = path;
  const driveName = Object.keys(tree.drives).find((d) => same(d, drive ?? ""));
  if (driveName === undefined) return null;
  let folder = tree.drives[driveName];
  const found = [driveName];
  for (const part of rest) {
    const name = Object.keys(folder?.folders ?? {}).find((n) => same(n, part));
    if (name === undefined || folder?.folders === undefined) return null;
    folder = folder.folders[name];
    found.push(name);
  }
  return folder === undefined ? null : { folder, path: found };
}

/** A typed path as the desktop takes it (`place.rs`, in short): `X:\` and the names under it. Null for a share,
 * which the fake has none of. Throws command.invalid for what the desktop refuses. */
function typedPath(text: string): FakePath | null {
  const typed = text.trim();
  if (typed.startsWith("\\\\?\\") || typed.startsWith("\\\\.\\")) throw invalid("refused path");
  if (typed.startsWith("\\\\")) return null;
  const drive = /^([A-Za-z]:)(?:[\\/](.*))?$/.exec(typed);
  if (drive === null) throw invalid("not an absolute path");
  const parts = (drive[2] ?? "").split(/[\\/]/).filter((p) => p !== "");
  if (parts.some((p) => p === ".." || p === "." || p.includes(":"))) throw invalid("refused path");
  return [(drive[1] ?? "").toUpperCase(), ...parts];
}

/** One chooser session over `tree`: its start, and its calls as the transport's wire. `seen` hears each call. */
export function fakeChooserSession(
  tree: FakeChooser,
  chooser: number,
  seen: (call: FakeChooserCall) => void = () => {},
): { start: ChooserStart; wire: ChooserWire } {
  const starts = startsOf(tree);
  let current: string[] | null = null;
  let listed: { name: string; refused: boolean }[] = [];
  let open = true;
  const tries = new Map<FakeFolder, number>();
  const points: StartPoint[] = starts.map((s, key) => ({
    key,
    kind: s.kind,
    display: s.kind === "drive" ? (s.path[0] ?? "") : displayOf(tree, s.path).display,
  }));

  const shown = (path: FakePath, state: ListState): Listing => ({
    ...displayOf(tree, path),
    state,
  });
  /** The listing of `path` (now the folder shown), or not_found with the folder shown as it was. */
  const read = (path: FakePath): Listing => {
    const at = find(tree, path);
    if (at === null) return shown(current ?? path, { kind: "not_found" });
    if (at.folder.refused === true) return shown(current ?? at.path, { kind: "not_allowed" });
    current = at.path;
    const { folder } = at;
    if (folder.slow !== undefined) {
      const tried = tries.get(folder) ?? 0;
      tries.set(folder, tried + 1);
      if (folder.slow === true || tried < folder.slow) {
        listed = [];
        return shown(at.path, { kind: "no_answer" });
      }
    }
    listed = Object.entries(folder.folders ?? {}).map(([name, f]) => ({
      name,
      refused: f.refused === true,
    }));
    return shown(at.path, { kind: "folders", items: listed, more: folder.more === true });
  };
  const live = () => {
    if (!open) throw invalid("the chooser session is closed");
  };

  const wire: ChooserWire = {
    async starts() {
      seen({ call: "chooser_starts", chooser });
      live();
      await tree.hold?.("starts");
      return points;
    },
    async list(go) {
      seen({ call: "chooser_list", chooser, go });
      live();
      await tree.hold?.("list");
      switch (go.kind) {
        case "start": {
          const start = starts[go.key];
          if (start === undefined) throw invalid("no such start");
          return read(start.path);
        }
        case "into": {
          const item = listed[go.index];
          if (item === undefined || current === null) throw invalid("no such folder");
          if (item.refused) return shown(current, { kind: "not_allowed" });
          return read([...current, item.name]);
        }
        case "up":
          if (current === null) throw invalid("nothing shown");
          return read(current.length > 1 ? current.slice(0, -1) : current);
        case "typed": {
          const path = typedPath(go.path);
          if (path === null) return shown(current ?? tree.home, { kind: "not_found" });
          return read(path);
        }
        case "again":
          if (current === null) throw invalid("nothing shown");
          return read(current);
      }
    },
    async choose() {
      seen({ call: "chooser_choose", chooser });
      live();
      if (current === null) throw invalid("nothing shown");
      const full = current.length === 1 ? `${current[0]}\\` : current.join("\\");
      const picked =
        tree.choose === undefined
          ? { folder: `chosen:${full}`, display: displayOf(tree, current).display }
          : await tree.choose(full);
      if (picked !== null) open = false;
      return picked;
    },
    async cancel() {
      seen({ call: "chooser_cancel", chooser });
      if (!open) throw invalid("the chooser session is closed");
      open = false;
    },
  };
  return { start: { chooser }, wire };
}

/** A lawyer's Windows machine, for tests and the review page: C: with Program Files and Windows (refused), home at
 * C:\Users\rmehra with its Documents, Desktop and a OneDrive folder that doesn't answer; D: with cases, and an archive
 * with more folders than shown; the last place Documents\Matters. */
export const FAKE_CHOOSER: FakeChooser = {
  drives: {
    "C:": {
      folders: {
        Matters: { folders: { Iyer: {}, "Kapoor Textiles": {} } },
        "Program Files": { refused: true },
        Users: {
          folders: {
            Public: { refused: true },
            rmehra: {
              folders: {
                Desktop: { folders: { "Hearing notes": {} } },
                Documents: {
                  folders: {
                    Clauseo: { refused: true },
                    Matters: {
                      folders: {
                        "Iyer v. Iyer": {},
                        "Mehta v. Mehta": {},
                        "Rathi Steels": { folders: { Orders: {}, Pleadings: {} } },
                        "Zaveri Exports": {},
                      },
                    },
                    "Old drafts": {},
                  },
                },
                Downloads: {},
                "OneDrive - Iyer & Co": { slow: true },
              },
            },
          },
        },
        Windows: { refused: true },
      },
    },
    "D:": {
      folders: {
        Archive: {
          more: true,
          folders: Object.fromEntries(
            ["2016", "2017", "2018", "2019", "2020", "2021", "2022", "2023"].map((y) => [y, {}]),
          ),
        },
        Cases: { folders: { "Acme Traders": {}, "Khan Leather Works": {} } },
      },
    },
  },
  home: ["C:", "Users", "rmehra"],
  desktop: ["C:", "Users", "rmehra", "Desktop"],
  last: ["C:", "Users", "rmehra", "Documents", "Matters"],
};
