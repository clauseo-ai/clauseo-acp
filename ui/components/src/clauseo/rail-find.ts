// Finding a matter from the rail. Pure, so the rail and the tests share it. A search matches a matter's name as filed or
// its client, ignoring case, spaces and dots; names come first, then clients, each in the rail's own order.

/** What a matter can be found by. */
export type Findable = { name: string; client: string };

/** What matched, so the row can say it when the name alone doesn't show why. */
export type FoundBy = "name" | "client";

const fold = (s: string) => s.toLowerCase().replace(/[\s.]+/g, "");

/** The matters a search finds, in order, each with what it matched on. An empty search finds nothing. */
export function findMatters<M extends Findable>(
  all: readonly M[],
  query: string,
): { matter: M; by: FoundBy }[] {
  const q = fold(query);
  if (q === "") return [];
  const found: { matter: M; by: FoundBy }[] = [];
  for (const by of ["name", "client"] as const) {
    for (const m of all) {
      if (found.some((f) => f.matter === m)) continue;
      if (fold(m[by]).includes(q)) found.push({ matter: m, by });
    }
  }
  return found;
}
