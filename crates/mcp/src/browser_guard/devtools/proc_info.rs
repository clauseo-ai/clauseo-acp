//! macOS: the part of `struct socket_fdinfo` the relay's peer check reads
//! (`proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`, `devtools/peer.rs`), mirrored from xnu's
//! `bsd/sys/proc_info.h` field for field up to the protocol union, which keeps only its TCP member
//! and pads to the union's size. The `libc` crate defines `vinfo_stat` and none of the rest.
//!
//! Every offset read is pinned below at compile time, as the header lays it out on every 64-bit
//! macOS (`x86_64` and `arm64` align `uint64_t` to 8): `soi_family` 160, `soi_kind` 232 and
//! `soi_proto` 240 in `socket_info`, after the 24-byte `proc_fileinfo`; in `in_sockinfo`,
//! `insi_vflag` 24, `insi_faddr` 32 and `insi_laddr` 48, each address a 16-byte `in4in6_addr`
//! whose IPv4 address is its last 4 bytes. So from the start of `socket_fdinfo`: `insi_vflag`
//! at 288, the foreign IPv4 address at 308 and the local one at 324, as route 2's review recalled
//! them. The union's biggest member is `un_sockinfo` (two `uint64_t` and two 255-byte
//! addresses: 528 bytes), so `socket_fdinfo` is 792 bytes. Built on every system for its tests;
//! the check is the same on any 64-bit target.

use std::mem::{offset_of, size_of};

/// `SOCKINFO_TCP`, a `socket_info`'s `soi_kind` for a TCP socket.
pub(crate) const SOCKINFO_TCP: i32 = 2;
/// `AF_INET`.
pub(crate) const AF_INET: i32 = 2;
/// `INI_IPV4`, an `in_sockinfo`'s `insi_vflag` bit for IPv4.
pub(crate) const INI_IPV4: u8 = 0x1;

/// `struct proc_fileinfo`, 24 bytes; never read: `uint32_t fi_openflags, fi_status; off_t
/// fi_offset; int32_t fi_type; uint32_t fi_guardflags`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct ProcFileInfo {
    pub(crate) words: [u64; 3],
}

/// `struct vinfo_stat`, 136 bytes; never read.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct VinfoStat {
    pub(crate) words: [u64; 17],
}

/// `struct sockbuf_info`, 24 bytes; never read: `uint32_t sbi_cc, sbi_hiwat, sbi_mbcnt,
/// sbi_mbmax, sbi_lowat; short sbi_flags, sbi_timeo`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct SockbufInfo {
    pub(crate) words: [u32; 6],
}

/// `struct in4in6_addr`: an IPv4 address (network order) after 12 bytes of padding. The whole
/// `union { in4in6_addr; in6_addr }` is the same 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct In4In6Addr {
    pub(crate) i46a_pad32: [u32; 3],
    pub(crate) i46a_addr4: [u8; 4],
}

/// `struct in_sockinfo`. Ports are the network-order `u_short`s, stored in an `int`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct InSockInfo {
    pub(crate) insi_fport: i32,
    pub(crate) insi_lport: i32,
    pub(crate) insi_gencnt: u64,
    pub(crate) insi_flags: u32,
    pub(crate) insi_flow: u32,
    pub(crate) insi_vflag: u8,
    pub(crate) insi_ip_ttl: u8,
    pub(crate) rfu_1: u32,
    pub(crate) insi_faddr: In4In6Addr,
    pub(crate) insi_laddr: In4In6Addr,
    /// `insi_v4` (one byte, padded) and `insi_v6` (12 bytes).
    pub(crate) insi_v4_v6: [u32; 4],
}

/// `struct tcp_sockinfo`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct TcpSockInfo {
    pub(crate) tcpsi_ini: InSockInfo,
    pub(crate) tcpsi_state: i32,
    /// `TSI_T_NTIMERS` (4).
    pub(crate) tcpsi_timer: [i32; 4],
    pub(crate) tcpsi_mss: i32,
    pub(crate) tcpsi_flags: u32,
    pub(crate) rfu_1: u32,
    pub(crate) tcpsi_tp: u64,
}

/// `struct socket_info`, its `soi_proto` union as its TCP member and the rest of the union.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct SocketInfo {
    pub(crate) soi_stat: VinfoStat,
    pub(crate) soi_so: u64,
    pub(crate) soi_pcb: u64,
    pub(crate) soi_type: i32,
    pub(crate) soi_protocol: i32,
    pub(crate) soi_family: i32,
    pub(crate) soi_options: i16,
    pub(crate) soi_linger: i16,
    pub(crate) soi_state: i16,
    pub(crate) soi_qlen: i16,
    pub(crate) soi_incqlen: i16,
    pub(crate) soi_qlimit: i16,
    pub(crate) soi_timeo: i16,
    pub(crate) soi_error: u16,
    pub(crate) soi_oobmark: u32,
    pub(crate) soi_rcv: SockbufInfo,
    pub(crate) soi_snd: SockbufInfo,
    pub(crate) soi_kind: i32,
    pub(crate) rfu_1: u32,
    pub(crate) soi_proto: TcpSockInfo,
    /// The rest of the 528-byte union (`un_sockinfo` is its biggest member).
    pub(crate) soi_proto_rest: UnionRest,
}

/// The bytes of the `soi_proto` union past its TCP member.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct UnionRest(pub(crate) [u64; (UNION_BYTES - size_of::<TcpSockInfo>()) / 8]);

impl Default for UnionRest {
    fn default() -> UnionRest {
        UnionRest([0; (UNION_BYTES - size_of::<TcpSockInfo>()) / 8])
    }
}

/// `struct socket_fdinfo`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct SocketFdInfo {
    pub(crate) pfi: ProcFileInfo,
    pub(crate) psi: SocketInfo,
}

/// The size of `socket_info`'s `soi_proto` union.
const UNION_BYTES: usize = 528;

/// The bytes of a `socket_fdinfo` the check reads: up to the end of the local address.
#[cfg(target_os = "macos")]
pub(crate) const READ_BYTES: usize = LADDR4_AT + 4;
/// `insi_vflag`, from the start of `socket_fdinfo`.
const VFLAG_AT: usize = offset_of!(SocketFdInfo, psi.soi_proto.tcpsi_ini.insi_vflag);
/// The foreign IPv4 address, from the start of `socket_fdinfo`.
const FADDR4_AT: usize = offset_of!(SocketFdInfo, psi.soi_proto.tcpsi_ini.insi_faddr.i46a_addr4);
/// The local IPv4 address, from the start of `socket_fdinfo`.
const LADDR4_AT: usize = offset_of!(SocketFdInfo, psi.soi_proto.tcpsi_ini.insi_laddr.i46a_addr4);

// `sys/proc_info.h`'s layout, as the module's comment counts it.
const _: () = {
    assert!(size_of::<ProcFileInfo>() == 24);
    assert!(size_of::<VinfoStat>() == 136);
    assert!(size_of::<SockbufInfo>() == 24);
    assert!(size_of::<In4In6Addr>() == 16);
    assert!(size_of::<InSockInfo>() == 80);
    assert!(offset_of!(InSockInfo, insi_vflag) == 24);
    assert!(offset_of!(InSockInfo, insi_faddr) == 32);
    assert!(offset_of!(InSockInfo, insi_laddr) == 48);
    assert!(offset_of!(In4In6Addr, i46a_addr4) == 12);
    assert!(size_of::<TcpSockInfo>() == 120);
    assert!(offset_of!(SocketInfo, soi_family) == 160);
    assert!(offset_of!(SocketInfo, soi_kind) == 232);
    assert!(offset_of!(SocketInfo, soi_proto) == 240);
    assert!(size_of::<SocketInfo>() == 768);
    assert!(offset_of!(SocketFdInfo, psi) == 24);
    assert!(size_of::<SocketFdInfo>() == 792);
    assert!(VFLAG_AT == 288);
    assert!(FADDR4_AT == 308);
    assert!(LADDR4_AT == 324);
};

/// Whether `info` is an IPv4 TCP socket from 127.0.0.1:`client` to 127.0.0.1:`relay`. Pure.
pub(crate) fn is_client_socket(info: &SocketFdInfo, client: u16, relay: u16) -> bool {
    let ini = &info.psi.soi_proto.tcpsi_ini;
    // The low 16 bits of the `int` hold the network-order port.
    let port = |value: i32| u16::from_be(u16::try_from(value & 0xFFFF).unwrap_or(0));
    let loopback = [127, 0, 0, 1];
    info.psi.soi_kind == SOCKINFO_TCP
        && info.psi.soi_family == AF_INET
        && ini.insi_vflag & INI_IPV4 != 0
        && ini.insi_laddr.i46a_addr4 == loopback
        && ini.insi_faddr.i46a_addr4 == loopback
        && port(ini.insi_lport) == client
        && port(ini.insi_fport) == relay
}
