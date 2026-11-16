//! For other crates' tests only (feature `test-support`, which only dev-dependencies turn on, so
//! no shipped build has it): the real proxy with a fixed table of names in place of the OS's
//! resolver, as this crate's own tests have through `start_with`. A test that makes up its sites
//! (the host's block list tests, whose upstream is a local stand-in) then resolves them with no
//! DNS, so it passes on an offline runner (CI's network namespace with loopback only), where a
//! real lookup fails and open mode skips the address check (a name through an upstream goes on
//! by name alone, `name_only`; a direct route gets 502). Only the resolver is
//! replaced: connects are real sockets, and every check, the machine's own networks
//! (`local_net`, read from the OS) and `is_public` included, is the production one.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;

use async_io::Async;
use futures::future::{self, BoxFuture};

use super::net::{Network, SystemNetwork};
use super::{ProxyConfig, ProxyHandle, start_with, system_options};
use crate::Error;

/// [`start_proxy`](super::start_proxy), with each name in `names` resolving to its addresses
/// only; any other name fails to resolve, as with no DNS. Names are matched in ASCII lower case.
pub fn start_proxy_with_names(
    config: ProxyConfig,
    names: &[(&str, &[IpAddr])],
) -> Result<ProxyHandle, Error> {
    let table = names
        .iter()
        .map(|(name, ips)| (name.to_ascii_lowercase(), ips.to_vec()))
        .collect();
    start_with(config, Arc::new(FixedNames(table)), system_options())
}

/// The fixed table, and the OS's sockets.
struct FixedNames(BTreeMap<String, Vec<IpAddr>>);

impl Network for FixedNames {
    fn resolve(&self, host: &str, port: u16) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        let found = match self.0.get(&host.to_ascii_lowercase()) {
            Some(ips) => Ok(ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect()),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "not in the test's names",
            )),
        };
        Box::pin(future::ready(found))
    }

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'static, io::Result<Async<TcpStream>>> {
        SystemNetwork.connect(addr)
    }
}
