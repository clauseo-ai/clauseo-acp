//! `WebView2`'s `ProcessFailed`: a render process that exited is a renderer gone, with
//! `WebView2`'s reason; every other kind (the browser, the GPU, a frame's renderer, an unresponsive
//! renderer still running) is not, and `TERMINATED` (ended from outside) is not a crash.

#![expect(unsafe_code, reason = "WebView2's COM calls")]

use clauseo_counts::schema::RendererGoneReason;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
    COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED, COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE,
    ICoreWebView2Controller, ICoreWebView2ProcessFailedEventArgs,
    ICoreWebView2ProcessFailedEventArgs2,
};
use webview2_com::ProcessFailedEventHandler;
use windows::core::Interface as _;

/// Calls `gone` for each render process of `controller`'s webview that exits, with its reason.
pub(super) fn watch(
    controller: &ICoreWebView2Controller,
    gone: impl Fn(RendererGoneReason) + 'static,
) -> windows::core::Result<()> {
    // SAFETY: a live controller from Tauri, on the thread it was made on (`with_webview`).
    let webview = unsafe { controller.CoreWebView2() }?;
    let handler = ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
        if let Some(reason) = args.as_ref().and_then(reason_of) {
            gone(reason);
        }
        Ok(())
    }));
    let mut token = 0_i64;
    // SAFETY: the handler is a COM object WebView2 holds a reference to; the token is ours.
    unsafe { webview.add_ProcessFailed(&handler, &raw mut token) }
}

/// The reason, for a render process that exited; `None` for any other failure and for an end
/// from outside.
fn reason_of(args: &ICoreWebView2ProcessFailedEventArgs) -> Option<RendererGoneReason> {
    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
    // SAFETY: valid event arguments from WebView2, during its event.
    unsafe { args.ProcessFailedKind(&raw mut kind) }.ok()?;
    if kind != COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
        return None;
    }
    // An older runtime has no reason.
    let Ok(args2) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() else {
        return Some(RendererGoneReason::Other);
    };
    let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
    // SAFETY: as above.
    if unsafe { args2.Reason(&raw mut reason) }.is_err() {
        return Some(RendererGoneReason::Other);
    }
    Some(match reason {
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED => return None,
        COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED => RendererGoneReason::Crashed,
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY => RendererGoneReason::OutOfMemory,
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE => RendererGoneReason::Unresponsive,
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED => RendererGoneReason::LaunchFailed,
        _ => RendererGoneReason::Other,
    })
}
