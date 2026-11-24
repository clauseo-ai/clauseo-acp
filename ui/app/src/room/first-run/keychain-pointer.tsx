// What points at the keychain row (stage E: prompting off, so items another build wrote read as unreadable until the
// row is allowed): an unreadable agent key, a signed-out Case Law, a tool's missing key, a refused keychain write.
// While the window's one permissions read (permissions-host.ts's heardOf) lists the row not allowed, each says one line
// and presses the row's own ask through, never a sign-in or the key typed again. Granted, or no row: today's words.
import type { ReactNode } from "react";
import { Act } from "@clauseo/components/clauseo/first-launch/parts.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../../transport/host-transport.ts";
import { pointsToKeychain, useKeychain, type Keychain } from "./keychain-state.ts";

export function KeychainAllow({ k, small = true }: { k: Keychain; small?: boolean }) {
  const { lang } = useDisplay();
  return (
    <Button
      kind="tone"
      {...(small ? { size: "small" as const } : {})}
      disabled={k.asking}
      onClick={k.allow}
      data-testid="keychain-allow"
    >
      {fl(lang, "Allow")}
    </Button>
  );
}

/** The gate's failure, for the composer's gate and the thread's expired card: the pointer as the gate's act row. */
export function GateKeyLine(p: { transport: HostTransport; own: ReactNode; code: string }) {
  const k = useKeychain(p.transport);
  if (k === null || !pointsToKeychain(p.code)) return p.own;
  return (
    <Act act={<KeychainAllow k={k} />}>
      <Say>{k.line}</Say>
    </Act>
  );
}
