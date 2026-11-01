// Which colouring client a code block asks: the page's one client unless a provider says otherwise (the tests' fake).
import { createContext } from "react";
import type { HighlightClient } from "./client.ts";

/** null: the page's one client (start-worker.ts), created on the first block that asks. */
export const HighlightClientContext = createContext<HighlightClient | null>(null);
