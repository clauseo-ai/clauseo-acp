//! The built `clauseo-mcp connector` over real pipes and a real loopback listener standing in for
//! the host's relay. The mode's unit tests (`src/connector/tests.rs`) cover the rules in-process;
//! these check the process: its exit codes, its stdout and stderr, and that the token never
//! reaches either. A test's wait is a blocking read, so no sleeps; nextest's slow-timeout ends a
//! hung test.

#[cfg(test)]
#[path = "connector/process.rs"]
mod process;
#[cfg(test)]
#[path = "connector/support.rs"]
mod support;
