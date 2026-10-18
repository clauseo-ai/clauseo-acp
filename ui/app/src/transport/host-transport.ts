/**
 * The one way the app reaches the host. Every frontend call to the host goes through this
 * interface, so the desktop webview and a test or remote client can each supply their own.
 * It has no methods yet; they arrive with the protocol.
 */
export interface HostTransport {}

/** A transport that reaches nothing. The app uses it until a real one exists. */
export class NullTransport implements HostTransport {}
