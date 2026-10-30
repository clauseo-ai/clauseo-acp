// A small reader for the token stylesheets: every rule's selector, the at-rules around it, and its custom
// properties. Enough for tokens.css; not a general CSS parser.

export type Block = { context: string; selector: string; decls: Map<string, string> };

const norm = (s: string) => s.replace(/\s+/g, " ").trim();

/** Every rule in css with its custom properties, skipping @theme, @utility and @custom-variant bodies. */
export function blocks(css: string): Block[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Block[] = [];
  const walk = (src: string, context: string) => {
    let i = 0;
    while (i < src.length) {
      const open = src.indexOf("{", i);
      if (open < 0) return;
      const head = norm(src.slice(i, open).split(";").pop() ?? "");
      let depth = 1;
      let j = open + 1;
      while (j < src.length && depth > 0) {
        if (src[j] === "{") depth++;
        else if (src[j] === "}") depth--;
        j++;
      }
      const body = src.slice(open + 1, j - 1);
      if (
        head.startsWith("@theme") ||
        head.startsWith("@utility") ||
        head.startsWith("@custom-variant")
      ) {
        // mappings and utilities, not token values
      } else if (head.startsWith("@")) {
        walk(body, norm(`${context} ${head}`));
      } else {
        const decls = new Map<string, string>();
        for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);?/g)) {
          if (m[1] !== undefined && m[2] !== undefined) decls.set(m[1], norm(m[2]));
        }
        out.push({ context, selector: head, decls });
      }
      i = j;
    }
  };
  walk(text, "");
  return out;
}
