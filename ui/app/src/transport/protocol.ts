// The protocol's types as the app reads them, from the bindings tauri-specta generates (bindings.ts, the one TS home of
// those types). The store and the window import types from here, never from bindings.ts, so nothing outside
// src/transport/ reaches the module that imports Tauri. Types only; nothing here runs.
import type * as B from "./bindings.ts";

export type * from "./bindings.ts";

// TODO(#1348): the host's #1348 adds this field to bindings.ts in these words (and the error code
// connector.keychain_refused, which words-errors.ts's KeychainRefused stands in for); once it lands, delete from here
// to the end, and the import above.
/** One connector, with #1348's `left_in_keychain`. */
export type ConnectorView = B.ConnectorView & {
  /**
   *  Whether the last `ConnectorCommand::Remove` (which answered `Done`) left the saved
   *  sign-in in the keychain: the keychain refused its read (on a Mac nothing was then revoked
   *  or deleted) or its delete. The connector is forgotten on this computer either way, and
   *  listed while this is set; a later Add reads that sign-in again. It lasts until the next
   *  `Add`, `AddApiKey`, `SignOut` or `Remove`. False is written as a missing field, and a
   *  missing field reads as false (additive: an older host leaves it out).
   */
  left_in_keychain?: boolean;
};
/** The connectors, each with #1348's `left_in_keychain`. */
export type ConnectorsState = Omit<B.ConnectorsState, "connectors"> & {
  connectors: ConnectorView[];
};
