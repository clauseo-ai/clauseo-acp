//! The watcher's ETW glue: the real-time session [`SESSION_NAME`], Kernel-File enabled on it
//! with each event's SID, and the consumer thread (`OpenTraceW` in real-time, event-record mode,
//! then `ProcessTrace`), whose callback hands each event's raw parts to the [`Consumer`] and
//! its records to a bounded channel. Thin on purpose: every decision is in `consumer.rs`. Every
//! few seconds it queries the session for the events ETW lost (`losses.rs`).
#![expect(unsafe_code, reason = "FFI to the ETW controller and consumer calls")]

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_NO_SYSTEM_RESOURCES, ERROR_SUCCESS, ERROR_WMI_INSTANCE_NOT_FOUND,
    WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{GetLogicalDrives, QueryDosDeviceW};
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, ENABLE_TRACE_PARAMETERS,
    ENABLE_TRACE_PARAMETERS_VERSION_2, EVENT_CONTROL_CODE_DISABLE_PROVIDER,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_ENABLE_PROPERTY_SID, EVENT_HEADER_EXT_TYPE_SID,
    EVENT_HEADER_FLAG_64_BIT_HEADER, EVENT_RECORD, EVENT_TRACE_CONTROL_QUERY,
    EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE, ProcessTrace, StartTraceW,
    WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::core::{GUID, HSTRING, PWSTR};

use super::consumer::{
    CREATE_ID, Consumer, END_ID, EXT_TYPE_SID, KERNEL_FILE, RawEvent, Record, sid_in,
};
use super::losses::Losses;
use super::{Delivery, SESSION_NAME, TraceError};
use crate::windows::refresh::event_log;
use crate::windows::sids::Sid;

/// Kernel-File's keywords `CREATE` (0x80) and `OP_END` (0x40).
const KEYWORDS: u64 = 0xC0;
/// `TRACE_LEVEL_VERBOSE`.
const LEVEL: u8 = 5;
/// Each buffer's size, in KB, and how many: at most 4 MB.
const BUFFER_KB: u32 = 64;
const MIN_BUFFERS: u32 = 4;
const MAX_BUFFERS: u32 = 64;
/// How often the buffers are flushed to the consumer, in seconds: the reporter's latency.
const FLUSH_SECONDS: u32 = 1;
/// `ClientContext` 1: query-performance-counter timestamps.
const QPC: u32 = 1;
/// The records the channel holds before the consumer counts the rest as lost.
const CHANNEL_CAP: usize = 1024;
// The pure SID lookup's item type is the header's.
const _: () = assert!(EVENT_HEADER_EXT_TYPE_SID == EXT_TYPE_SID as u32);

/// Room for a session name and a log file name after the properties, in UTF-16 units each.
const NAME_ROOM: usize = 1024;

/// A running trace: its consumer thread and what it produces.
pub(crate) struct Trace {
    thread: Option<JoinHandle<()>>,
    records: Receiver<Record>,
    lost: Arc<AtomicU64>,
    losses: Losses,
    /// `GetTickCount64` when the trace began.
    began_ms: u64,
    session: CONTROLTRACE_HANDLE,
    /// Every Kernel-File event the callback saw, for the breaker.
    events: Arc<AtomicU64>,
    /// Set by a pause: the callback clears the pending Creates before its next event.
    clear: Arc<AtomicBool>,
}

/// The callback's state, owned by the consumer thread and reached through `UserContext`.
struct Callback {
    consumer: Consumer,
    out: SyncSender<Record>,
    lost: Arc<AtomicU64>,
    events: Arc<AtomicU64>,
    clear: Arc<AtomicBool>,
    /// `GetTickCount64` when the consumer began.
    began_ms: u64,
}

/// `EVENT_TRACE_PROPERTIES` with room for both names after it, 8-byte aligned.
struct Properties(Vec<u64>);

impl Properties {
    fn new() -> Properties {
        let head = size_of::<EVENT_TRACE_PROPERTIES>();
        let size = head + 2 * NAME_ROOM * 2;
        let mut properties = Properties(vec![0u64; size.div_ceil(8)]);
        let fields = properties.fields();
        fields.Wnode.BufferSize = u32::try_from(size).unwrap_or(u32::MAX);
        fields.LoggerNameOffset = u32::try_from(head).unwrap_or(0);
        fields.LogFileNameOffset = u32::try_from(head + NAME_ROOM * 2).unwrap_or(0);
        properties
    }

    fn fields(&mut self) -> &mut EVENT_TRACE_PROPERTIES {
        // SAFETY: the buffer is 8-byte aligned, zeroed and at least the struct's size, and the
        // struct is plain data for which all zeroes is valid.
        unsafe { &mut *self.0.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() }
    }

    fn as_mut_ptr(&mut self) -> *mut EVENT_TRACE_PROPERTIES {
        self.0.as_mut_ptr().cast()
    }
}

/// `call` failed with `code`, as a line for the log.
fn failed(call: &str, code: WIN32_ERROR) -> String {
    format!("{call} failed ({})", code.0)
}

/// Stops the session [`SESSION_NAME`]: `Ok(true)` when one ran, `Ok(false)` when none did.
pub(crate) fn stop_session() -> Result<bool, String> {
    let mut properties = Properties::new();
    // SAFETY: the name is a live HSTRING and the properties' buffer outlives the call, with
    // room for both names at the offsets it gives.
    let code = unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            &HSTRING::from(SESSION_NAME),
            properties.as_mut_ptr(),
            EVENT_TRACE_CONTROL_STOP,
        )
    };
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_WMI_INSTANCE_NOT_FOUND => Ok(false),
        code => Err(failed("ControlTraceW", code)),
    }
}

/// The session's `EventsLost` and `RealTimeBuffersLost`.
fn query_session() -> Result<(u32, u32), String> {
    let mut properties = Properties::new();
    // SAFETY: as in `stop_session`.
    let code = unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            &HSTRING::from(SESSION_NAME),
            properties.as_mut_ptr(),
            EVENT_TRACE_CONTROL_QUERY,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(failed("ControlTraceW", code));
    }
    let fields = properties.fields();
    Ok((fields.EventsLost, fields.RealTimeBuffersLost))
}

/// Starts the session, a stale one stopped first, with real-time delivery.
fn start_session() -> Result<CONTROLTRACE_HANDLE, TraceError> {
    let other = |call: &str, code| TraceError::Other(failed(call, code));
    stop_session().map_err(TraceError::Other)?;
    let mut tries = 0;
    loop {
        let mut properties = Properties::new();
        let fields = properties.fields();
        fields.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        fields.Wnode.ClientContext = QPC;
        fields.BufferSize = BUFFER_KB;
        fields.MinimumBuffers = MIN_BUFFERS;
        fields.MaximumBuffers = MAX_BUFFERS;
        fields.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        fields.FlushTimer = FLUSH_SECONDS;
        // Real-time only: no log file.
        fields.LogFileNameOffset = 0;
        let mut handle = CONTROLTRACE_HANDLE::default();
        // SAFETY: `handle` is a live out-pointer; the name is a live HSTRING; the properties'
        // buffer outlives the call, sized as `Wnode.BufferSize` says.
        let code = unsafe {
            StartTraceW(
                &raw mut handle,
                &HSTRING::from(SESSION_NAME),
                properties.as_mut_ptr(),
            )
        };
        match code {
            ERROR_SUCCESS => return Ok(handle),
            // Started between the stop and now (another watcher can't be: one instance).
            ERROR_ALREADY_EXISTS if tries == 0 => {
                tries += 1;
                stop_session().map_err(TraceError::Other)?;
            }
            code => return Err(other("StartTraceW", code)),
        }
    }
}

impl Trace {
    /// Starts the session, enables Kernel-File on it and starts the consumer for `pool`. A
    /// failure stops the session again.
    pub(crate) fn start(pool: &[Sid]) -> Result<Trace, TraceError> {
        let session = start_session()?;
        if let Err(error) = enable(session) {
            drop(stop_session());
            return Err(error);
        }
        let (out, records) = sync_channel(CHANNEL_CAP);
        let lost = Arc::new(AtomicU64::new(0));
        let events = Arc::new(AtomicU64::new(0));
        let clear = Arc::new(AtomicBool::new(false));
        let callback = Box::new(Callback {
            consumer: Consumer::new(pool),
            out,
            lost: Arc::clone(&lost),
            events: Arc::clone(&events),
            clear: Arc::clone(&clear),
            // SAFETY: GetTickCount64 only reads the tick count.
            began_ms: unsafe { GetTickCount64() },
        });
        let context = Box::into_raw(callback);
        let mut name: Vec<u16> = SESSION_NAME.encode_utf16().chain([0]).collect();
        // SAFETY: plain data; all zeroes is a valid EVENT_TRACE_LOGFILEW.
        let mut logfile: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
        logfile.LoggerName = PWSTR(name.as_mut_ptr());
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        logfile.Context = context.cast::<c_void>();
        // SAFETY: `logfile` and the name it points at outlive the call.
        let handle = unsafe { OpenTraceW(&raw mut logfile) };
        if handle.Value == u64::MAX {
            let error = std::io::Error::last_os_error()
                .raw_os_error()
                .and_then(|code| u32::try_from(code).ok())
                .map_or(WIN32_ERROR(0), WIN32_ERROR);
            // SAFETY: `context` came from Box::into_raw above and nothing else holds it.
            drop(unsafe { Box::from_raw(context) });
            drop(stop_session());
            return Err(TraceError::Other(failed("OpenTraceW", error)));
        }
        let owned = context as usize;
        let thread = std::thread::Builder::new()
            .name("watch-trace".to_owned())
            .spawn(move || consume(handle, owned))
            .map_err(|error| {
                drop(stop_session());
                TraceError::Other(format!("the consumer thread: {error}"))
            })?;
        Ok(Trace {
            thread: Some(thread),
            records,
            lost,
            losses: Losses::default(),
            // SAFETY: GetTickCount64 only reads the tick count.
            began_ms: unsafe { GetTickCount64() },
            session,
            events,
            clear,
        })
    }

    /// Waits up to `wait` for a record, then takes every one waiting.
    pub(crate) fn records(&mut self, wait: Duration) -> Delivery {
        let mut records = Vec::new();
        match self.records.recv_timeout(wait) {
            Ok(first) => {
                records.push(first);
                records.extend(self.records.try_iter());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Delivery::Ended,
        }
        let mut lost = self.lost.swap(0, Ordering::Relaxed);
        // SAFETY: GetTickCount64 only reads the tick count.
        let now = Duration::from_millis(unsafe { GetTickCount64() }.saturating_sub(self.began_ms));
        if self.losses.due(now) {
            // A query that fails is tried again at the next due time.
            if let Ok((events, buffers)) = query_session() {
                lost = lost.saturating_add(self.losses.read(now, events, buffers));
            }
        }
        Delivery::Records(records, lost)
    }

    /// How many Kernel-File events the callback has seen.
    pub(crate) fn events(&self) -> u64 {
        self.events.load(Ordering::Relaxed)
    }

    /// Disables Kernel-File on the session, and has the callback clear its pending Creates.
    pub(crate) fn pause(&mut self) -> Result<(), String> {
        self.clear.store(true, Ordering::Relaxed);
        control(self.session, EVENT_CONTROL_CODE_DISABLE_PROVIDER.0).map_err(|error| match error {
            TraceError::ProviderFull(why) | TraceError::Other(why) => why,
        })
    }

    /// Enables Kernel-File on the session again.
    pub(crate) fn resume(&mut self) -> Result<(), String> {
        enable(self.session).map_err(|error| match error {
            TraceError::ProviderFull(why) | TraceError::Other(why) => why,
        })
    }

    /// Stops the session, which ends `ProcessTrace`, and waits for the consumer thread.
    pub(crate) fn stop(mut self) -> Result<(), String> {
        let stopped = stop_session().map(drop);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
        stopped
    }
}

/// Enables Kernel-File on `session` for Create and `OperationEnd`, with each event's SID.
fn enable(session: CONTROLTRACE_HANDLE) -> Result<(), TraceError> {
    control(session, EVENT_CONTROL_CODE_ENABLE_PROVIDER.0)
}

/// `EnableTraceEx2` with `code` (enable or disable) for Kernel-File on `session`.
fn control(session: CONTROLTRACE_HANDLE, code: u32) -> Result<(), TraceError> {
    let parameters = ENABLE_TRACE_PARAMETERS {
        Version: ENABLE_TRACE_PARAMETERS_VERSION_2,
        EnableProperty: EVENT_ENABLE_PROPERTY_SID,
        ..ENABLE_TRACE_PARAMETERS::default()
    };
    let provider = GUID::from_u128(KERNEL_FILE);
    // SAFETY: a live session handle; the GUID and the parameters outlive the call.
    let code = unsafe {
        EnableTraceEx2(
            session,
            &raw const provider,
            code,
            LEVEL,
            KEYWORDS,
            0,
            0,
            Some(&raw const parameters),
        )
    };
    match code {
        ERROR_SUCCESS => Ok(()),
        ERROR_NO_SYSTEM_RESOURCES => Err(TraceError::ProviderFull(failed("EnableTraceEx2", code))),
        code => Err(TraceError::Other(failed("EnableTraceEx2", code))),
    }
}

/// The consumer thread: `ProcessTrace` until the session stops, then the callback's state goes,
/// and with it the channel's sender, which [`Trace::records`] sees as the end.
fn consume(handle: PROCESSTRACE_HANDLE, context: usize) {
    // SAFETY: a handle OpenTraceW returned, used by this thread alone.
    let code = unsafe { ProcessTrace(&[handle], None, None) };
    if code != ERROR_SUCCESS {
        tracing::warn!(code = code.0, "the watcher's ProcessTrace ended");
    }
    // SAFETY: as above; ProcessTrace has returned, so no callback runs after this.
    let _closed = unsafe { CloseTrace(handle) };
    // SAFETY: `context` came from Box::into_raw in `Trace::start`; the callback that used it
    // can't run again, so this thread owns it alone.
    let callback = unsafe { Box::from_raw(context as *mut Callback) };
    let unknown = callback.consumer.counts().unknown_version;
    if unknown > 0 {
        let line = format!(
            "the sandbox's watcher: {unknown} Kernel-File events of an unknown version were \
             skipped"
        );
        event_log(true, &line);
    }
    drop(callback);
}

/// The callback: one event's header, SID item and payload to the consumer.
unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    // SAFETY: ETW passes a record valid for the call, or null.
    let Some(record) = (unsafe { record.as_ref() }) else {
        return;
    };
    let header = &record.EventHeader;
    let id = header.EventDescriptor.Id;
    if header.ProviderId != GUID::from_u128(KERNEL_FILE) {
        return;
    }
    // SAFETY: `UserContext` is the `Context` `Trace::start` gave, a live Callback that only
    // this thread's callbacks use, one at a time.
    let Some(state) = (unsafe { record.UserContext.cast::<Callback>().as_mut() }) else {
        return;
    };
    // The breaker counts every Kernel-File event, before anything else is read.
    state.events.fetch_add(1, Ordering::Relaxed);
    if state.clear.load(Ordering::Relaxed) && state.clear.swap(false, Ordering::Relaxed) {
        state.consumer.clear_pending();
    }
    if id != CREATE_ID && id != END_ID {
        return;
    }
    let data: &[u8] = if record.UserData.is_null() {
        &[]
    } else {
        // SAFETY: ETW gives `UserDataLength` bytes at `UserData`, valid for the call.
        unsafe {
            std::slice::from_raw_parts(
                record.UserData.cast::<u8>(),
                usize::from(record.UserDataLength),
            )
        }
    };
    let event = RawEvent {
        id,
        pid: header.ProcessId,
        sid: if id == CREATE_ID {
            sid_item(record)
        } else {
            None
        },
        data,
        wide: u32::from(header.Flags) & EVENT_HEADER_FLAG_64_BIT_HEADER != 0,
        version: header.EventDescriptor.Version,
        stamp: header.TimeStamp,
    };
    let Callback {
        consumer,
        out,
        lost,
        began_ms,
        ..
    } = state;
    // SAFETY: GetTickCount64 only reads the tick count.
    let now = Duration::from_millis(unsafe { GetTickCount64() }.saturating_sub(*began_ms));
    consumer.on_event(&event, now, &mut dos_devices, &mut |found| {
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = out.try_send(found) {
            lost.fetch_add(1, Ordering::Relaxed);
        }
    });
    let dropped = consumer.take_dropped();
    if dropped > 0 {
        lost.fetch_add(dropped, Ordering::Relaxed);
    }
}

/// The record's `EVENT_HEADER_EXT_TYPE_SID` item, when it has one: each item's type and bytes
/// to [`sid_in`], which picks it.
fn sid_item(record: &EVENT_RECORD) -> Option<&[u8]> {
    if record.ExtendedData.is_null() {
        return None;
    }
    // SAFETY: ETW gives `ExtendedDataCount` items at `ExtendedData`, valid for the call.
    let items = unsafe {
        std::slice::from_raw_parts(record.ExtendedData, usize::from(record.ExtendedDataCount))
    };
    sid_in(items.iter().map(|item| {
        let size = usize::from(item.DataSize);
        let data = usize::try_from(item.DataPtr)
            .ok()
            .filter(|at| *at != 0)
            // SAFETY: the item's `DataSize` bytes at `DataPtr` are valid for the call.
            .map(|at| unsafe { std::slice::from_raw_parts(at as *const u8, size) });
        (item.ExtType, data)
    }))
}

/// Each drive letter's device (`QueryDosDeviceW`), for the device map.
fn dos_devices() -> Vec<(char, String)> {
    // SAFETY: GetLogicalDrives only reads the drive mask.
    let mask = unsafe { GetLogicalDrives() };
    let mut buffer = vec![0u16; 1024];
    (0u8..26)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| {
            let letter = char::from(b'A' + bit);
            // SAFETY: the name is a live HSTRING and the buffer a live slice.
            let n =
                unsafe { QueryDosDeviceW(&HSTRING::from(format!("{letter}:")), Some(&mut buffer)) };
            let n = usize::try_from(n).ok().filter(|n| *n > 0)?;
            let target = buffer.get(..n)?;
            let end = target
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(target.len());
            Some((letter, String::from_utf16_lossy(target.get(..end)?)))
        })
        .collect()
}
