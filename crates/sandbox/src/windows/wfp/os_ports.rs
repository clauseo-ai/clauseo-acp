//! The machine's side of tier B's port check (S15 PR7; `port_check.rs` decides):
//!
//! - Windows' TCP dynamic range, from WMI: the class `MSFT_NetTCPSetting` in `ROOT\StandardCimv2`,
//!   properties `DynamicPortRangeStartPort` and `DynamicPortRangeNumberOfPorts` (the class behind
//!   `Get-NetTCPSetting` and `netsh int ipv4 show dynamicport tcp`; learn.microsoft.com,
//!   windows/win32/fwp/wmi/nettcpipprov/msft-nettcpsetting), every setting's range;
//! - the excluded ports, by test binds (`ports::refused_ports` says why there is no list to
//!   read);
//! - the TCP listeners, from `GetExtendedTcpTable` with `TCP_TABLE_OWNER_PID_LISTENER`, IPv4 and
//!   IPv6, any address, each with its process's program (`QueryFullProcessImageNameW`);
//! - for the slot claims and the browser's gate (`ports.rs`): every TCP socket's local port
//!   (`TCP_TABLE_OWNER_PID_ALL`), the listeners' processes, and whether a process is in a Job
//!   Object (`IsProcessInJob`).

#![expect(
    unsafe_code,
    reason = "FFI to WMI, GetExtendedTcpTable and the process image calls"
)]

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_CLASS,
    TCP_TABLE_OWNER_PID_ALL, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::JobObjects::IsProcessInJob;
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::{VARIANT, VT_I2, VT_I4, VT_UI2, VariantClear};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, WBEM_FLAG_CONNECT_USE_MAX_WAIT, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WbemLocator,
};
use windows::core::{BOOL, BSTR, HRESULT, PCWSTR, PWSTR, w};

use super::check::NetworkGap;
use super::port_check::{Listener, PortsSeen, dynamic_span, is_ours, ports_usable};
use crate::ports::{TIER_B_PORTS, refused_ports};

/// `ERROR_INSUFFICIENT_BUFFER`.
const INSUFFICIENT_BUFFER: u32 = 122;

/// Checks tier B's ports on this machine ([`ports_usable`]): Windows' dynamic range and its
/// excluded ports always, and with `launch` every listener in the range too, which must be this
/// process's (its pid; the proxy and the relay run in the host). Fails closed when the machine
/// can't be read.
pub fn check_ports(launch: bool) -> Result<(), NetworkGap> {
    let dynamic = dynamic_ranges()?;
    // The listeners first, then the test binds, then the listeners again for any port that
    // refused a bind: a listener that came up in between is named as one, not as a reservation.
    let mut found = listeners()?;
    let listening: BTreeSet<u16> = found.iter().map(|listener| listener.port).collect();
    let mut reserved = refused_ports(&listening);
    if !reserved.is_empty() {
        found = listeners()?;
        reserved.retain(|port| !found.iter().any(|listener| listener.port == *port));
    }
    let seen = PortsSeen {
        dynamic,
        reserved,
        listeners: if launch { found } else { Vec::new() },
    };
    ports_usable(&seen, |listener| is_ours(listener, std::process::id()))
}

/// How long the whole WMI read may take before the check fails closed.
const WMI_LIMIT: Duration = Duration::from_secs(5);

/// The longest one `IEnumWbemClassObject::Next` call may wait, in milliseconds.
const NEXT_LIMIT_MS: i32 = 4_000;

/// Windows' TCP dynamic port ranges as WMI reports them, every `MSFT_NetTCPSetting`'s, for
/// the VM rows.
pub fn dynamic_port_ranges() -> Result<Vec<(u16, u16)>, NetworkGap> {
    dynamic_ranges()
}

/// `result` as a Win32 or HRESULT code.
fn code(result: HRESULT) -> u32 {
    u32::from_ne_bytes(result.0.to_ne_bytes())
}

/// The gap for a failed `call`.
fn unchecked(call: &'static str, code: u32) -> NetworkGap {
    NetworkGap::PortsUnchecked { call, code }
}

/// Every setting's dynamic range, read on a thread of its own in COM's multithreaded
/// apartment, so the caller's thread (the host's, any apartment) is left as it is. The read
/// gets [`WMI_LIMIT`]; past it the check fails closed and the thread is left to finish.
fn dynamic_ranges() -> Result<Vec<(u16, u16)>, NetworkGap> {
    let (send, receive) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("clauseo-port-check".to_owned())
        .spawn(move || drop(send.send(read_in_com())));
    if spawned.is_err() {
        return Err(unchecked("the port check's WMI thread", 0));
    }
    match receive.recv_timeout(WMI_LIMIT) {
        Ok(ranges) => ranges,
        Err(_) => Err(unchecked("WMI (no answer within 5 s)", 0)),
    }
}

/// [`query_dynamic_ranges`] inside COM's multithreaded apartment on this thread.
fn read_in_com() -> Result<Vec<(u16, u16)>, NetworkGap> {
    // SAFETY: this new thread enters COM once and leaves it below.
    let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if entered.is_err() {
        return Err(unchecked("CoInitializeEx", code(entered)));
    }
    let ranges = query_dynamic_ranges();
    // SAFETY: balances this thread's successful CoInitializeEx.
    unsafe { CoUninitialize() };
    ranges
}

/// [`dynamic_ranges`] on a thread that has entered COM.
fn query_dynamic_ranges() -> Result<Vec<(u16, u16)>, NetworkGap> {
    let wmi = |call| move |error: windows::core::Error| unchecked(call, code(error.code()));
    // SAFETY: COM is entered on this thread; the class id is WMI's locator.
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
            .map_err(wmi("CoCreateInstance(WbemLocator)"))?;
    let empty = BSTR::new();
    // SAFETY: live strings; no context; the current user's credentials.
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from("ROOT\\StandardCimv2"),
            &empty,
            &empty,
            &empty,
            // Bounded by Windows (two minutes at most); WMI_LIMIT bounds the whole read.
            WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
            &empty,
            None,
        )
    }
    .map_err(wmi("IWbemLocator::ConnectServer"))?;
    // SAFETY: `services` is a live proxy; calls go as this process, impersonation allowed.
    unsafe {
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(wmi("CoSetProxyBlanket"))?;
    let query = BSTR::from(
        "SELECT DynamicPortRangeStartPort, DynamicPortRangeNumberOfPorts FROM MSFT_NetTCPSetting",
    );
    // SAFETY: live strings; no context.
    let rows = unsafe {
        services.ExecQuery(
            &BSTR::from("WQL"),
            &query,
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )
    }
    .map_err(wmi("IWbemServices::ExecQuery"))?;
    let mut spans = Vec::new();
    loop {
        let mut row = [None];
        let mut returned = 0u32;
        // SAFETY: one live slot and a live count.
        let next = unsafe { rows.Next(NEXT_LIMIT_MS, &mut row, &raw mut returned) };
        if next.is_err() {
            return Err(unchecked("IEnumWbemClassObject::Next", code(next)));
        }
        // WBEM_S_TIMEDOUT: no row within the limit.
        if next.0 == 0x0004_0004 {
            return Err(unchecked(
                "IEnumWbemClassObject::Next (timed out)",
                code(next),
            ));
        }
        let [Some(row)] = row else { break };
        let start = number(&row, w!("DynamicPortRangeStartPort"))?;
        let count = number(&row, w!("DynamicPortRangeNumberOfPorts"))?;
        if let (Some(start), Some(count)) = (start, count)
            && let Some(span) = dynamic_span(start, count)
        {
            spans.push(span);
        }
    }
    if spans.is_empty() {
        return Err(unchecked("MSFT_NetTCPSetting (no dynamic port range)", 0));
    }
    Ok(spans)
}

/// `row`'s property `name` as a port number; `None` when it is null or not a number.
fn number(row: &IWbemClassObject, name: PCWSTR) -> Result<Option<u16>, NetworkGap> {
    let mut value = VARIANT::default();
    // SAFETY: a live VARIANT, cleared below; no type or flavour asked for.
    unsafe { row.Get(name, 0, &raw mut value, None, None) }
        .map_err(|error| unchecked("IWbemClassObject::Get", code(error.code())))?;
    // SAFETY: the union is read as the member its type tag names.
    let number = unsafe {
        let inner = &value.Anonymous.Anonymous;
        match inner.vt {
            vt if vt == VT_I4 => u16::try_from(inner.Anonymous.lVal).ok(),
            vt if vt == VT_UI2 => Some(inner.Anonymous.uiVal),
            vt if vt == VT_I2 => u16::try_from(inner.Anonymous.iVal).ok(),
            _ => None,
        }
    };
    // SAFETY: `value` came from Get and is cleared once, here.
    drop(unsafe { VariantClear(&raw mut value) });
    Ok(number)
}

/// The TCP listeners in [`TIER_B_PORTS`], IPv4 and IPv6, with their programs.
fn listeners() -> Result<Vec<Listener>, NetworkGap> {
    let mut found = Vec::new();
    for family in [AF_INET.0, AF_INET6.0] {
        let table = table(family, TCP_TABLE_OWNER_PID_LISTENER)?;
        for (port, pid) in rows(&table, family == AF_INET6.0) {
            if TIER_B_PORTS.contains(&port) {
                found.push(Listener {
                    port,
                    pid,
                    image: process_image(pid),
                });
            }
        }
    }
    Ok(found)
}

/// Every TCP listener, IPv4 and IPv6, any address, as (port, process id).
pub(crate) fn listener_rows() -> Result<Vec<(u16, u32)>, NetworkGap> {
    every_row(TCP_TABLE_OWNER_PID_LISTENER)
}

/// The local port of every TCP socket, IPv4 and IPv6, any state (listening, connected, or
/// closing in `TIME_WAIT`): a port Windows lent as a connection's source port is here.
pub(crate) fn ports_in_use() -> Result<BTreeSet<u16>, NetworkGap> {
    Ok(every_row(TCP_TABLE_OWNER_PID_ALL)?
        .into_iter()
        .map(|(port, _)| port)
        .collect())
}

/// Every row of `class`'s table, IPv4 then IPv6, as (port, process id).
fn every_row(class: TCP_TABLE_CLASS) -> Result<Vec<(u16, u32)>, NetworkGap> {
    let mut found = Vec::new();
    for family in [AF_INET.0, AF_INET6.0] {
        found.extend(rows(&table(family, class)?, family == AF_INET6.0));
    }
    Ok(found)
}

/// Whether process `pid` is in the Job Object `job`; `None` when it can't be opened or asked.
/// A gate, not the boundary: the pid is read from the TCP table first, so a process that exited
/// and whose pid Windows gave to another in between is asked about instead.
pub(crate) fn process_in_job(pid: u32, job: std::os::windows::io::RawHandle) -> Option<bool> {
    // SAFETY: an ordinary open; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut inside = BOOL::default();
    // SAFETY: `process` is open; `job` is the caller's live job handle, borrowed for the call;
    // `inside` is a live out-pointer.
    let asked = unsafe { IsProcessInJob(process, Some(HANDLE(job)), &raw mut inside) };
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseHandle(process) });
    asked.ok()?;
    Some(inside.as_bool())
}

/// `GetExtendedTcpTable`'s `class` table for `family`, in an aligned buffer.
fn table(family: u16, class: TCP_TABLE_CLASS) -> Result<Vec<u64>, NetworkGap> {
    let mut buffer: Vec<u64> = Vec::new();
    for _ in 0..8 {
        let mut size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        let pointer = (!buffer.is_empty()).then(|| buffer.as_mut_ptr().cast());
        // SAFETY: `pointer` is `size` writable bytes, or none to ask for the size.
        let result = unsafe {
            GetExtendedTcpTable(pointer, &raw mut size, false, u32::from(family), class, 0)
        };
        match result {
            0 => return Ok(buffer),
            INSUFFICIENT_BUFFER => buffer = vec![0; usize::try_from(size).unwrap_or(0) / 8 + 1],
            other => return Err(unchecked("GetExtendedTcpTable", other)),
        }
    }
    Err(unchecked("GetExtendedTcpTable", INSUFFICIENT_BUFFER))
}

/// Each row's local port and process, from a table [`table`] filled.
fn rows(table: &[u64], v6: bool) -> Vec<(u16, u32)> {
    let port = |raw: u32| u16::from_be(u16::try_from(raw & 0xFFFF).unwrap_or(0));
    if table.is_empty() {
        return Vec::new();
    }
    // SAFETY: GetExtendedTcpTable wrote the family's table at the start of the aligned buffer:
    // its count, then that many rows.
    unsafe {
        if v6 {
            let head = &*table.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>();
            let all = std::slice::from_raw_parts(head.table.as_ptr(), head.dwNumEntries as usize);
            all.iter()
                .map(|row| (port(row.dwLocalPort), row.dwOwningPid))
                .collect()
        } else {
            let head = &*table.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
            let all = std::slice::from_raw_parts(head.table.as_ptr(), head.dwNumEntries as usize);
            all.iter()
                .map(|row| (port(row.dwLocalPort), row.dwOwningPid))
                .collect()
        }
    }
}

/// The program file of process `pid`; `None` when it can't be opened (System's, say).
fn process_image(pid: u32) -> Option<String> {
    // SAFETY: an ordinary open; the handle is closed below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let image = image_of(handle);
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseHandle(handle) });
    image
}

/// The program file of the process `handle` names.
fn image_of(handle: HANDLE) -> Option<String> {
    let mut name = vec![0u16; 1024];
    let mut len = u32::try_from(name.len()).unwrap_or(0);
    // SAFETY: `name` holds `len` UTF-16 units.
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(name.as_mut_ptr()),
            &raw mut len,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(name.get(..len as usize)?))
}
