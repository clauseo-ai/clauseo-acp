//! The test network: names to addresses, and addresses to local servers, with a record of
//! every call and holds on the next lookup or connect.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};

use async_io::Async;
use futures::channel::oneshot;
use futures::future::BoxFuture;

use crate::proxy::net::Network;

/// Names to addresses, and addresses to local servers, with a record of every call.
#[derive(Default)]
pub(crate) struct TestNet {
    /// Successive answers per name; the last one repeats.
    names: Mutex<BTreeMap<String, Vec<Vec<IpAddr>>>>,
    routes: Mutex<BTreeMap<SocketAddr, SocketAddr>>,
    dialled: Mutex<Vec<SocketAddr>>,
    resolved: Mutex<Vec<String>>,
    /// The names the name-only path's probe asked, apart from `resolved`.
    probed: Mutex<Vec<String>>,
    /// Holds the next resolve, or the next connect, until released.
    hold_resolve: Mutex<Option<Hold>>,
    hold_connect: Mutex<Option<Hold>>,
}

/// One held call: it tells the test it arrived, then waits for the release.
struct Hold {
    arrived: mpsc::Sender<()>,
    release: oneshot::Receiver<()>,
}

/// The test's end of a held call: wait for it to arrive, then release it.
pub(crate) struct Held {
    pub(crate) arrived: Receiver<()>,
    pub(crate) release: oneshot::Sender<()>,
}

fn hold(slot: &Mutex<Option<Hold>>) -> Held {
    let (arrived_tx, arrived) = mpsc::channel();
    let (release, release_rx) = oneshot::channel();
    *slot.lock().unwrap() = Some(Hold {
        arrived: arrived_tx,
        release: release_rx,
    });
    Held { arrived, release }
}

/// Waits for the hold's release, if the call was held.
async fn wait_for(hold: Option<Hold>) {
    if let Some(hold) = hold {
        hold.arrived.send(()).unwrap();
        match hold.release.await {
            Ok(()) | Err(oneshot::Canceled) => {}
        }
    }
}

impl TestNet {
    /// `host` resolves to `ips`.
    pub(crate) fn name(&self, host: &str, ips: &[&str]) {
        self.answers(host, &[ips]);
    }

    /// `host` resolves to each answer in turn.
    pub(crate) fn answers(&self, host: &str, answers: &[&[&str]]) {
        let answers = answers
            .iter()
            .map(|ips| ips.iter().map(|ip| ip.parse().unwrap()).collect())
            .collect();
        self.names.lock().unwrap().insert(host.to_owned(), answers);
    }

    /// A connect to `from` reaches the server at `to`.
    pub(crate) fn route(&self, from: &str, to: SocketAddr) {
        self.routes
            .lock()
            .unwrap()
            .insert(from.parse().unwrap(), to);
    }

    pub(crate) fn dialled(&self) -> Vec<SocketAddr> {
        self.dialled.lock().unwrap().clone()
    }

    pub(crate) fn resolved(&self) -> Vec<String> {
        self.resolved.lock().unwrap().clone()
    }

    pub(crate) fn probed(&self) -> Vec<String> {
        self.probed.lock().unwrap().clone()
    }

    /// The next answer for `host`: an error for a name it does not know.
    fn answer(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        let mut names = self.names.lock().unwrap();
        let answer = names.get_mut(host).and_then(|answers| {
            if answers.len() > 1 {
                Some(answers.remove(0))
            } else {
                answers.first().cloned()
            }
        });
        match answer {
            Some(ips) => Ok(ips
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect()),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "no such name")),
        }
    }

    /// The next resolve waits until the test releases it.
    pub(crate) fn hold_next_resolve(&self) -> Held {
        hold(&self.hold_resolve)
    }

    /// The next connect waits until the test releases it.
    pub(crate) fn hold_next_connect(&self) -> Held {
        hold(&self.hold_connect)
    }
}

impl Network for TestNet {
    fn resolve(&self, host: &str, port: u16) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        self.resolved.lock().unwrap().push(host.to_owned());
        let result = self.answer(host, port);
        let held = self.hold_resolve.lock().unwrap().take();
        Box::pin(async move {
            wait_for(held).await;
            result
        })
    }

    /// Never held, and kept apart, so a probe at the proxy's start leaves `resolved` alone.
    fn resolve_alone(
        &self,
        host: &str,
        port: u16,
    ) -> BoxFuture<'static, io::Result<Vec<SocketAddr>>> {
        self.probed.lock().unwrap().push(host.to_owned());
        let result = self.answer(host, port);
        Box::pin(async move { result })
    }

    fn connect(&self, addr: SocketAddr) -> BoxFuture<'static, io::Result<Async<TcpStream>>> {
        self.dialled.lock().unwrap().push(addr);
        let mapped = self.routes.lock().unwrap().get(&addr).copied();
        let target = match mapped {
            Some(to) => Some(to),
            // The upstream proxy in the upstream tests listens on 127.0.0.1 itself.
            None if addr.ip().is_loopback() => Some(addr),
            None => None,
        };
        let held = self.hold_connect.lock().unwrap().take();
        Box::pin(async move {
            wait_for(held).await;
            match target {
                Some(to) => Async::<TcpStream>::connect(to).await,
                None => Err(io::Error::from(io::ErrorKind::ConnectionRefused)),
            }
        })
    }
}
