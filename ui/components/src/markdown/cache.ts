// Caches the answer model (answer.ts) keeps from one update to the next, so that what did not change
// is the same object or string as before: React's memoised blocks and the model's own lookups rely on it.

/** A cache that keeps only what the last update used, once it holds more than twice that. */
export function createCache<V>() {
  const map = new Map<string, V>();
  let used = new Set<string>();
  return {
    get(key: string, make: () => V): V {
      used.add(key);
      let value = map.get(key);
      if (value === undefined) {
        value = make();
        map.set(key, value);
      }
      return value;
    },
    /** Mark `key` as used by this update without looking it up. */
    keep(key: string) {
      used.add(key);
    },
    /** Forget every entry whose key holds `text`. */
    forget(text: string) {
      for (const key of map.keys()) if (key.includes(text)) map.delete(key);
    },
    sweep() {
      if (map.size > 2 * used.size + 16)
        for (const key of map.keys()) if (!used.has(key)) map.delete(key);
      used = new Set();
    },
    /** Forget every entry but those of `keys`, whatever this update used (sweep's place for a strict cache). */
    retain(keys: ReadonlySet<string>) {
      for (const key of map.keys()) if (!keys.has(key)) map.delete(key);
      used = new Set();
    },
  };
}

/** A cache key for an object, by identity. */
export function createIds() {
  const ids = new WeakMap<object, number>();
  let next = 0;
  return (value: object) => {
    let id = ids.get(value);
    if (id === undefined) ids.set(value, (id = next++));
    return id;
  };
}

/** `make(value)`, made once per value object and the same object each time after (by identity). */
export function createVariants<V extends object>(make: (value: V) => V): (value: V) => V {
  const made = new WeakMap<V, V>();
  return (value) => {
    let known = made.get(value);
    if (!known) made.set(value, (known = make(value)));
    return known;
  };
}

type Joined = { parts: readonly string[]; joined: string };

/**
 * Joins strings (a paragraph's parts, split.ts's joinParts), the same string each update while the parts
 * are the same strings: a finished block's caches and views are found by it without hashing a new copy
 * every update. `sweep`, at the end of each update, keeps only what that update joined.
 */
export function createJoiner(): ((parts: readonly string[]) => string) & { sweep: () => void } {
  let before = new Map<string, Joined>();
  let now = new Map<string, Joined>();
  const join = (parts: readonly string[]): string => {
    let known = before.get(parts[0]!);
    if (known?.parts.length !== parts.length || known.parts.some((part, at) => part !== parts[at]))
      known = { parts, joined: parts.join("") };
    now.set(parts[0]!, known);
    return known.joined;
  };
  return Object.assign(join, {
    sweep: () => {
      before = now;
      now = new Map();
    },
  });
}
