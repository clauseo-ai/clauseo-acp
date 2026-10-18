/**
 * The one way the app reaches the host. Every frontend call to the host goes through this
 * interface, so the desktop webview and a test or remote client can each supply their own.
 * Its methods arrive with the protocol. Until then `kind` is all it checks: without a member, any
 * value (`42`) would pass as a transport.
 */
export interface HostTransport {
  /** Which transport this is, for logs and tests: `"null"` for now. */
  readonly kind: string;
}

/** A transport that reaches nothing. The app uses it until a real one exists. */
export class NullTransport implements HostTransport {
  readonly kind = "null";
}
