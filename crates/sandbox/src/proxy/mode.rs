//! Which hosts a launch's proxy admits: those on its allowlist, or on the browser listener any
//! public host.

/// Which hosts a launch's proxy admits ([`ProxyConfig::mode`](crate::ProxyConfig::mode)).
///
/// The mode moves one check only, the allowlist's, and only on the browser listener: the agent
/// listener asks the allowlist in both modes, so a jailed agent never gets open egress. In both
/// modes the proxy still checks the listener's credential, the form and scheme, the silent
/// list, the block list (with the lawyer's exact allows skipping it), the port (443 for
/// CONNECT, 80 for plain `http://`), a private IP literal or `localhost` name, and every
/// address a name resolves to before it connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyMode {
    /// Only a host on the launch's allowlist passes (or one `ProxyHandle::allow` added), on
    /// both listeners; any other is refused with `HostRefusal::NotAllowed`. From
    /// `ProxyConfig::new`.
    #[default]
    Allowlist,
    /// On the browser listener any public host passes: the allowlist is not asked, and
    /// `ProxyHandle::revoke` closes none of its tunnels. Through an upstream proxy, a name local
    /// DNS cannot check is refused there rather than sent on. The agent listener behaves as in
    /// `Allowlist`. A revoke of the lawyer's allow for a host the block list names still closes
    /// its tunnels on both.
    Open,
}
