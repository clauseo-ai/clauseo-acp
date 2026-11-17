// A saved API key the keychain didn't give (decision record 0048): its read had not answered in time
// (agent.key_unreadable), or the keychain answered it with its error, such as a denied prompt (agent.key_denied). Each
// surface that says one says the other the same way: its own words, Try again, and the way out through Plans.

/** The two codes of a saved key not read. */
export const KEY_NOT_READ = ["agent.key_unreadable", "agent.key_denied"] as const;

/** One of {@link KEY_NOT_READ}. */
export type KeyNotRead = (typeof KEY_NOT_READ)[number];

/** Whether `code` is a saved key not read, of either kind. */
export const isKeyNotRead = (code: string | null | undefined): code is KeyNotRead =>
  code === "agent.key_unreadable" || code === "agent.key_denied";
