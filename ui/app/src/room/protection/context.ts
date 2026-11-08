// UC: what the room's protection gives the open conversation's composer: its line under the field, whether the firm's
// requirement holds the send, and the notice that says so. Provided by room.tsx (use-room-protection.tsx); a composer
// outside a room (tests, the review) reads null and shows no line, as before UC.
import { createContext, type ReactNode } from "react";

export type ComposerProtection = {
  /** The room's line: Not protected; null for the composer's own Safe workspace line; false for none (the matter's
   * protection isn't known). */
  line: ReactNode | false;
  /** The firm requires protection and the open matter has none: send is held. */
  blocked: boolean;
  /** The firm's notice in the notice slot while blocked; null otherwise. */
  notice: ReactNode;
};

export const ProtectionContext = createContext<ComposerProtection | null>(null);
