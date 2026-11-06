// Where the expired sign-in card stands, from the agent's sign-in, and its signed-in-again line: pure, for
// expired-card.tsx and its tests.
import type { Found } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { sx } from "@clauseo/components/lib/words-signin-expired.ts";
import type { SignIn, Timestamp } from "../../transport/protocol.ts";

/** Where the card stands, from the agent's sign-in. */
export type Expired =
  | { kind: "expired" }
  | { kind: "waiting" }
  | { kind: "again"; at: Timestamp; email?: string | undefined; plan?: string | undefined }
  | { kind: "reuse-stopped"; found: Found }
  | { kind: "reused-failed" }
  | { kind: "switching"; stuck: boolean }
  | { kind: "key" }
  | { kind: "firm" }
  | { kind: "failed" };

/** The card's state for the agent's sign-in, the failed turn's step at `failedAt`. A plan login signed in after the
 * failure is "signed in again"; one signed in before it is the login that expired. */
export function expiredOf(signIn: SignIn | undefined, failedAt: Timestamp): Expired {
  if (signIn === undefined || signIn === "queued")
    return signIn === "queued" ? { kind: "waiting" } : { kind: "expired" };
  if (signIn.signed_in !== undefined) {
    const s = signIn.signed_in;
    if (s.method === "api_key") return { kind: "key" };
    if (s.method === "firm") return { kind: "firm" };
    return s.at > failedAt
      ? { kind: "again", at: s.at, email: s.email, plan: s.plan }
      : { kind: "expired" };
  }
  if (signIn.waiting !== undefined || signIn.device_code !== undefined) return { kind: "waiting" };
  if (signIn.switching !== undefined) return { kind: "switching", stuck: signIn.switching.stuck };
  if (signIn.signed_out?.found !== undefined) {
    const found = signIn.signed_out.found;
    // nothing found, or reuse off here: a plain signed-out, as the first run's gate reads it (views.ts)
    if (found === "none_found" || found === "reuse_off") return { kind: "expired" };
    return found === "reused_failed" ? { kind: "reused-failed" } : { kind: "reuse-stopped", found };
  }
  if (signIn.failed !== undefined && signIn.failed.code !== "agent.auth_required")
    return { kind: "failed" };
  // signed out, a login found (the card offers its own sign-in), or the agent's own refusal
  return { kind: "expired" };
}

/** "Signed in again at 4:32 pm as …, Claude Max.", by what the host gave. */
export function againLine(
  lang: Lang,
  e: Extract<Expired, { kind: "again" }>,
  time: string,
): string {
  if (e.email !== undefined && e.plan !== undefined)
    return sx(lang, "Signed in again at {time} as {email}, {plan}.", {
      time,
      email: e.email,
      plan: e.plan,
    });
  if (e.email !== undefined)
    return sx(lang, "Signed in again at {time} as {email}.", { time, email: e.email });
  return sx(lang, "Signed in again at {time}.", { time });
}
