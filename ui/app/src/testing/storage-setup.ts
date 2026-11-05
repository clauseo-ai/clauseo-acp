// A DOM test's localStorage and sessionStorage work on every Node. Node 25 and later define globalThis.localStorage (Web
// Storage), which reads undefined without --localstorage-file. vitest's happy-dom environment installs happy-dom's
// storage only over a global key that is not already there, so the test sees the undefined one, where Node 24 (which
// has no global localStorage) gives it happy-dom's. Here a DOM test whose global storage is unusable gets a fresh
// happy-dom Window's storage, and the global back as it was when the file ends. Node 24 and the node-environment tests
// are unchanged: they never reach the branch.
import { afterAll } from "vitest";

const KEYS = ["localStorage", "sessionStorage"] as const;

function usable(key: (typeof KEYS)[number]): boolean {
  try {
    const storage = (globalThis as unknown as Partial<Record<string, Storage>>)[key];
    return typeof storage?.getItem === "function";
  } catch {
    return false;
  }
}

if (typeof document !== "undefined") {
  const missing = KEYS.filter((key) => !usable(key));
  if (missing.length > 0) {
    const { Window } = await import("happy-dom");
    const dom = new Window();
    const saved = missing.map(
      (key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)] as const,
    );
    for (const key of missing) {
      Object.defineProperty(globalThis, key, {
        value: dom[key],
        configurable: true,
        writable: true,
      });
    }
    afterAll(() => {
      for (const [key, descriptor] of saved) {
        if (descriptor === undefined) delete (globalThis as Record<string, unknown>)[key];
        else Object.defineProperty(globalThis, key, descriptor);
      }
    });
  }
}
