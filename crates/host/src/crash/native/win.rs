//! Windows: the unhandled-exception filter, called for an exception no handler took. It records
//! the exception code, then calls the filter that was set before, or lets the search go on, so
//! Windows Error Reporting and the process's end are as they would have been.

#![expect(
    unsafe_code,
    reason = "SetUnhandledExceptionFilter installs the filter, which reads the system's record"
)]

use std::sync::OnceLock;

use ::windows::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, LPTOP_LEVEL_EXCEPTION_FILTER, SetUnhandledExceptionFilter,
};

/// `EXCEPTION_CONTINUE_SEARCH`: the filter did not handle the exception.
const CONTINUE_SEARCH: i32 = 0;

/// The filter set before ours.
static PREVIOUS: OnceLock<LPTOP_LEVEL_EXCEPTION_FILTER> = OnceLock::new();

/// Installs the filter. Always takes.
pub(super) fn install() -> bool {
    // SAFETY: `filter` lives as long as the process and calls only `WriteFile` on an open file
    // and the previous filter.
    let previous = unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    let _first = PREVIOUS.set(previous);
    true
}

/// The filter: records the code, then chains.
unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
    // SAFETY: the system passes a valid EXCEPTION_POINTERS and record, checked for null anyway.
    let code = unsafe { info.as_ref().and_then(|info| info.ExceptionRecord.as_ref()) }
        .map(|record| record.ExceptionCode.0);
    if let Some(code) = code {
        super::on_fault(code.cast_unsigned());
    }
    match PREVIOUS.get().copied().flatten() {
        // SAFETY: the filter set before ours, called as the system would have, with its argument.
        Some(previous) => unsafe { previous(info) },
        None => CONTINUE_SEARCH,
    }
}
