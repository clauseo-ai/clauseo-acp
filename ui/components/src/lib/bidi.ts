// A path set into a line of words is isolated (docs/ui-states.md: every path shown is bidi-isolated), so a folder
// named with right-to-left marks, such as U+202E, can't reorder the words around it. In markup, <bdi>; in a string (a
// slot of a words key, a title), First Strong Isolate and Pop Directional Isolate around it.

/** The path between FSI (U+2068) and PDI (U+2069): a string-only <bdi>. */
export const isolate = (path: string): string => `\u2068${path}\u2069`;
