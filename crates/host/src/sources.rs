//! The thread's sources: what the agent read on the web, recorded on the thread from the ACP tool
//! calls themselves (synthesis §3.3, seam 3: tool calls are looked up by name) and shown in the
//! answer's sources list. Only the browser's pages so far ([`browser`]); S26 adds the agents'
//! own searches and fetches.

pub(crate) mod browser;
