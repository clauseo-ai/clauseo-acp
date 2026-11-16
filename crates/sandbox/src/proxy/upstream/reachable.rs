//! Whether a proxy answers at all ([`reachable`]): desktop's refreshed route asks it before it
//! keeps an earlier proxy over a lookup with no answer, so a laptop off the firm's network does
//! not keep a proxy it cannot reach.

use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::Duration;

use crate::{Clock as _, SystemClock};

/// Whether `host` (a name or a bare IP literal, as in [`Route::Http`](super::Route::Http))
/// resolves and one of its addresses takes a TCP connection on `port`, all within `cap`. The
/// connection is closed at once with nothing sent. Blocking, for at most `cap`: the name is
/// resolved on a thread of its own, left to end there when it runs past `cap`. `false` when it
/// does not resolve, no address connects in time, or the thread does not start.
pub fn reachable(host: &str, port: u16, cap: Duration) -> bool {
    let clock = SystemClock::new();
    let (send, got) = std::sync::mpsc::sync_channel(1);
    let host = host.to_owned();
    let spawned = std::thread::Builder::new()
        .name("clauseo-proxy-probe".to_owned())
        .spawn(move || {
            let connects = |addr| {
                let left = cap.saturating_sub(clock.now());
                !left.is_zero() && TcpStream::connect_timeout(&addr, left).is_ok()
            };
            let reached = (host.as_str(), port)
                .to_socket_addrs()
                .is_ok_and(|mut addrs| addrs.any(connects));
            // The caller may have stopped waiting.
            let _sent = send.send(reached);
        });
    if let Err(err) = spawned {
        tracing::debug!(%err, "the proxy probe did not start");
        return false;
    }
    got.recv_timeout(cap).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::time::Duration;

    use super::reachable;
    use crate::{Clock as _, SystemClock};

    const CAP: Duration = Duration::from_secs(1);

    /// A listening port is reachable, and the probe sends it nothing.
    #[test]
    fn a_listening_proxy_is_reachable_and_is_sent_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(reachable("127.0.0.1", port, CAP));
        let (mut accepted, _) = listener.accept().unwrap();
        let mut sent = Vec::new();
        std::io::Read::read_to_end(&mut accepted, &mut sent).unwrap();
        assert!(sent.is_empty(), "connect and close, nothing sent");
    }

    /// A closed port and a name that does not resolve are not, within the cap.
    #[test]
    fn a_closed_port_or_unknown_name_is_unreachable_within_the_cap() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let clock = SystemClock::new();
        assert!(!reachable("127.0.0.1", port, CAP), "a closed port");
        assert!(!reachable("proxy.firm.invalid", 3128, CAP), "no such name");
        assert!(clock.now() < 2 * CAP + Duration::from_millis(500));
    }
}
